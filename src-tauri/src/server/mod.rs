//! t17 — embedded Nodus daemon: an HTTP (`/rpc`) + WebSocket (`/ws`) server that
//! exposes the live engine to non-Tauri clients (the Web-UI and Claude-preview).
//!
//! Phase A: transport over the EXISTING command/event contract — no scene-state
//! migration. The browser at :5173 points `bridge.ts` here and drives the real
//! engine instead of sample data. Commands go through [`rpc::dispatch`] (the same
//! engine methods the Tauri commands call); the event `bus` mirrors the desktop's
//! `process-changed` / `audio-devices-changed` / `volume-levels` stream to every
//! WS client.
//!
//! The engine is native and cannot run in a browser — the browser is always a
//! client. Default bind is loopback + a per-launch token; LAN is Phase C.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use crate::routing::engine::RoutingEngine;
use scene_store::SceneStore;
use settings_store::SettingsStore;

pub mod rpc;
pub mod scene_store;
pub mod settings_store;

/// One backend event mirrored to WS clients (same name + payload as `emit_all`).
#[derive(Clone, Debug, Serialize)]
pub struct ServerEvent {
    pub event: String,
    pub payload: Value,
}

/// Broadcast bus: the background tasks send, each WS connection subscribes.
pub type EventBus = broadcast::Sender<ServerEvent>;

/// Shared daemon state handed to every request.
#[derive(Clone)]
pub struct ServerState {
    pub engine: Arc<RoutingEngine>,
    /// Workspace document store — single source of truth for the scene (Phase B).
    pub scene: Arc<SceneStore>,
    /// Application settings store — mirrored + persisted like the scene (t14).
    pub settings: Arc<SettingsStore>,
    pub bus: EventBus,
    /// Empty = auth disabled (loopback dev only). Otherwise required on every call.
    pub token: String,
}

/// Where to reach the daemon — managed in Tauri so the desktop UI can show the
/// URL/token to paste into a browser or hand to Claude-preview.
#[derive(Clone, Debug, Serialize)]
pub struct ServerInfo {
    pub url: String,
    pub token: String,
}

/// A `/rpc` call: `{ "cmd": "...", "args": { ... } }`.
#[derive(Deserialize)]
pub struct RpcRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

fn authorized(state: &ServerState, provided: Option<&str>) -> bool {
    state.token.is_empty() || provided == Some(state.token.as_str())
}

/// Run the daemon until the process exits. Spawned on the Tauri async runtime.
pub async fn serve(state: ServerState, addr: SocketAddr) {
    // Loopback dev: allow any origin so the Vite preview (:5173) can POST to /rpc.
    let cors = CorsLayer::permissive();
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/rpc", post(rpc_handler))
        .route("/ws", get(ws_handler))
        .layer(cors)
        .with_state(state);

    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            info!("Nodus daemon listening on http://{addr}");
            if let Err(e) = axum::serve(listener, app).await {
                error!("daemon server error: {e}");
            }
        }
        Err(e) => error!("daemon failed to bind {addr}: {e}"),
    }
}

async fn rpc_handler(
    State(state): State<ServerState>,
    Query(auth): Query<AuthQuery>,
    Json(req): Json<RpcRequest>,
) -> impl IntoResponse {
    if !authorized(&state, auth.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error": "unauthorized"})))
            .into_response();
    }
    match rpc::dispatch(&state, req).await {
        Ok(result) => (StatusCode::OK, Json(json!({"ok": true, "result": result}))).into_response(),
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": e}))).into_response()
        }
    }
}

async fn ws_handler(
    State(state): State<ServerState>,
    Query(auth): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !authorized(&state, auth.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Each WS connection: push every bus event to the client; also accept inbound
/// `{cmd,args}` frames (fire-and-forget) so a client can issue commands over the
/// socket too. Phase B will add scene snapshot/patch frames here.
async fn handle_socket(socket: WebSocket, state: ServerState) {
    let (mut sender, mut receiver) = socket.split();
    // Subscribe BEFORE sending the initial snapshot so no scene update slips
    // through the gap between the snapshot and the live stream.
    let mut rx = state.bus.subscribe();

    // Hydrate the new client with the current scene document (Phase B) and the
    // current settings (t14), so it renders correct state on connect.
    for init in [
        serde_json::to_value(&state.scene.snapshot())
            .map(|payload| ServerEvent { event: "scene:snapshot".into(), payload }),
        serde_json::to_value(&state.settings.get())
            .map(|payload| ServerEvent { event: "settings:changed".into(), payload }),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(txt) = serde_json::to_string(&init) {
            let _ = sender.send(Message::Text(txt)).await;
        }
    }

    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let Ok(txt) = serde_json::to_string(&ev) else { continue };
                    if sender.send(Message::Text(txt)).await.is_err() {
                        break;
                    }
                }
                // Lagged: we dropped some events (slow client) — keep going.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(t) => {
                    if let Ok(req) = serde_json::from_str::<RpcRequest>(&t) {
                        let _ = rpc::dispatch(&recv_state, req).await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // When either half ends (client disconnects), tear the other down.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(token: &str) -> ServerState {
        let (bus, _rx) = broadcast::channel(8);
        ServerState {
            engine: Arc::new(RoutingEngine::new()),
            scene: Arc::new(SceneStore::new(None, bus.clone())),
            settings: Arc::new(SettingsStore::new(None, bus.clone())),
            bus,
            token: token.to_string(),
        }
    }

    #[test]
    fn empty_token_disables_auth() {
        let s = state("");
        assert!(authorized(&s, None));
        assert!(authorized(&s, Some("anything")));
    }

    #[test]
    fn token_must_match() {
        let s = state("secret");
        assert!(!authorized(&s, None));
        assert!(!authorized(&s, Some("wrong")));
        assert!(authorized(&s, Some("secret")));
    }

    #[test]
    fn server_event_serializes_flat() {
        let ev = ServerEvent {
            event: "volume-levels".into(),
            payload: json!({"dev-1": 0.5}),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"event\":\"volume-levels\""));
        assert!(s.contains("\"dev-1\":0.5"));
    }
}
