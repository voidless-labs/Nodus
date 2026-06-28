/// All Tauri invoke commands — the only public interface between UI and Rust.
///
/// Rules:
/// - Every command returns Result<T, String> (Tauri requirement)
/// - State lives in tauri::State wrappers, registered in main.rs
/// - Events are emitted on the AppHandle

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tauri::{AppHandle, Manager, State};
use tracing::{error, info};

use crate::{
    audio::{
        devices::{enumerate_audio_devices, AudioDevice},
        virtual_device::{get_virtual_setup, query_virtual_status, VirtualSetupStatus},
        wasapi::ComGuard,
    },
    detection::process::{detect_audio_processes, AudioProcess, ProcessDetector},
    routing::{engine::RoutingEngine, graph::RoutingGraph, node::RouteId},
};

// ── Shared state ──────────────────────────────────────────────────────────

// RoutingEngine is internally synchronized (Arc<Mutex<…>> fields + atomics), so it
// needs no outer Mutex. Sharing it via Arc lets commands run concurrently and lets
// slow operations move onto a blocking thread without holding a global engine lock.
pub struct EngineState(pub Arc<RoutingEngine>);
pub struct DetectorState(pub Mutex<ProcessDetector>);

// ── Device commands ────────────────────────────────────────────────────────

/// Return all audio devices (input + output + detected virtual).
/// VB-Audio CABLE devices are returned with Nodus-branded names.
#[tauri::command]
pub async fn get_audio_devices(
    _engine: State<'_, EngineState>,
) -> Result<Vec<AudioDevice>, String> {
    let _com = ComGuard::init().map_err(|e| e.to_string())?;
    let mut devices = enumerate_audio_devices().map_err(|e| e.to_string())?;

    // Upgrade virtual devices to Virtual type.
    // Only render endpoints (Output) become Virtual — capture endpoints (Input) stay as Input.
    // CABLE Input is a render endpoint → Virtual (shown as Output target in UI).
    // CABLE Output is a capture endpoint → stays Input (not a render target).
    let virtual_status = query_virtual_status(&devices);
    for vd in &virtual_status.devices {
        if let Some(d) = devices.iter_mut().find(|d| d.id == vd.id) {
            if d.device_type == crate::audio::devices::DeviceType::Output {
                d.device_type = crate::audio::devices::DeviceType::Virtual;
            }
        }
    }

    Ok(devices)
}

/// Return the current virtual device setup status (NotFound / VbAudio / NodusDriver).
/// UI uses this on startup to decide whether to show the onboarding dialog.
#[tauri::command]
pub async fn get_virtual_setup_status() -> Result<VirtualSetupStatus, String> {
    let _com = ComGuard::init().map_err(|e| e.to_string())?;
    let devices = enumerate_audio_devices().map_err(|e| e.to_string())?;
    Ok(get_virtual_setup(&devices))
}

/// Download and install VB-Audio VBCABLE (Windows only, shows UAC prompt).
/// Poll `get_virtual_setup_status` afterward to confirm success.
#[tauri::command]
pub async fn install_vbcable() -> Result<(), String> {
    crate::audio::virtual_device::setup::install_vbcable().await
}

/// Return whether Windows test signing mode is currently enabled.
/// Used by onboarding UI to show Test Mode option state.
#[tauri::command]
pub async fn is_test_signing_enabled() -> Result<bool, String> {
    Ok(crate::audio::virtual_device::setup::is_test_signing_enabled())
}

// ── Process commands ───────────────────────────────────────────────────────

/// Return currently running audio processes.
#[tauri::command]
pub async fn get_running_audio_processes() -> Result<Vec<AudioProcess>, String> {
    detect_audio_processes().map_err(|e| e.to_string())
}

// ── Routing commands ───────────────────────────────────────────────────────

/// Replace the entire routing graph and restart routing if engine is running.
/// Runs on a blocking thread: the restart settles WASAPI over ~80ms and must not
/// block the async runtime. Per-route volume/mute stay responsive meanwhile.
#[tauri::command]
pub async fn apply_routing_graph(
    graph: RoutingGraph,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    let engine = Arc::clone(&engine.0);
    tokio::task::spawn_blocking(move || engine.apply_graph(graph))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Mute or unmute a specific route (edge) without restarting. Fast, lock-free hot path.
#[tauri::command]
pub async fn set_route_mute(
    route_id: RouteId,
    muted: bool,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine.0.set_route_mute(&route_id, muted).map_err(|e| e.to_string())
}

/// Set volume [0.0 .. 1.0] on a specific route without restarting. Fast, lock-free hot path.
#[tauri::command]
pub async fn set_route_volume(
    route_id: RouteId,
    volume: f32,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine.0.set_route_volume(&route_id, volume).map_err(|e| e.to_string())
}

/// Set stereo balance [-1.0 .. 1.0] on a specific route without restarting.
#[tauri::command]
pub async fn set_route_pan(
    route_id: RouteId,
    pan: f32,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine.0.set_route_pan(&route_id, pan).map_err(|e| e.to_string())
}

/// Start the routing engine (WASAPI setup → blocking thread).
#[tauri::command]
pub async fn start_engine(engine: State<'_, EngineState>) -> Result<(), String> {
    let engine = Arc::clone(&engine.0);
    tokio::task::spawn_blocking(move || engine.start())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Stop the routing engine (WASAPI teardown → blocking thread).
#[tauri::command]
pub async fn stop_engine(engine: State<'_, EngineState>) -> Result<(), String> {
    let engine = Arc::clone(&engine.0);
    tokio::task::spawn_blocking(move || engine.stop())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// ── Background tasks ───────────────────────────────────────────────────────

/// Spawn background tasks that emit events to the UI.
/// State must already be managed (done in main.rs Builder::manage).
pub fn setup_background_tasks(handle: AppHandle) {
    // Process detector — emits "process-changed" every 2 seconds when list changes.
    // The ProcessDetector's background thread holds Arc refs, so dropping the
    // local detector here is fine — the thread runs until process exit.
    {
        let handle_proc = handle.clone();
        let detector = ProcessDetector::new();
        detector.start(Duration::from_secs(2), move |procs| {
            match serde_json::to_value(&procs) {
                Ok(payload) => {
                    if let Err(e) = handle_proc.emit_all("process-changed", payload) {
                        error!("failed to emit process-changed: {e}");
                    }
                }
                Err(e) => error!("failed to serialize process list: {e}"),
            }
        });
        // detector drops here; background thread is kept alive by its own Arc refs
    }

    // Emit initial device list on a background thread
    let handle_dev = handle.clone();
    std::thread::spawn(move || {
        if let Ok(_com) = ComGuard::init() {
            match enumerate_audio_devices() {
                Ok(devices) => {
                    if let Ok(payload) = serde_json::to_value(&devices) {
                        let _ = handle_dev.emit_all("audio-devices-changed", payload);
                    }
                }
                Err(e) => error!("failed to enumerate devices on startup: {e}"),
            }
        }
    });

    // VU meter — emits "volume-levels" at ~15fps when engine is running, and only
    // when the levels actually changed: every event triggers a WebView repaint,
    // which is expensive on weak GPUs (Pentium N4200 field test: WebView2 GPU
    // process at ~27% CPU). Payload: {device_id: level_0_to_1}.
    let handle_levels = handle.clone();
    std::thread::spawn(move || {
        let mut prev: std::collections::HashMap<String, f32> = Default::default();
        loop {
            std::thread::sleep(Duration::from_millis(66));
            let engine = handle_levels.state::<EngineState>();
            // Engine is lock-free to query now; get_levels takes only the
            // short-lived internal captures/routes locks.
            if !engine.0.is_running() {
                if !prev.is_empty() {
                    prev.clear(); // engine stopped — let the UI zero the meters
                    if let Ok(payload) = serde_json::to_value(&prev) {
                        let _ = handle_levels.emit_all("volume-levels", payload);
                    }
                }
                continue;
            }
            let levels = engine.0.get_levels();
            let changed = levels.len() != prev.len()
                || levels
                    .iter()
                    .any(|(k, v)| (prev.get(k).copied().unwrap_or(-1.0) - v).abs() > 0.01);
            if changed {
                if let Ok(payload) = serde_json::to_value(&levels) {
                    let _ = handle_levels.emit_all("volume-levels", payload);
                }
                prev = levels;
            }
        }
    });

    info!("background tasks started");
}

// ── Serialization tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::{
        audio::devices::{AudioDevice, DeviceType},
        detection::process::{AudioProcess, SourceType},
        routing::{
            graph::RoutingGraph,
            node::{Node, NodeType, Route},
        },
    };

    #[test]
    fn audio_device_serializes() {
        let d = AudioDevice {
            id: "test-id".into(),
            name: "Test Device".into(),
            device_type: DeviceType::Output,
            is_default: true,
            original_name: None,
            is_virtual: false,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: AudioDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, d.id);
        assert_eq!(back.device_type, DeviceType::Output);
    }

    #[test]
    fn audio_process_serializes() {
        let p = AudioProcess {
            exe_name: "arma3_x64.exe".into(),
            pid: 1234,
            display_name: "Arma 3".into(),
            source_type: SourceType::Game,
            icon: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("arma3_x64.exe"));
        assert!(json.contains("\"game\""));
    }

    #[test]
    fn routing_graph_round_trips() {
        let src = Node::new(NodeType::Source, "Arma 3", "dev-arma");
        let dst = Node::new(NodeType::Output, "Headphones", "dev-hp");
        let route = Route::new(src.id.clone(), dst.id.clone());
        let graph = RoutingGraph {
            nodes: vec![src, dst],
            routes: vec![route],
        };

        let json = serde_json::to_string(&graph).unwrap();
        let back: RoutingGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.routes.len(), 1);
    }
}
