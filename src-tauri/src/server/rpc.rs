//! `/rpc` command dispatcher (t17 phase A).
//!
//! Maps a `{cmd, args}` request onto the SAME engine operations the Tauri
//! commands in `commands/bridge.rs` use — no duplicated business logic, just a
//! second transport. Arg keys mirror the JS bridge: Tauri 1.x converts camelCase
//! invoke args to snake_case, so the bridge sends e.g. `routeId`; here we accept
//! both `routeId` and `route_id` for parity.

use serde_json::Value;

use crate::{
    audio::{
        devices::enumerate_audio_devices, virtual_device::get_virtual_setup, wasapi::ComGuard,
    },
    commands::bridge::list_devices_full,
    detection::process::detect_audio_processes,
    routing::graph::RoutingGraph,
    server::{RpcRequest, ServerState},
};

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Accept `routeId` or `route_id`.
fn route_id(args: &Value) -> Option<String> {
    arg_str(args, "routeId").or_else(|| arg_str(args, "route_id"))
}

fn arg_f32(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(|v| v.as_f64()).map(|f| f as f32)
}

fn arg_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

pub async fn dispatch(state: &ServerState, req: RpcRequest) -> Result<Value, String> {
    let args = &req.args;
    match req.cmd.as_str() {
        // ── Devices / processes (blocking COM/WASAPI → spawn_blocking) ──────
        "get_audio_devices" => {
            let devices = tokio::task::spawn_blocking(list_devices_full)
                .await
                .map_err(|e| e.to_string())??;
            to_value(devices)
        }
        "get_running_audio_processes" => {
            let procs = tokio::task::spawn_blocking(|| {
                detect_audio_processes().map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
            to_value(procs)
        }
        "get_virtual_setup_status" => {
            let status = tokio::task::spawn_blocking(|| -> Result<_, String> {
                let _com = ComGuard::init().map_err(|e| e.to_string())?;
                let devices = enumerate_audio_devices().map_err(|e| e.to_string())?;
                Ok(get_virtual_setup(&devices))
            })
            .await
            .map_err(|e| e.to_string())??;
            to_value(status)
        }
        "is_test_signing_enabled" => {
            to_value(crate::audio::virtual_device::setup::is_test_signing_enabled())
        }

        // ── Scene sync (phase B) ───────────────────────────────────────────
        "get_scene" => to_value(state.scene.snapshot()),
        "push_scene" => {
            let doc = args.get("doc").cloned().ok_or("missing doc")?;
            let origin = arg_str(args, "origin");
            let rev = state.scene.push(doc, origin);
            Ok(serde_json::json!({ "rev": rev }))
        }

        // ── Routing graph + engine lifecycle (blocking → spawn_blocking) ────
        "apply_routing_graph" => {
            // bridge sends { graph: {...} }; tolerate the bare graph too.
            let raw = args.get("graph").cloned().unwrap_or_else(|| args.clone());
            let graph: RoutingGraph = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            let engine = state.engine.clone();
            tokio::task::spawn_blocking(move || engine.apply_graph(graph))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "start_engine" => {
            let engine = state.engine.clone();
            tokio::task::spawn_blocking(move || engine.start())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "stop_engine" => {
            let engine = state.engine.clone();
            tokio::task::spawn_blocking(move || engine.stop())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "is_engine_running" => to_value(state.engine.is_running()),

        // ── Virtual devices (t5 step 3, S3.5) ──────────────────────────────
        "list_virtual_devices" => {
            let list = tokio::task::spawn_blocking(|| {
                let ctl = crate::audio::device_control::open_control().map_err(|e| e.to_string())?;
                ctl.list_devices().map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
            to_value(list)
        }
        "create_virtual_device" => {
            let kind = arg_str(args, "kind").ok_or("missing kind")?;
            let name = arg_str(args, "name").ok_or("missing name")?;
            let k = match kind.as_str() {
                "render" => crate::audio::device_control::DeviceKind::Render,
                "capture" => crate::audio::device_control::DeviceKind::Capture,
                other => return Err(format!("unknown kind '{other}'")),
            };
            let info = tokio::task::spawn_blocking(move || -> Result<_, String> {
                let ctl = crate::audio::device_control::open_control().map_err(|e| e.to_string())?;
                let id = ctl.create_device(k, None, &name).map_err(|e| e.to_string())?;
                Ok(crate::audio::device_control::VirtualDeviceInfo {
                    id, kind: k, name, is_static: false, ring_active: false,
                })
            })
            .await
            .map_err(|e| e.to_string())??;
            to_value(info)
        }
        "remove_virtual_device" => {
            let id = args.get("id").and_then(|v| v.as_u64()).ok_or("missing id")? as u32;
            tokio::task::spawn_blocking(move || {
                let ctl = crate::audio::device_control::open_control().map_err(|e| e.to_string())?;
                ctl.destroy_device(id).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
            Ok(Value::Null)
        }

        // ── Settings (t14) ─────────────────────────────────────────────────
        "get_settings" => to_value(state.settings.get()),
        "set_settings" => {
            // bridge sends { next: {...} } (Tauri command convention); tolerate a bare object too.
            let raw = args.get("next").cloned().unwrap_or_else(|| args.clone());
            let next: crate::server::settings_store::Settings =
                serde_json::from_value(raw).map_err(|e| e.to_string())?;
            to_value(state.settings.set(next))
        }

        // ── Per-route hot path (lock-free, runs inline) ────────────────────
        "set_route_mute" => {
            let id = route_id(args).ok_or("missing routeId")?;
            let muted = arg_bool(args, "muted").ok_or("missing muted")?;
            state.engine.set_route_mute(&id, muted).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "set_route_volume" => {
            let id = route_id(args).ok_or("missing routeId")?;
            let volume = arg_f32(args, "volume").ok_or("missing volume")?;
            state.engine.set_route_volume(&id, volume).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "set_route_pan" => {
            let id = route_id(args).ok_or("missing routeId")?;
            let pan = arg_f32(args, "pan").ok_or("missing pan")?;
            state.engine.set_route_pan(&id, pan).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }

        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{EventBus, ServerState};
    use std::sync::Arc;

    fn test_state() -> ServerState {
        let (bus, _rx): (EventBus, _) = tokio::sync::broadcast::channel(8);
        ServerState {
            engine: Arc::new(crate::routing::engine::RoutingEngine::new()),
            scene: Arc::new(crate::server::scene_store::SceneStore::new(None, bus.clone())),
            settings: Arc::new(crate::server::settings_store::SettingsStore::new(None, bus.clone())),
            bus,
            token: String::new(),
        }
    }

    #[tokio::test]
    async fn unknown_command_errors() {
        let s = test_state();
        let r = dispatch(&s, RpcRequest { cmd: "nope".into(), args: Value::Null }).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn set_route_volume_accepts_camel_and_snake() {
        let s = test_state();
        // Engine not running / edge absent → tolerant Ok (matches Tauri command).
        let camel = dispatch(
            &s,
            RpcRequest {
                cmd: "set_route_volume".into(),
                args: serde_json::json!({ "routeId": "e1", "volume": 0.5 }),
            },
        )
        .await;
        assert!(camel.is_ok(), "camelCase routeId should dispatch: {camel:?}");
        let snake = dispatch(
            &s,
            RpcRequest {
                cmd: "set_route_volume".into(),
                args: serde_json::json!({ "route_id": "e1", "volume": 0.5 }),
            },
        )
        .await;
        assert!(snake.is_ok(), "snake_case route_id should dispatch: {snake:?}");
    }

    #[tokio::test]
    async fn set_route_volume_missing_id_errors() {
        let s = test_state();
        let r = dispatch(
            &s,
            RpcRequest {
                cmd: "set_route_volume".into(),
                args: serde_json::json!({ "volume": 0.5 }),
            },
        )
        .await;
        assert!(r.is_err());
    }
}
