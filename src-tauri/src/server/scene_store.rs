//! t17 phase B — the workspace document store: the single source of truth for the
//! scene graph, shared by the Desktop and Web UIs and persisted to disk.
//!
//! Design: **state sync, not command sync.** The store keeps the scene as an
//! opaque JSON document (the very `{ tabs, activeId }` shape the React store uses)
//! — it does not understand nodes/edges/hubs. A client mutates locally (all that
//! rich logic stays in TS, unchanged), then pushes the whole document; the store
//! persists it, bumps a monotonic `rev`, and broadcasts a `scene:snapshot` so the
//! other UI replaces its state. Conflicts resolve last-writer-wins at document
//! granularity — fine for a mostly single-user tool. Per-edge volume/mute/pan keep
//! going straight to the engine live (Phase A); this only mirrors + persists the
//! visual document. Deltas are a later optimization.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::error;

use crate::server::{EventBus, ServerEvent};

/// What clients receive: the document, its revision, and (on a broadcast) which
/// client caused it, so that client can ignore its own echo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneSnapshot {
    pub doc: Value,
    pub rev: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// On-disk form (atomic-replaced).
#[derive(Serialize, Deserialize)]
struct Persisted {
    rev: u64,
    doc: Value,
}

struct Inner {
    doc: Value,
    rev: u64,
}

pub struct SceneStore {
    inner: Mutex<Inner>,
    path: Option<PathBuf>,
    bus: EventBus,
}

impl SceneStore {
    /// Load the persisted document if `path` exists, else start empty.
    pub fn new(path: Option<PathBuf>, bus: EventBus) -> Self {
        let (doc, rev) = path
            .as_ref()
            .and_then(|p| load(p))
            .unwrap_or((Value::Null, 0));
        Self {
            inner: Mutex::new(Inner { doc, rev }),
            path,
            bus,
        }
    }

    /// Current document + revision (no origin — this is a pull, not an echo).
    pub fn snapshot(&self) -> SceneSnapshot {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        SceneSnapshot {
            doc: g.doc.clone(),
            rev: g.rev,
            origin: None,
        }
    }

    /// Replace the document, persist, broadcast `scene:snapshot`, return new rev.
    pub fn push(&self, doc: Value, origin: Option<String>) -> u64 {
        let rev = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.doc = doc.clone();
            g.rev += 1;
            g.rev
        };
        if let Some(p) = &self.path {
            if let Err(e) = persist(p, &doc, rev) {
                error!("scene persist failed: {e}");
            }
        }
        let snap = SceneSnapshot { doc, rev, origin };
        if let Ok(payload) = serde_json::to_value(&snap) {
            let _ = self.bus.send(ServerEvent {
                event: "scene:snapshot".into(),
                payload,
            });
        }
        rev
    }
}

fn load(path: &PathBuf) -> Option<(Value, u64)> {
    let bytes = std::fs::read(path).ok()?;
    let p: Persisted = serde_json::from_slice(&bytes).ok()?;
    Some((p.doc, p.rev))
}

/// Atomic write: serialize → temp file → rename over the target.
fn persist(path: &PathBuf, doc: &Value, rev: u64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_vec_pretty(&Persisted {
        rev,
        doc: doc.clone(),
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store(path: Option<PathBuf>) -> SceneStore {
        let (bus, _rx) = tokio::sync::broadcast::channel(8);
        SceneStore::new(path, bus)
    }

    #[test]
    fn empty_by_default() {
        let s = store(None);
        let snap = s.snapshot();
        assert_eq!(snap.rev, 0);
        assert!(snap.doc.is_null());
    }

    #[test]
    fn push_increments_rev_and_keeps_doc() {
        let s = store(None);
        let r1 = s.push(json!({ "tabs": [], "activeId": "a" }), Some("c1".into()));
        let r2 = s.push(json!({ "tabs": [1], "activeId": "b" }), Some("c1".into()));
        assert_eq!(r1, 1);
        assert_eq!(r2, 2);
        let snap = s.snapshot();
        assert_eq!(snap.rev, 2);
        assert_eq!(snap.doc["activeId"], "b");
    }

    #[test]
    fn push_broadcasts_with_origin() {
        let (bus, mut rx) = tokio::sync::broadcast::channel(8);
        let s = SceneStore::new(None, bus);
        s.push(json!({ "x": 1 }), Some("client-7".into()));
        let ev = rx.try_recv().expect("a scene:snapshot was broadcast");
        assert_eq!(ev.event, "scene:snapshot");
        assert_eq!(ev.payload["rev"], 1);
        assert_eq!(ev.payload["origin"], "client-7");
        assert_eq!(ev.payload["doc"]["x"], 1);
    }

    #[test]
    fn persists_and_reloads() {
        let dir = std::env::temp_dir().join(format!("nodus-scene-test-{}", std::process::id()));
        let path = dir.join("workspace.json");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let s = store(Some(path.clone()));
            s.push(json!({ "tabs": [{ "id": "s1" }], "activeId": "s1" }), None);
        }
        // Fresh store from the same path must see the persisted doc + rev.
        let s2 = store(Some(path.clone()));
        let snap = s2.snapshot();
        assert_eq!(snap.rev, 1);
        assert_eq!(snap.doc["activeId"], "s1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
