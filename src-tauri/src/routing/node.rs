use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type NodeId = String;
pub type RouteId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Source,
    Output,
    Splitter,
    Mixer,
    Virtual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    /// Human-readable label (e.g. "Arma 3", "Headphones")
    pub label: String,
    /// Device ID as returned by WASAPI (empty for Splitter/Mixer and app-capture sources)
    pub device_id: String,
    /// Exe name for app-capture sources (e.g. "spotify.exe"). Mutually exclusive with device_id.
    #[serde(default)]
    pub exe_name: Option<String>,
}

impl Node {
    pub fn new(node_type: NodeType, label: impl Into<String>, device_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_type,
            label: label.into(),
            device_id: device_id.into(),
            exe_name: None,
        }
    }
}

/// A directed audio route from one node to another.
/// Volume and mute are per-route, not per-node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub id: RouteId,
    pub from_node: NodeId,
    pub to_node: NodeId,
    /// Linear gain [0.0 .. 1.0], default 1.0
    pub volume: f32,
    pub muted: bool,
}

impl Route {
    pub fn new(from_node: NodeId, to_node: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            from_node,
            to_node,
            volume: 1.0,
            muted: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_defaults_are_audible() {
        let r = Route::new("a".into(), "b".into());
        assert!(!r.muted);
        assert!((r.volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn node_gets_unique_ids() {
        let a = Node::new(NodeType::Source, "A", "");
        let b = Node::new(NodeType::Source, "B", "");
        assert_ne!(a.id, b.id);
    }
}
