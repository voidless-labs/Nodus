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

use super::graph::{ActiveRoute, Graph, RoutingGraph};
use crate::audio::{
    session::{
        clamp_volume, find_audio_pid_for_exe, get_device_capture_format, volume_to_atomic,
        AudioFrame, AudioRenderer, LoopbackCapture, ProcessLoopbackCapture, SessionError,
    },
    virtual_capture::VirtualCapture,
    wasapi::AudioFormat,
};
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

    /// Current RMS level for VU metering. VirtualCapture has no meter yet → 0.0.
    fn current_level(&self) -> f32 {
        match self {
            CaptureSource::Loopback(c) => c.current_level(),
            CaptureSource::ProcessLoopback(c) => c.current_level(),
            CaptureSource::Virtual(_) => 0.0,
        }
    }

    fn stop(&self) {
        match self {
            CaptureSource::Loopback(c) => c.stop(),
            CaptureSource::ProcessLoopback(c) => c.stop(),
            CaptureSource::Virtual(c) => c.stop(),
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

struct RouteHandles {
    volume: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    /// None for same-device routes (no renderer, capture is suppressed to avoid feedback).
    renderer: Option<AudioRenderer>,
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
}

impl RoutingEngine {
    pub fn new() -> Self {
        Self {
            graph: Arc::new(Mutex::new(Graph::new())),
            running: Arc::new(AtomicBool::new(false)),
            captures: Arc::new(Mutex::new(HashMap::new())),
            routes: Arc::new(Mutex::new(HashMap::new())),
            format: AudioFormat::default(),
        }
    }

    /// Apply a new routing graph snapshot. Restarts active routes.
    pub fn apply_graph(&self, snapshot: RoutingGraph) -> Result<(), EngineError> {
        let mut g = lock_recover(&self.graph);
        g.apply_snapshot(snapshot);

        if self.running.load(Ordering::SeqCst) {
            drop(g);
            self.stop_internal();
            // Give background capture/render threads time to release their WASAPI COM objects.
            // Without this pause, a rapid stop→start on the same device causes
            // AUDCLNT_E_DEVICE_IN_USE (0x8889000A) in the new session's Initialize().
            std::thread::sleep(std::time::Duration::from_millis(80));
            self.start_internal()?;
        }
        Ok(())
    }

    /// Start routing according to the current graph.
    pub fn start(&self) -> Result<(), EngineError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(EngineError::AlreadyRunning);
        }
        self.start_internal()
    }

    /// Stop all active routing.
    pub fn stop(&self) -> Result<(), EngineError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EngineError::NotRunning);
        }
        self.stop_internal();
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Update mute on a live route without restarting the engine.
    pub fn set_route_mute(&self, route_id: &str, muted: bool) -> Result<(), EngineError> {
        {
            let mut g = lock_recover(&self.graph);
            g.set_mute(&route_id.to_string(), muted)
                .map_err(|e| EngineError::Session(e.to_string()))?;
        }
        if let Some(handles) = lock_recover(&self.routes).get(route_id) {
            for handle in handles {
                // Per-route: mute only our captured copy, not the app/device globally.
                handle.muted.store(muted, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Update volume on a live route without restarting the engine.
    pub fn set_route_volume(&self, route_id: &str, volume: f32) -> Result<(), EngineError> {
        let volume = clamp_volume(volume);
        {
            let mut g = lock_recover(&self.graph);
            g.set_volume(&route_id.to_string(), volume)
                .map_err(|e| EngineError::Session(e.to_string()))?;
        }
        if let Some(handles) = lock_recover(&self.routes).get(route_id) {
            for handle in handles {
                // Per-route: scale only our captured copy, not the app/device globally.
                handle.volume.store(volume_to_atomic(volume), Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Current RMS levels keyed by source identifier (for VU meters in UI).
    /// App-capture sources are keyed by exe_name (e.g. "spotify.exe");
    /// device sources are keyed by WASAPI device ID.
    pub fn get_source_levels(&self) -> HashMap<String, f32> {
        lock_recover(&self.captures)
            .iter()
            .map(|(device_id, handle)| {
                let key = handle.exe_name.clone().unwrap_or_else(|| device_id.clone());
                (key, handle.capture.current_level())
            })
            .collect()
    }

    fn start_internal(&self) -> Result<(), EngineError> {
        let g = lock_recover(&self.graph);
        let active_routes = g.resolve_device_routes();
        drop(g);

        info!("starting engine with {} active routes", active_routes.len());
        self.running.store(true, Ordering::SeqCst);

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
            match find_audio_pid_for_exe(exe) {
                Ok(pid) => {
                    debug!("resolved {exe} → pid {pid} (process loopback)");
                    (format!("exe:{exe}"), Backend::Process(pid))
                }
                Err(e) => {
                    debug!("skipping route for {exe}: {e}");
                    return Ok(()); // app not running or no audio session yet
                }
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
                    CaptureSource::ProcessLoopback(ProcessLoopbackCapture::new(pid, self.format)),
                    self.format,
                ),
                Backend::Virtual => (CaptureSource::Virtual(VirtualCapture::new()), self.format),
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
            routes
                .entry(ar.route_id)
                .or_default()
                .push(RouteHandles { volume, muted, renderer: None });
            return Ok(());
        }

        let volume = Arc::new(AtomicU32::new(volume_to_atomic(ar.volume)));
        let muted = Arc::new(AtomicBool::new(ar.muted));
        // Renderer takes the SOURCE format; AUTOCONVERTPCM remixes/resamples to the
        // output device. This fixes channel-count / sample-rate mismatch for device
        // loopback sources whose mix format differs from our default.
        let renderer = AudioRenderer::new(ar.to_device_id.clone(), capture_format);
        renderer.start(receiver, Arc::clone(&volume), Arc::clone(&muted));

        debug!(
            "wired route → {} (vol={:.2} muted={})",
            ar.to_device_id, ar.volume, ar.muted
        );

        routes
            .entry(ar.route_id)
            .or_default()
            .push(RouteHandles { volume, muted, renderer: Some(renderer) });
        Ok(())
    }

    fn stop_internal(&self) {
        self.running.store(false, Ordering::SeqCst);

        let mut captures = lock_recover(&self.captures);
        let mut routes = lock_recover(&self.routes);

        for (_, handles) in routes.drain() {
            for handle in handles {
                if let Some(r) = handle.renderer { r.stop(); }
            }
        }
        for (_, handle) in captures.drain() {
            handle.capture.stop();
        }

        info!("engine stopped");
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
}
