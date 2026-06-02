/// All Tauri invoke commands — the only public interface between UI and Rust.
///
/// Rules:
/// - Every command returns Result<T, String> (Tauri requirement)
/// - State lives in tauri::State wrappers, registered in main.rs
/// - Events are emitted on the AppHandle

use std::{sync::Mutex, time::Duration};

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

pub struct EngineState(pub Mutex<RoutingEngine>);
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
#[tauri::command]
pub async fn apply_routing_graph(
    graph: RoutingGraph,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .apply_graph(graph)
        .map_err(|e| e.to_string())
}

/// Mute or unmute a specific route (edge) without restarting.
#[tauri::command]
pub async fn set_route_mute(
    route_id: RouteId,
    muted: bool,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_route_mute(&route_id, muted)
        .map_err(|e| e.to_string())
}

/// Set volume [0.0 .. 1.0] on a specific route without restarting.
#[tauri::command]
pub async fn set_route_volume(
    route_id: RouteId,
    volume: f32,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    engine
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_route_volume(&route_id, volume)
        .map_err(|e| e.to_string())
}

/// Start the routing engine.
#[tauri::command]
pub async fn start_engine(engine: State<'_, EngineState>) -> Result<(), String> {
    engine
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .start()
        .map_err(|e| e.to_string())
}

/// Stop the routing engine.
#[tauri::command]
pub async fn stop_engine(engine: State<'_, EngineState>) -> Result<(), String> {
    engine
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .stop()
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

    // VU meter — emits "volume-levels" at ~30fps when engine is running.
    // Payload: {device_id: level_0_to_1} — UI maps device_id to node._deviceId.
    let handle_levels = handle.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let engine = handle_levels.state::<EngineState>();
            // try_lock: never block this 30fps poll on the engine mutex. A graph
            // apply (which holds the lock across a restart + sleep) or a real-time
            // volume command would otherwise stall the VU meter. Skip this frame
            // if the lock is contended — the next tick (33ms later) will catch up.
            let levels = match engine.0.try_lock() {
                Ok(guard) if guard.is_running() => guard.get_source_levels(),
                _ => continue,
            };
            if !levels.is_empty() {
                if let Ok(payload) = serde_json::to_value(&levels) {
                    let _ = handle_levels.emit_all("volume-levels", payload);
                }
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
