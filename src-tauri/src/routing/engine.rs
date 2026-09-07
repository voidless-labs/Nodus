/// Routing engine — wires WASAPI capture to WASAPI render according to the graph.
///
/// For each resolved device route (from_device → to_device) the engine:
///   1. Starts a LoopbackCapture on from_device (shared — one capture per device)
///   2. Starts an AudioRenderer on to_device subscribed to that capture's broadcast
///   3. Applies per-route volume and mute atomically (no lock on the hot path)
///
/// Splitter: one capture → multiple renderers (broadcast channel handles fanout).
/// Mixer: multiple captures → one renderer (the renderer receives from multiple senders;
///   currently approximated by separate renderers writing to the same device, which Windows
///   mixes in shared mode automatically).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
};

use thiserror::Error;
use tracing::{debug, info, warn};

use super::graph::{ActiveRoute, Graph, HubTap, RoutingGraph};
use super::node::{FxSpec, NodeId, RouteId};
use crate::audio::{
    dsp::{FxParams, FxProcessor},
    session::{
        clamp_volume, find_audio_pid_for_exe, get_device_capture_format, volume_to_atomic,
        AudioFrame, AudioRenderer, LoopbackCapture, ProcessLoopbackCapture, SessionError,
    },
    wasapi::AudioFormat,
};
use crate::virtual_audio::{virtual_capture::VirtualCapture, virtual_render::VirtualRender};
use tokio::sync::broadcast;

/// A capture source feeding one source's broadcast channel:
///   - Loopback: whole-device WASAPI loopback / input capture (device sources)
///   - ProcessLoopback: isolated per-app capture by PID (app sources)
///   - Virtual: the Nodus kernel driver's shared ring buffer (virtual sources)
enum CaptureSource {
    Loopback(LoopbackCapture),
    ProcessLoopback(ProcessLoopbackCapture),
    Virtual(VirtualCapture),
}

impl CaptureSource {
    /// Start (or re-subscribe to) the capture, yielding a frame receiver.
    fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
        match self {
            CaptureSource::Loopback(c) => c.start(),
            CaptureSource::ProcessLoopback(c) => c.start(),
            CaptureSource::Virtual(c) => c.start(),
        }
    }

    /// Current RMS level [0,1] for VU metering (t21: virtual sources metered too).
    fn current_level(&self) -> f32 {
        match self {
            CaptureSource::Loopback(c) => c.current_level(),
            CaptureSource::ProcessLoopback(c) => c.current_level(),
            CaptureSource::Virtual(c) => c.current_level(),
        }
    }

    fn stop(&self) {
        match self {
            CaptureSource::Loopback(c) => c.stop(),
            CaptureSource::ProcessLoopback(c) => c.stop(),
            CaptureSource::Virtual(c) => c.stop(),
        }
    }

    /// Link state of a *device* source (loopback / input) as (device_id, LINK_* code),
    /// for the source node's status dot. App (process) and virtual sources have no
    /// device link — their status comes from process detection / presence → None.
    fn device_link(&self) -> Option<(String, u8)> {
        match self {
            CaptureSource::Loopback(c) => Some((c.device_id().to_string(), c.link_state())),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("engine already running")]
    AlreadyRunning,
    #[error("engine not running")]
    NotRunning,
    #[error("session error: {0}")]
    Session(String),
}

/// Lock a mutex, recovering the guard even if a previous holder panicked.
/// Project rule: no `.unwrap()` in production code — a poisoned mutex must not
/// cascade into a panic on every subsequent engine call. The data behind the
/// lock is plain audio routing state; recovering it is safe.
fn lock_recover<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where a route delivers its audio:
///   - Wasapi: a real render endpoint (speakers, headphones, VB-Cable, …)
///   - VirtualMic: the kernel driver's virtual-microphone ring buffer
/// Both consume the same broadcast receiver and the same vol/mute/pan atomics,
/// so set_route_volume/mute/pan work identically for either sink.
enum RouteSink {
    Wasapi(AudioRenderer),
    VirtualMic(VirtualRender),
}

impl RouteSink {
    fn stop(&self) {
        match self {
            RouteSink::Wasapi(r) => r.stop(),
            RouteSink::VirtualMic(v) => v.stop(),
        }
    }

    /// Post-volume output level [0,1] of this sink (for the destination VU meter,
    /// t21: the virtual mic is metered too).
    fn current_level(&self) -> f32 {
        match self {
            RouteSink::Wasapi(r) => r.current_level(),
            RouteSink::VirtualMic(v) => v.current_level(),
        }
    }

    /// Link state (LINK_* code) of this sink's destination. Our own virtual mic is
    /// a kernel ring — it never has a Bluetooth-style dropout, so it's always online.
    fn link_state(&self) -> u8 {
        match self {
            RouteSink::Wasapi(r) => r.link_state(),
            RouteSink::VirtualMic(_) => crate::audio::session::LINK_ONLINE,
        }
    }
}

struct RouteHandles {
    volume: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    /// Stereo balance as f32 bits in [-1.0 .. 1.0] (shared with the render thread).
    pan: Arc<AtomicU32>,
    /// None for same-device routes (no sink, capture is suppressed to avoid feedback).
    sink: Option<RouteSink>,
    /// Edge ids source→…→output. Effective volume = product of these edges'
    /// volumes; effective mute = OR. Lets an intermediate edge (e.g. a Mixer-input
    /// slider) update this route live, not just the final edge into the output.
    chain: Vec<String>,
    /// Destination device id — used to aggregate output VU per device (t16).
    to_device_id: String,
    /// Where this route meets a Mixer/Splitter, for the per-input dots (t18 wave 6b).
    hub_taps: Vec<HubTap>,
    /// Key into `captures` — the tap's level source when no FX precedes the hub.
    capture_key: String,
}

struct CaptureHandle {
    capture: CaptureSource,
    sender_count: usize,
    exe_name: Option<String>,
    /// Actual format the capture produces — renderers fed by it must use this so
    /// WASAPI AUTOCONVERTPCM resamples/remixes source→output device correctly.
    format: AudioFormat,
}

pub struct RoutingEngine {
    graph: Arc<Mutex<Graph>>,
    running: Arc<AtomicBool>,
    captures: Arc<Mutex<HashMap<String, CaptureHandle>>>,
    /// Keyed by UI route id. A single UI edge can resolve to several physical
    /// render paths (e.g. a Mixer→Output edge is traversed once per input
    /// source), so each id maps to a list of handles, not a single one.
    /// All handles under one id share mute/volume — that edge's control.
    routes: Arc<Mutex<HashMap<String, Vec<RouteHandles>>>>,
    format: AudioFormat,
    /// Serializes engine restarts (apply_graph/start/stop) so concurrent calls don't
    /// interleave their stop→start sequences. Deliberately separate from the graph/routes
    /// locks: real-time set_route_volume/mute never take it, so a graph apply (which holds
    /// it across the ~80ms WASAPI settle) can't stall the volume slider.
    restart_lock: Mutex<()>,
    /// JSON of the last-applied graph. A re-apply of an identical graph while
    /// running is a no-op — no teardown — so redundant applies don't flicker the
    /// engine (on→off→on) or drop a self-healing capture whose app is out. (t19)
    last_applied: Mutex<Option<String>>,
    /// Live FX parameters keyed per FX node id (t18). Rebuilt on each apply from the
    /// graph; `set_fx_params` updates a node's params in place (live, no re-apply),
    /// so every renderer whose chain passes through that node hears it immediately.
    fx_params: Arc<Mutex<HashMap<NodeId, Arc<FxParams>>>>,
    /// Routes whose source app wasn't running when the graph was applied. They used
    /// to be dropped silently and never looked at again, so an app started after the
    /// engine stayed mute until a manual Restart Engine (t31). Keeping them here and
    /// retrying makes the engine converge on the graph's intent instead of wiring it
    /// once: the graph says "route Firefox", so the route is owed until Firefox is up.
    pending: Arc<Mutex<Vec<ActiveRoute>>>,
}

impl RoutingEngine {
    pub fn new() -> Self {
        Self {
            graph: Arc::new(Mutex::new(Graph::new())),
            running: Arc::new(AtomicBool::new(false)),
            captures: Arc::new(Mutex::new(HashMap::new())),
            routes: Arc::new(Mutex::new(HashMap::new())),
            format: AudioFormat::default(),
            restart_lock: Mutex::new(()),
            last_applied: Mutex::new(None),
            fx_params: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Live-update an FX node's parameters (called from the UI, like set_route_volume).
    /// No-op if the node isn't in the currently-applied graph.
    pub fn set_fx_params(&self, node_id: &str, spec: &FxSpec) {
        // Live path — every renderer whose chain passes this node hears it at once.
        {
            if let Some(p) = lock_recover(&self.fx_params).get(node_id) {
                p.store(spec);
            }
        }
        // Durable path — the graph is what a route is rebuilt from. For an FX node that
        // no renderer has instantiated (its route is still waiting for its app) the live
        // write above is a no-op, and the knob turn would vanish the moment the route
        // wires. `set_route_volume`/`mute`/`pan` already write through; this is the same
        // guarantee for FX. Node absent from the applied graph → nothing to record. (t31)
        let _ = lock_recover(&self.graph).set_node_fx(&node_id.to_string(), *spec);
    }

    /// Build the FX processor chain for a route, sharing one `Arc<FxParams>` per FX
    /// node (created on first use this apply) so live updates reach every renderer.
    fn build_fx_chain(&self, ar: &ActiveRoute, sample_rate: f32) -> Vec<FxProcessor> {
        if ar.fx_chain.is_empty() {
            return Vec::new();
        }
        let mut map = lock_recover(&self.fx_params);
        ar.fx_chain
            .iter()
            .map(|inst| {
                let params = Arc::clone(
                    map.entry(inst.node_id.clone())
                        .or_insert_with(|| Arc::new(FxParams::new(&inst.spec))),
                );
                FxProcessor::new(params, sample_rate)
            })
            .collect()
    }

    /// Apply a new routing graph snapshot. Restarts active routes.
    pub fn apply_graph(&self, snapshot: RoutingGraph) -> Result<(), EngineError> {
        let _restart = lock_recover(&self.restart_lock);

        // Skip a redundant re-apply of an UNCHANGED graph while running. A restart
        // tears down + re-resolves every route, which (a) flickers the engine
        // on→off→on and storms if the UI re-applies the same graph, and (b) drops a
        // self-healing capture whose source app is momentarily gone → its recovery
        // never runs. Compare by serialized JSON (RoutingGraph isn't PartialEq). (t19)
        let key = serde_json::to_string(&snapshot).unwrap_or_default();
        if self.running.load(Ordering::SeqCst)
            && lock_recover(&self.last_applied).as_deref() == Some(key.as_str())
        {
            return Ok(());
        }

        // Validate + swap the graph (held briefly — released before the restart below).
        {
            let mut g = lock_recover(&self.graph);
            g.apply_snapshot(snapshot)
                .map_err(|e| EngineError::Session(e.to_string()))?;
        }
        *lock_recover(&self.last_applied) = Some(key);

        if self.running.load(Ordering::SeqCst) {
            self.stop_internal();
            // Give background capture/render threads time to release their WASAPI COM objects.
            // Without this pause, a rapid stop→start on the same device causes
            // AUDCLNT_E_DEVICE_IN_USE (0x8889000A) in the new session's Initialize().
            // Only restart_lock is held here — volume/mute commands stay responsive.
            std::thread::sleep(std::time::Duration::from_millis(80));
            self.start_internal()?;
        }
        Ok(())
    }

    /// Start routing according to the current graph.
    pub fn start(&self) -> Result<(), EngineError> {
        let _restart = lock_recover(&self.restart_lock);
        if self.running.load(Ordering::SeqCst) {
            return Err(EngineError::AlreadyRunning);
        }
        self.start_internal()
    }

    /// Stop all active routing.
    pub fn stop(&self) -> Result<(), EngineError> {
        let _restart = lock_recover(&self.restart_lock);
        if !self.running.load(Ordering::SeqCst) {
            return Err(EngineError::NotRunning);
        }
        self.stop_internal();
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Update mute on a live edge (any edge in a chain) without restarting.
    /// A route is effectively muted if ANY edge in its chain is muted.
    pub fn set_route_mute(&self, edge_id: &str, muted: bool) -> Result<(), EngineError> {
        let mut g = lock_recover(&self.graph);
        if g.set_mute(&edge_id.to_string(), muted).is_err() {
            return Ok(()); // edge not in the applied graph — picked up on next apply
        }
        let routes = lock_recover(&self.routes);
        for handles in routes.values() {
            for h in handles {
                if h.chain.iter().any(|c| c.as_str() == edge_id) {
                    let eff = h
                        .chain
                        .iter()
                        .any(|c| g.get_route(c).map(|r| r.muted).unwrap_or(false));
                    h.muted.store(eff, Ordering::Relaxed);
                }
            }
        }
        Ok(())
    }

    /// Update volume on a live edge (any edge in a chain) without restarting.
    /// The route's effective volume is the product of its chain edges' volumes,
    /// so an intermediate edge (a Mixer-input slider) takes effect, not only the
    /// final edge into the output.
    pub fn set_route_volume(&self, edge_id: &str, volume: f32) -> Result<(), EngineError> {
        let volume = clamp_volume(volume);
        let mut g = lock_recover(&self.graph);
        if g.set_volume(&edge_id.to_string(), volume).is_err() {
            return Ok(()); // edge not in the applied graph — picked up on next apply
        }
        let routes = lock_recover(&self.routes);
        for handles in routes.values() {
            for h in handles {
                if h.chain.iter().any(|c| c.as_str() == edge_id) {
                    let eff: f32 = h
                        .chain
                        .iter()
                        .map(|c| g.get_route(c).map(|r| r.volume).unwrap_or(1.0))
                        .product();
                    h.volume.store(volume_to_atomic(eff), Ordering::Relaxed);
                }
            }
        }
        Ok(())
    }

    /// Update stereo balance [-1.0 .. 1.0] on a live route without restarting.
    pub fn set_route_pan(&self, route_id: &str, pan: f32) -> Result<(), EngineError> {
        let pan = pan.clamp(-1.0, 1.0);
        {
            let mut g = lock_recover(&self.graph);
            if g.set_pan(&route_id.to_string(), pan).is_err() {
                return Ok(()); // edge not in the applied graph — picked up on next apply
            }
        }
        if let Some(handles) = lock_recover(&self.routes).get(route_id) {
            for handle in handles {
                handle.pan.store(volume_to_atomic(pan), Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Current VU levels [0,1] for the UI, keyed by:
    ///  - source: exe_name (apps) or WASAPI device id (device sources);
    ///  - output: destination device id — the COMBINED post-volume level of every
    ///    route rendering to that device (so two apps → one output show one merged
    ///    meter). Combined in linear RMS space, then re-scaled to dBFS [0,1] (t16).
    /// Gain reduction each FX node is applying right now, in dB (0 = idle), keyed
    /// by FX node id. This is how a Gate / Limiter / Compressor proves it is doing
    /// something: their faces otherwise carry no indicator at all, so "seems not to
    /// work" and "works but is invisible" look identical. (t18 wave 6)
    pub fn get_fx_levels(&self) -> HashMap<NodeId, crate::audio::dsp::FxLevel> {
        lock_recover(&self.fx_params)
            .iter()
            .map(|(id, p)| (id.clone(), p.level()))
            .collect()
    }

    /// Signal level arriving at each hub row, keyed by the EDGE that row owns —
    /// the UI addresses a Mixer input / Splitter output by its port, and each port
    /// is exactly one edge. (t18 wave 6b)
    ///
    /// The value is measured where the signal actually is: the OUTPUT of the last FX
    /// before the hub, or the source capture when nothing precedes it. Reading the
    /// source level instead would light the dot for a microphone sitting behind a
    /// closed gate — the node would contradict what you hear, which is the same
    /// class of bug as the gate badge.
    pub fn get_hub_levels(&self) -> HashMap<RouteId, f32> {
        let fx = lock_recover(&self.fx_params);
        let captures = lock_recover(&self.captures);
        let mut out: HashMap<RouteId, f32> = HashMap::new();
        for handles in lock_recover(&self.routes).values() {
            for h in handles {
                for tap in &h.hub_taps {
                    let level = match &tap.after_fx {
                        Some(node) => fx.get(node).map(|p| p.output_level()),
                        None => captures.get(&h.capture_key).map(|c| c.capture.current_level()),
                    };
                    if let Some(v) = level {
                        // Splitter fan-out visits the same edge from several routes
                        // carrying the same signal; keep the loudest so a momentary
                        // zero from one of them can't blank a live row.
                        out.entry(tap.edge.clone())
                            .and_modify(|cur| *cur = cur.max(v))
                            .or_insert(v);
                    }
                }
            }
        }
        out
    }

    pub fn get_levels(&self) -> HashMap<String, f32> {
        // Per-source capture levels.
        let mut levels: HashMap<String, f32> = lock_recover(&self.captures)
            .iter()
            .map(|(device_id, handle)| {
                let key = handle.exe_name.clone().unwrap_or_else(|| device_id.clone());
                (key, handle.capture.current_level())
            })
            .collect();

        // Combined per-destination output levels. Each route's level is dBFS-scaled
        // [0,1]; convert back to linear, sum squares per device, take RMS, re-scale.
        let mut sumsq: HashMap<String, f32> = HashMap::new();
        for handles in lock_recover(&self.routes).values() {
            for h in handles {
                if h.to_device_id.is_empty() {
                    continue;
                }
                if let Some(sink) = &h.sink {
                    let scaled = sink.current_level();
                    if scaled <= 0.0 {
                        continue;
                    }
                    let lin = 10f32.powf((scaled * 100.0 - 100.0) / 20.0);
                    *sumsq.entry(h.to_device_id.clone()).or_insert(0.0) += lin * lin;
                }
            }
        }
        for (dev, ss) in sumsq {
            let db = 20.0 * ss.sqrt().max(1e-7_f32).log10();
            levels.insert(dev, ((db + 100.0) / 100.0).clamp(0.0, 1.0));
        }
        levels
    }

    /// Link state per output device (LINK_* code) for the per-node UI status dot.
    /// A device may back several routes (several render threads); it's reported by
    /// the healthiest of them (min code: any online route ⇒ the device is online).
    pub fn get_link_states(&self) -> HashMap<String, u8> {
        let mut states: HashMap<String, u8> = HashMap::new();
        // Output devices (render sinks).
        for handles in lock_recover(&self.routes).values() {
            for h in handles {
                if h.to_device_id.is_empty() {
                    continue;
                }
                if let Some(sink) = &h.sink {
                    let code = sink.link_state();
                    states
                        .entry(h.to_device_id.clone())
                        .and_modify(|c| *c = (*c).min(code))
                        .or_insert(code);
                }
            }
        }
        // Input/loopback device sources (self-healing too, t19).
        for handle in lock_recover(&self.captures).values() {
            if let Some((dev, code)) = handle.capture.device_link() {
                states
                    .entry(dev)
                    .and_modify(|c| *c = (*c).min(code))
                    .or_insert(code);
            }
        }
        states
    }

    /// Number of routes waiting for their source app to appear. Diagnostics + tests.
    pub fn pending_route_count(&self) -> usize {
        lock_recover(&self.pending).len()
    }

    /// Retry routes deferred because their source app wasn't running (t31).
    ///
    /// Called periodically from the background thread in `bridge.rs`: the engine has
    /// no scheduler of its own, and spawning one just for this would duplicate a loop
    /// that already ticks several times a second. Returns how many routes got wired.
    ///
    /// Deliberately independent of the process detector — `find_audio_pid_for_exe`
    /// asks the session manager directly, so a missed `process-changed` event can't
    /// keep a route deferred.
    pub fn retry_pending_routes(&self) -> usize {
        // Cheap path first: this runs on a timer and the queue is empty in the
        // normal case, so it must not touch the restart lock to find that out.
        if !self.running.load(Ordering::SeqCst) || lock_recover(&self.pending).is_empty() {
            return 0;
        }
        // Wiring mutates the same state apply_graph/stop rebuild, so take the same
        // lock they do, in the same order (restart → captures → routes).
        let _restart = lock_recover(&self.restart_lock);
        if !self.running.load(Ordering::SeqCst) {
            return 0; // stopped while we waited for the lock
        }
        // Drain rather than iterate in place: wire_route re-queues whatever still
        // isn't ready, and the lock isn't reentrant.
        let mut queue: Vec<ActiveRoute> = std::mem::take(&mut *lock_recover(&self.pending));
        if queue.is_empty() {
            return 0;
        }

        // The queue holds a SNAPSHOT taken when the graph was applied. A route can
        // wait here for minutes, and meanwhile the user can mute it, move its faders
        // or turn an FX knob — all of which land on the graph and on live handles,
        // and a route that has no handles yet receives none of them. Wiring the stale
        // snapshot would bring the app up unmuted or at the old gain, which is worse
        // than staying silent. So re-read from the graph, with the same formulas
        // `resolve_device_routes` uses: volume is the product of the chain, mute is
        // any edge in it, pan is the final edge. (t31)
        {
            let g = lock_recover(&self.graph);
            for ar in &mut queue {
                refresh_route_from_graph(&g, ar);
            }
        }
        let attempted = queue.len();
        let mut failed = 0usize;

        let mut captures = lock_recover(&self.captures);
        let mut routes = lock_recover(&self.routes);
        for ar in queue {
            let route_id = ar.route_id.clone();
            let exe = ar.exe_name.clone().unwrap_or_default();
            // A hard wiring error (capture/render failed to start) is NOT the deferred
            // case — the app is up and something else is wrong. Report it and drop the
            // route rather than retrying a broken device twice a second forever.
            if let Err(e) = self.wire_route(ar, &mut captures, &mut routes) {
                failed += 1;
                warn!("deferred route {route_id} for '{exe}' failed to wire: {e}");
            }
        }
        drop(routes);
        drop(captures);

        let wired = attempted
            .saturating_sub(lock_recover(&self.pending).len())
            .saturating_sub(failed);
        if wired > 0 {
            info!("picked up {wired} deferred route(s) — source app(s) now running");
        }
        wired
    }

    fn start_internal(&self) -> Result<(), EngineError> {
        let g = lock_recover(&self.graph);
        let active_routes = g.resolve_device_routes();
        drop(g);

        info!("starting engine with {} active routes", active_routes.len());
        self.running.store(true, Ordering::SeqCst);

        // Rebuild FX param stores from scratch — nodes removed from the graph drop
        // out; wire_route repopulates from each route's fx_chain. (t18)
        lock_recover(&self.fx_params).clear();
        // Same for the deferred queue: it describes the PREVIOUS graph's unmet routes.
        // wire_route refills it from this resolve. (t31)
        lock_recover(&self.pending).clear();

        let mut captures = lock_recover(&self.captures);
        let mut routes = lock_recover(&self.routes);

        // Detect feedback conflicts before wiring:
        // if device D is both a loopback source AND a render target for a different source,
        // the rendered audio will be re-captured by the loopback → double signal + comb filter.
        {
            use std::collections::{HashMap, HashSet};
            // capture_device → set of to_devices for that source
            let mut cap_devices: HashMap<&str, HashSet<&str>> = HashMap::new();
            for ar in &active_routes {
                cap_devices
                    .entry(&ar.from_device_id)
                    .or_default()
                    .insert(&ar.to_device_id);
            }
            for ar in &active_routes {
                // If we render TO device X, and device X is ALSO a loopback capture source
                if cap_devices.contains_key(ar.to_device_id.as_str())
                    && ar.to_device_id != ar.from_device_id
                {
                    warn!(
                        "Feedback risk: route {} renders TO device '{}' which is also \
                         a loopback capture source. The rendered audio will be re-captured \
                         and appear doubled (with delay) in any output receiving that loopback. \
                         To avoid this, do not route mic-monitoring to the same device that \
                         app-sources use for loopback capture.",
                        ar.route_id, ar.to_device_id
                    );
                }
            }
        }

        for ar in active_routes {
            info!(
                "wiring route {}: from_device='{}' exe={:?} → to_device='{}' vol={:.2} muted={}",
                ar.route_id, ar.from_device_id, ar.exe_name, ar.to_device_id, ar.volume, ar.muted
            );
            self.wire_route(ar, &mut captures, &mut routes)?;
        }

        Ok(())
    }

    fn wire_route(
        &self,
        ar: ActiveRoute,
        captures: &mut HashMap<String, CaptureHandle>,
        routes: &mut HashMap<String, Vec<RouteHandles>>,
    ) -> Result<(), EngineError> {
        // Choose the capture backend + a key for splitter fanout reuse:
        //   - App source (exe): isolated WASAPI process loopback on the app's PID.
        //     Per-route mute/volume act on our captured copy only — no global app mute.
        //   - Nodus virtual source: kernel-driver ring buffer (fallback to loopback).
        //   - Device source: whole-device loopback / input capture.
        enum Backend {
            Process(u32),
            Virtual,
            Device,
        }
        let (capture_key, backend) = if let Some(ref exe) = ar.exe_name {
            // Prefer the pid that actually holds the audio session: a multi-process
            // app (Spotify, browsers) has several, and only one of them plays.
            // When none does yet — the app is up but hasn't opened its session, the
            // usual state right after a Windows boot — bind provisionally so the
            // route exists, and let the capture's silence watchdog move it onto the
            // real audio pid once the session appears. Saying which of the two
            // happened is what makes the boot race readable in the log. (t31)
            match find_audio_pid_for_exe(exe, true) {
                Ok(pid) => {
                    debug!("resolved {exe} → pid {pid} (process loopback)");
                    (format!("exe:{exe}"), Backend::Process(pid))
                }
                Err(_) => match find_audio_pid_for_exe(exe, false) {
                    Ok(pid) => {
                        debug!(
                            "{exe} has no audio session yet — binding provisionally to pid \
                             {pid}; will rebind when the session appears"
                        );
                        (format!("exe:{exe}"), Backend::Process(pid))
                    }
                    Err(e) => {
                        // App not running at all. Defer instead of dropping: the graph
                        // still asks for this route, and the app may start at any moment
                        // (autostart after a boot, or the user just opening it). `warn!`
                        // rather than `debug!` — silently doing nothing is exactly what
                        // made this undiagnosable from the log. (t31)
                        warn!(
                            "route for '{exe}' deferred — app not running ({e}); \
                             will retry until it appears"
                        );
                        lock_recover(&self.pending).push(ar.clone());
                        return Ok(());
                    }
                },
            }
        } else if ar.from_is_virtual {
            (ar.from_device_id.clone(), Backend::Virtual)
        } else {
            (ar.from_device_id.clone(), Backend::Device)
        };

        // Reuse an existing capture for the same source (splitter fanout).
        let (receiver, capture_format) = if let Some(handle) = captures.get_mut(&capture_key) {
            handle.sender_count += 1;
            let rx = handle
                .capture
                .start()
                .map_err(|e| EngineError::Session(e.to_string()))?;
            (rx, handle.format)
        } else {
            // Build the source and the format it will actually produce:
            //  - Process loopback / virtual ring → our normalized format.
            //  - Device loopback → the device's mix format (channels/rate vary).
            let (mut source, mut fmt) = match backend {
                Backend::Process(pid) => (
                    CaptureSource::ProcessLoopback(ProcessLoopbackCapture::new(
                        pid,
                        ar.exe_name.clone(),
                        self.format,
                    )),
                    self.format,
                ),
                Backend::Virtual => {
                    // Read the driver render ring for this source's device: `nodus:<N>`
                    // → dynamic virtual output N, else the static speaker (ring 0). (t8)
                    let ring_id = crate::virtual_audio::virtual_device::ring_id_from_device_id(
                        &ar.from_device_id,
                    );
                    (CaptureSource::Virtual(VirtualCapture::new(ring_id)), self.format)
                }
                Backend::Device => {
                    let f = get_device_capture_format(&ar.from_device_id).unwrap_or(self.format);
                    (
                        CaptureSource::Loopback(LoopbackCapture::new(ar.from_device_id.clone(), f)),
                        f,
                    )
                }
            };
            let rx = match source.start() {
                Ok(rx) => rx,
                // If the Nodus ring isn't available (driver not loaded), fall back to
                // WASAPI loopback — the virtual speaker is a real render endpoint.
                Err(e) if matches!(source, CaptureSource::Virtual(_)) => {
                    debug!("virtual ring unavailable ({e}); falling back to WASAPI loopback");
                    let f = get_device_capture_format(&ar.from_device_id).unwrap_or(self.format);
                    let mut lb =
                        CaptureSource::Loopback(LoopbackCapture::new(ar.from_device_id.clone(), f));
                    let rx = lb
                        .start()
                        .map_err(|e| EngineError::Session(e.to_string()))?;
                    source = lb;
                    fmt = f;
                    rx
                }
                Err(e) => return Err(EngineError::Session(e.to_string())),
            };
            captures.insert(
                capture_key.clone(),
                CaptureHandle {
                    capture: source,
                    sender_count: 1,
                    exe_name: ar.exe_name.clone(),
                    format: fmt,
                },
            );
            (rx, fmt)
        };

        // Feedback guard applies only to whole-device loopback sources: capturing a
        // device and rendering back to it loops. App process loopback captures only the
        // app (no device feedback); virtual sources read the driver ring.
        let is_device_source = ar.exe_name.is_none() && !ar.from_is_virtual;
        if is_device_source && !ar.from_device_id.is_empty() && ar.from_device_id == ar.to_device_id
        {
            debug!("same-device route on {}: skipping render to avoid feedback", ar.from_device_id);
            let volume = Arc::new(AtomicU32::new(volume_to_atomic(ar.volume)));
            let muted = Arc::new(AtomicBool::new(ar.muted));
            let pan = Arc::new(AtomicU32::new(volume_to_atomic(ar.pan)));
            routes
                .entry(ar.route_id)
                .or_default()
                .push(RouteHandles {
                    volume,
                    muted,
                    pan,
                    sink: None,
                    chain: ar.chain,
                    to_device_id: ar.to_device_id,
                    hub_taps: ar.hub_taps,
                    capture_key: capture_key.clone(),
                });
            return Ok(());
        }

        let volume = Arc::new(AtomicU32::new(volume_to_atomic(ar.volume)));
        let muted = Arc::new(AtomicBool::new(ar.muted));
        let pan = Arc::new(AtomicU32::new(volume_to_atomic(ar.pan)));
        // FX chain for this route (t18) — one shared param store per FX node. Applied
        // to the source buffer before volume/pan, inside whichever sink renders it.
        let fx = self.build_fx_chain(&ar, capture_format.sample_rate as f32);

        let sink = if ar.to_is_virtual_mic {
            // Destination is the Nodus virtual microphone: write into the kernel
            // driver's mic ring instead of a WASAPI endpoint (that endpoint is a
            // CAPTURE device — run_render would rightly refuse it). VirtualRender
            // converts source format → 48k/stereo/i16 itself. If the driver isn't
            // loaded, the writer thread warns once and exits — route stays silent
            // but alive (driverless fallback policy). The ring id comes from the
            // destination device id: `nodus:<N>` → dynamic mic N, else the static
            // mic 0 — so distinct virtual mics carry independent audio. (t8)
            let ring_id =
                crate::virtual_audio::virtual_device::ring_id_from_device_id(&ar.to_device_id);
            let vr = VirtualRender::new(capture_format, ring_id);
            vr.start(
                receiver,
                Arc::clone(&volume),
                Arc::clone(&muted),
                Arc::clone(&pan),
                fx,
            );
            RouteSink::VirtualMic(vr)
        } else {
            // Renderer takes the SOURCE format; AUTOCONVERTPCM remixes/resamples to the
            // output device. This fixes channel-count / sample-rate mismatch for device
            // loopback sources whose mix format differs from our default.
            let renderer = AudioRenderer::new(ar.to_device_id.clone(), capture_format);
            renderer.start(
                receiver,
                Arc::clone(&volume),
                Arc::clone(&muted),
                Arc::clone(&pan),
                fx,
            );
            RouteSink::Wasapi(renderer)
        };

        debug!(
            "wired route → {}{} (vol={:.2} muted={} pan={:.2})",
            ar.to_device_id,
            if ar.to_is_virtual_mic { " [virtual mic ring]" } else { "" },
            ar.volume, ar.muted, ar.pan
        );

        routes
            .entry(ar.route_id)
            .or_default()
            .push(RouteHandles {
                volume,
                muted,
                pan,
                sink: Some(sink),
                chain: ar.chain,
                to_device_id: ar.to_device_id,
                hub_taps: ar.hub_taps,
                capture_key,
            });
        Ok(())
    }

    fn stop_internal(&self) {
        self.running.store(false, Ordering::SeqCst);
        // Nothing is owed while stopped — a start re-resolves the graph from scratch. (t31)
        lock_recover(&self.pending).clear();

        let mut captures = lock_recover(&self.captures);
        let mut routes = lock_recover(&self.routes);

        for (_, handles) in routes.drain() {
            for handle in handles {
                if let Some(s) = handle.sink { s.stop(); }
            }
        }
        for (_, handle) in captures.drain() {
            handle.capture.stop();
        }

        info!("engine stopped");
    }
}

/// Re-read a deferred route's live-controllable fields from the current graph.
///
/// Everything the user can change WITHOUT a re-apply — mute, faders, FX knobs —
/// reaches live handles and the graph, but a route still waiting for its app has no
/// handles to reach. Wiring its original snapshot would bring the app up unmuted or
/// at a stale gain. Mirrors `resolve_device_routes`: volume is the product of the
/// chain, mute is any edge in it, pan is the final edge. (t31)
fn refresh_route_from_graph(g: &Graph, ar: &mut ActiveRoute) {
    ar.volume = ar
        .chain
        .iter()
        .map(|c| g.get_route(c).map(|r| r.volume).unwrap_or(1.0))
        .product();
    ar.muted = ar
        .chain
        .iter()
        .any(|c| g.get_route(c).map(|r| r.muted).unwrap_or(false));
    if let Some(r) = g.get_route(&ar.route_id) {
        ar.pan = r.pan;
    }
    // A node shared with an already-wired route keeps its live params — build_fx_chain
    // reuses the existing Arc — so this can never overwrite a live value; it only fills
    // in a node no renderer has instantiated yet, whose knob turns live in the graph.
    for inst in &mut ar.fx_chain {
        if let Some(spec) = g.get_node(&inst.node_id).and_then(|n| n.fx) {
            inst.spec = spec;
        }
    }
}

impl Default for RoutingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RoutingEngine {
    fn drop(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            self.stop_internal();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{
        graph::RoutingGraph,
        node::{Node, NodeType, Route},
    };

    fn make_graph(from_dev: &str, to_dev: &str) -> RoutingGraph {
        let src = Node::new(NodeType::Source, "Src", from_dev);
        let dst = Node::new(NodeType::Output, "Dst", to_dev);
        let route = Route::new(src.id.clone(), dst.id.clone());
        RoutingGraph {
            nodes: vec![src, dst],
            routes: vec![route],
        }
    }

    #[test]
    fn engine_starts_and_stops() {
        let engine = RoutingEngine::new();
        assert!(!engine.is_running());

        // On non-Windows or with no real devices, start may succeed (stubs) or fail.
        // We just verify the state transitions.
        let _ = engine.start();
        let _ = engine.stop();
        assert!(!engine.is_running());
    }

    #[test]
    fn double_start_returns_error() {
        let engine = RoutingEngine::new();
        let _ = engine.start();
        let res = engine.start();
        assert!(matches!(res, Err(EngineError::AlreadyRunning)));
        let _ = engine.stop();
    }

    #[test]
    fn stop_without_start_returns_error() {
        let engine = RoutingEngine::new();
        let res = engine.stop();
        assert!(matches!(res, Err(EngineError::NotRunning)));
    }

    #[test]
    fn apply_graph_updates_state() {
        let engine = RoutingEngine::new();
        let graph = make_graph("dev-a", "dev-b");
        // Should not panic
        let _ = engine.apply_graph(graph);
    }

    /// t31: a route whose source app isn't running must be REMEMBERED, not dropped.
    /// Dropping it is what left an app started after the engine silent until the user
    /// hit Restart Engine by hand.
    #[test]
    fn route_for_absent_app_is_deferred_not_dropped() {
        let engine = RoutingEngine::new();
        // Nothing owed while stopped, and the retry is a cheap no-op there — it ticks
        // off the VU thread whether or not the engine is up.
        assert_eq!(engine.pending_route_count(), 0);
        assert_eq!(engine.retry_pending_routes(), 0);

        let ar = ActiveRoute {
            route_id: "r1".to_string(),
            fx_chain: Vec::new(),
            chain: vec!["r1".to_string()],
            from_device_id: String::new(),
            exe_name: Some("nodus-no-such-process-t31.exe".to_string()),
            from_is_virtual: false,
            to_device_id: "dev-b".to_string(),
            to_is_virtual_mic: false,
            volume: 1.0,
            muted: false,
            pan: 0.0,
            hub_taps: Vec::new(),
        };
        let mut captures = HashMap::new();
        let mut routes = HashMap::new();
        engine
            .wire_route(ar, &mut captures, &mut routes)
            .expect("deferring is not a wiring error");

        assert_eq!(engine.pending_route_count(), 1, "route must be queued for retry");
        assert!(routes.is_empty(), "nothing to wire yet — the app isn't up");
        assert!(captures.is_empty());

        // Stopping clears what's owed: a later start re-resolves the graph from scratch.
        engine.stop_internal();
        assert_eq!(engine.pending_route_count(), 0);
    }

    /// t31: a route can sit deferred for minutes. Mute/faders/FX knobs changed while it
    /// waited reach the graph but not the route (it has no handles yet), so wiring the
    /// original snapshot would bring the app up unmuted or at a stale gain.
    #[test]
    fn deferred_route_is_wired_from_the_current_graph_not_its_snapshot() {
        let src = Node::new(NodeType::Source, "Src", "");
        let mix = Node::new(NodeType::Mixer, "Mix", "");
        let out = Node::new(NodeType::Output, "Out", "dev-b");
        let e1 = Route::new(src.id.clone(), mix.id.clone());
        let e2 = Route::new(mix.id.clone(), out.id.clone());
        let (e1_id, e2_id) = (e1.id.clone(), e2.id.clone());

        let mut g = Graph::new();
        g.apply_snapshot(RoutingGraph {
            nodes: vec![src, mix, out],
            routes: vec![e1, e2],
        })
        .expect("valid graph");

        // How the route looked when it was deferred: open, full gain, centred.
        let mut ar = ActiveRoute {
            route_id: e2_id.clone(),
            fx_chain: Vec::new(),
            chain: vec![e1_id.clone(), e2_id.clone()],
            from_device_id: String::new(),
            exe_name: Some("nodus-no-such-process-t31.exe".to_string()),
            from_is_virtual: false,
            to_device_id: "dev-b".to_string(),
            to_is_virtual_mic: false,
            volume: 1.0,
            muted: false,
            pan: 0.0,
            hub_taps: Vec::new(),
        };

        // What the user did while the app was down.
        g.set_volume(&e1_id, 0.5).expect("edge exists");
        g.set_volume(&e2_id, 0.5).expect("edge exists");
        g.set_mute(&e1_id, true).expect("edge exists");
        g.set_pan(&e2_id, -1.0).expect("edge exists");

        refresh_route_from_graph(&g, &mut ar);

        assert!(ar.muted, "a muted route must not come up playing");
        assert!((ar.volume - 0.25).abs() < 1e-6, "chain volumes multiply, like resolve does");
        assert!((ar.pan + 1.0).abs() < 1e-6, "pan comes from the final edge");
    }
}
