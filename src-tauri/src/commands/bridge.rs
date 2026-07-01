/// All Tauri invoke commands — the only public interface between UI and Rust.
///
/// Rules:
/// - Every command returns Result<T, String> (Tauri requirement)
/// - State lives in tauri::State wrappers, registered in main.rs
/// - Events are emitted on the AppHandle

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, State};
use tracing::{error, info};

use crate::{
    audio::{
        devices::{enumerate_audio_devices, AudioDevice},
        virtual_device::{get_virtual_setup, query_virtual_status, VirtualSetupStatus},
        wasapi::ComGuard,
        device_control::{open_control, DeviceKind, VirtualDeviceInfo},
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
/// The shared workspace document (scenes) — single source of truth for both UIs
/// (t17 phase B). Same Arc the daemon's /rpc dispatcher uses.
pub struct SceneState(pub Arc<crate::server::scene_store::SceneStore>);
/// Shared application settings (t14) — mirrored + persisted like the scene.
pub struct SettingsState(pub Arc<crate::server::settings_store::SettingsStore>);

// ── Device commands ────────────────────────────────────────────────────────

/// Enumerate audio devices and upgrade render-side virtual endpoints to Virtual.
/// Shared by the Tauri command and the daemon's /rpc dispatcher (t17) so both
/// return identical device lists. Blocking (COM + WASAPI) — call off the async
/// runtime (spawn_blocking) when invoked from async contexts.
///
/// Only render endpoints (Output) become Virtual — capture endpoints (Input) stay
/// Input. CABLE Input is a render endpoint → Virtual (shown as Output target in
/// UI); CABLE Output is a capture endpoint → stays Input (not a render target).
pub fn list_devices_full() -> Result<Vec<AudioDevice>, String> {
    let _com = ComGuard::init().map_err(|e| e.to_string())?;
    let mut devices = enumerate_audio_devices().map_err(|e| e.to_string())?;

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

/// Return all audio devices (input + output + detected virtual).
/// VB-Audio CABLE devices are returned with Nodus-branded names.
#[tauri::command]
pub async fn get_audio_devices(
    _engine: State<'_, EngineState>,
) -> Result<Vec<AudioDevice>, String> {
    list_devices_full()
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

/// Whether the routing engine is currently running. Clients read this on mount /
/// reconnect to initialise the Engine button; live changes arrive via the
/// `engine-state` event (t17).
#[tauri::command]
pub fn is_engine_running(engine: State<'_, EngineState>) -> bool {
    engine.0.is_running()
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

// ── Virtual devices (t5 step 3, S3.5) ────────────────────────────────────────
// Dynamic Nodus virtual devices via the kernel control channel (\\.\NodusControl).
// All open a short-lived handle per call and run on a blocking thread (DeviceIoControl
// is a blocking syscall). No driver / older build → a clear error string.

/// List the driver's device table (2 static + up to 8 dynamic).
#[tauri::command]
pub async fn list_virtual_devices() -> Result<Vec<VirtualDeviceInfo>, String> {
    tokio::task::spawn_blocking(|| {
        let ctl = open_control().map_err(|e| e.to_string())?;
        ctl.list_devices().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Create a dynamic virtual device (render/capture) with a friendly name; returns it.
#[tauri::command]
pub async fn create_virtual_device(kind: String, name: String) -> Result<VirtualDeviceInfo, String> {
    let k = match kind.as_str() {
        "render" => DeviceKind::Render,
        "capture" => DeviceKind::Capture,
        other => return Err(format!("unknown kind '{other}' (use render|capture)")),
    };
    tokio::task::spawn_blocking(move || {
        let ctl = open_control().map_err(|e| e.to_string())?;
        let id = ctl.create_device(k, None, &name).map_err(|e| e.to_string())?;
        Ok(VirtualDeviceInfo { id, kind: k, name, is_static: false, ring_active: false })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Destroy a dynamic virtual device by its driver id (1..8; id 0 is refused).
#[tauri::command]
pub async fn remove_virtual_device(id: u32) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let ctl = open_control().map_err(|e| e.to_string())?;
        ctl.destroy_device(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Scene sync (t17 phase B) ─────────────────────────────────────────────────

/// Return the current workspace document + revision (single source of truth).
#[tauri::command]
pub fn get_scene(
    scene: State<'_, SceneState>,
) -> crate::server::scene_store::SceneSnapshot {
    scene.0.snapshot()
}

/// Replace the workspace document; persists + broadcasts `scene:snapshot` to the
/// other UIs. `origin` is the caller's client id so it can ignore its own echo.
/// Returns the new revision.
#[tauri::command]
pub fn push_scene(
    doc: serde_json::Value,
    origin: Option<String>,
    scene: State<'_, SceneState>,
) -> u64 {
    scene.0.push(doc, origin)
}

// ── Settings (t14) ───────────────────────────────────────────────────────────

/// Return the current application settings.
#[tauri::command]
pub fn get_settings(
    settings: State<'_, SettingsState>,
) -> crate::server::settings_store::Settings {
    settings.0.get()
}

/// Replace application settings; persists + broadcasts `settings:changed` to the
/// other UIs and applies side effects (e.g. Windows autostart). Returns normalised.
#[tauri::command]
pub fn set_settings(
    next: crate::server::settings_store::Settings,
    settings: State<'_, SettingsState>,
) -> crate::server::settings_store::Settings {
    settings.0.set(next)
}

// ── Background tasks ───────────────────────────────────────────────────────

/// Spawn background tasks that publish events onto the daemon `bus` (t17).
///
/// Single producer → one bus → two consumers: a forwarder in main.rs mirrors the
/// bus to `emit_all` (desktop webview) while each WS connection mirrors it to its
/// socket (Web-UI / Claude-preview). This same path also carries `scene:snapshot`
/// (Phase B), so a scene change from a web client reaches the desktop too.
pub fn setup_background_tasks(
    handle: AppHandle,
    bus: crate::server::EventBus,
    settings: Arc<crate::server::settings_store::SettingsStore>,
) {
    // Process detector — publishes "process-changed" when the list changes. Interval
    // comes from settings (applied at launch). The ProcessDetector's background thread
    // holds Arc refs, so dropping the local detector here is fine.
    {
        let bus_proc = bus.clone();
        let detector = ProcessDetector::new();
        detector.start(settings.get().scan_interval(), move |procs| {
            match serde_json::to_value(&procs) {
                Ok(payload) => {
                    let _ = bus_proc.send(crate::server::ServerEvent {
                        event: "process-changed".into(),
                        payload,
                    });
                }
                Err(e) => error!("failed to serialize process list: {e}"),
            }
        });
        // detector drops here; background thread is kept alive by its own Arc refs
    }

    // Publish the initial device list on a background thread.
    let bus_dev = bus.clone();
    std::thread::spawn(move || {
        // Use the shared helper so the startup event carries the same Virtual
        // upgrades as get_audio_devices / the /rpc dispatcher.
        match list_devices_full() {
            Ok(devices) => {
                if let Ok(payload) = serde_json::to_value(&devices) {
                    let _ = bus_dev.send(crate::server::ServerEvent {
                        event: "audio-devices-changed".into(),
                        payload,
                    });
                }
            }
            Err(e) => error!("failed to enumerate devices on startup: {e}"),
        }
    });

    // VU meter — publishes "volume-levels" at ~15fps when the engine is running, and
    // only when the levels actually changed: every event triggers a WebView repaint,
    // which is expensive on weak GPUs (Pentium N4200 field test: WebView2 GPU
    // process at ~27% CPU). Payload: {device_id: level_0_to_1}.
    let handle_levels = handle.clone();
    let bus_levels = bus.clone();
    let settings_levels = settings.clone();
    std::thread::spawn(move || {
        let mut prev: std::collections::HashMap<String, f32> = Default::default();
        let mut was_running = false;
        let publish = |payload: serde_json::Value, bus: &crate::server::EventBus| {
            let _ = bus.send(crate::server::ServerEvent {
                event: "volume-levels".into(),
                payload,
            });
        };
        loop {
            // VU refresh rate is configurable live (t14): re-read each iteration.
            let cfg = settings_levels.get();
            std::thread::sleep(cfg.vu_interval());
            let engine = handle_levels.state::<EngineState>();
            // Engine is lock-free to query now; get_levels takes only the
            // short-lived internal captures/routes locks.
            let running = engine.0.is_running();
            // Broadcast engine on/off transitions so EVERY client's Engine button
            // stays in sync — the engine is a single shared instance, so its state
            // is the source of truth, not any one UI's local flag (t17).
            if running != was_running {
                was_running = running;
                let _ = bus_levels.send(crate::server::ServerEvent {
                    event: "engine-state".into(),
                    payload: serde_json::json!(running),
                });
            }
            // VU disabled (t14) or engine stopped → make sure meters are zeroed once,
            // then idle. Engine-state above still flows so the button stays in sync.
            if !running || !cfg.vu_enabled {
                if !prev.is_empty() {
                    prev.clear();
                    if let Ok(payload) = serde_json::to_value(&prev) {
                        publish(payload, &bus_levels);
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
                    publish(payload, &bus_levels);
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
