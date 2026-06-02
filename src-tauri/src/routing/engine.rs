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
        clamp_volume, find_device_for_exe, volume_to_atomic, AppSessionControl, AudioFrame,
        AudioRenderer, LoopbackCapture, SessionError,
    },
    virtual_capture::VirtualCapture,
    wasapi::AudioFormat,
};
use tokio::sync::broadcast;

/// A capture source feeding one device's broadcast channel. Either a WASAPI
/// loopback/input capture, or the Nodus kernel driver's shared ring buffer.
enum CaptureSource {
    Loopback(LoopbackCapture),
    Virtual(VirtualCapture),
}

impl CaptureSource {
    /// Start (or re-subscribe to) the capture, yielding a frame receiver.
    fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
        match self {
            CaptureSource::Loopback(c) => c.start(),
            CaptureSource::Virtual(c) => c.start(),
        }
    }

    /// Current RMS level for VU metering. VirtualCapture has no meter yet → 0.0.
    fn current_level(&self) -> f32 {
        match self {
            CaptureSource::Loopback(c) => c.current_level(),
            CaptureSource::Virtual(_) => 0.0,
        }
    }

    fn stop(&self) {
        match self {
            CaptureSource::Loopback(c) => c.stop(),
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
    /// None for same-device routes (no loopback renderer needed, session control only).
    renderer: Option<AudioRenderer>,
    /// Windows audio session control for app-capture sources (mute/volume on the app itself).
    app_session: Option<AppSessionControl>,
}

struct CaptureHandle {
    capture: CaptureSource,
    sender_count: usize,
    exe_name: Option<String>,
    /// Shared session control — cloned into each RouteHandle for splitter fanout.
    app_session: Option<AppSessionControl>,
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
                handle.muted.store(muted, Ordering::Relaxed);
                // For app-capture sources, also mute the Windows audio session so the app's
                // direct-to-device path is silenced (not just the Nodus loopback copy).
                if let Some(ref session) = handle.app_session {
                    session.set_mute(muted);
                }
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
                handle.volume.store(volume_to_atomic(volume), Ordering::Relaxed);
                // Also apply volume to the Windows audio session for app-capture sources.
                if let Some(ref session) = handle.app_session {
                    session.set_volume(volume);
                }
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
        // For app-capture sources, discover device ID and get Windows session control.
        let (capture_device_id, app_session) = if let Some(ref exe) = ar.exe_name {
            match find_device_for_exe(exe) {
                Ok((id, ctrl)) => {
                    debug!("resolved {exe} → device {id}");
                    (id, Some(ctrl))
                }
                Err(e) => {
                    debug!("skipping route for {exe}: {e}");
                    return Ok(()); // app not running or no audio session yet
                }
            }
        } else {
            (ar.from_device_id.clone(), None)
        };

        // Reuse existing capture if same source device (splitter fanout).
        let (receiver, route_session) = if let Some(handle) = captures.get_mut(&capture_device_id) {
            handle.sender_count += 1;
            let rx = handle
                .capture
                .start()
                .map_err(|e| EngineError::Session(e.to_string()))?;
            (rx, handle.app_session.clone())
        } else {
            // Pick the capture backend. A Nodus virtual source reads from the
            // kernel driver's shared ring buffer (VirtualCapture); everything else
            // uses WASAPI loopback/input capture. If the ring isn't available
            // (driver not loaded), fall back to loopback — the virtual speaker is a
            // real render endpoint, so loopback still captures whatever apps play.
            let mut source = if ar.from_is_virtual && ar.exe_name.is_none() {
                CaptureSource::Virtual(VirtualCapture::new())
            } else {
                CaptureSource::Loopback(LoopbackCapture::new(
                    capture_device_id.clone(),
                    self.format,
                ))
            };
            let rx = match source.start() {
                Ok(rx) => rx,
                Err(e) if matches!(source, CaptureSource::Virtual(_)) => {
                    debug!("virtual ring unavailable ({e}); falling back to WASAPI loopback");
                    let mut lb = CaptureSource::Loopback(LoopbackCapture::new(
                        capture_device_id.clone(),
                        self.format,
                    ));
                    let rx = lb
                        .start()
                        .map_err(|e| EngineError::Session(e.to_string()))?;
                    source = lb;
                    rx
                }
                Err(e) => return Err(EngineError::Session(e.to_string())),
            };
            captures.insert(
                capture_device_id.clone(),
                CaptureHandle {
                    capture: source,
                    sender_count: 1,
                    exe_name: ar.exe_name.clone(),
                    app_session: app_session.clone(),
                },
            );
            (rx, app_session)
        };

        // Apply initial mute/volume to the Windows audio session (for app-capture sources).
        if let Some(ref s) = route_session {
            s.set_mute(ar.muted);
            s.set_volume(ar.volume);
        }

        // Same-device route (e.g. Spotify → same headphones it already plays to):
        // loopback capture + re-render creates a feedback loop causing buzzing.
        // Use session control only — no capture/render needed.
        if !capture_device_id.is_empty() && capture_device_id == ar.to_device_id {
            debug!(
                "same-device route {}: session-control only (no loopback)",
                ar.exe_name.as_deref().unwrap_or(&capture_device_id)
            );
            let volume = Arc::new(AtomicU32::new(volume_to_atomic(ar.volume)));
            let muted = Arc::new(AtomicBool::new(ar.muted));
            routes
                .entry(ar.route_id)
                .or_default()
                .push(RouteHandles { volume, muted, renderer: None, app_session: route_session });
            return Ok(());
        }

        let volume = Arc::new(AtomicU32::new(volume_to_atomic(ar.volume)));
        let muted = Arc::new(AtomicBool::new(ar.muted));
        let renderer = AudioRenderer::new(ar.to_device_id.clone(), self.format);
        renderer.start(receiver, Arc::clone(&volume), Arc::clone(&muted));

        debug!(
            "wired route {} → {} (vol={:.2} muted={})",
            ar.from_device_id, ar.to_device_id, ar.volume, ar.muted
        );

        routes
            .entry(ar.route_id)
            .or_default()
            .push(RouteHandles { volume, muted, renderer: Some(renderer), app_session: route_session });
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
