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
    /// An effect on the route: passes audio through but applies DSP (t18).
    Fx,
}

/// Which effect an Fx node applies (t18, Wave 1). Extensible: compressor/limiter/…
/// land in later waves without changing the contract shape.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FxKind {
    Gain,
    Gate,
    Eq,
    Limiter,
    Compressor,
}

/// FX parameters, flat + named so UI and engine share one shape. Fields are used
/// per `kind` (unused ones stay at default): gain_db (gain/eq), open_db/close_db
/// (gate), freq/q (eq). `bypassed` = pass through untouched (manual, or solo-skip).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FxSpec {
    pub kind: FxKind,
    #[serde(default)]
    pub bypassed: bool,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub open_db: f32,
    #[serde(default)]
    pub close_db: f32,
    #[serde(default)]
    pub freq: f32,
    #[serde(default)]
    pub q: f32,
    /// 5-band graphic EQ gains (dB) at fixed 60/250/1k/4k/16k Hz. UI-driven; the
    /// single-band biquad DSP still uses freq/q/gain_db (5-band DSP lands later).
    /// Fixed array keeps FxSpec `Copy`; `#[serde(default)]` = flat + back-compat.
    #[serde(default)]
    pub eq_bands: [f32; 5],
    /// Limiter threshold + ceiling (dB, −40…0). Compressor reuses threshold_db + ratio.
    /// UI-complete; DSP for limiter/compressor lands in the FX-functionality stage.
    #[serde(default)]
    pub threshold_db: f32,
    #[serde(default)]
    pub ceiling_db: f32,
    #[serde(default)]
    pub ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    /// Human-readable label (e.g. "Arma 3", "Headphones")
    pub label: String,
    /// Device ID as returned by WASAPI (empty for Splitter/Mixer/Fx and app-capture sources)
    pub device_id: String,
    /// Exe name for app-capture sources (e.g. "spotify.exe"). Mutually exclusive with device_id.
    #[serde(default)]
    pub exe_name: Option<String>,
    /// FX settings — present only on `Fx` nodes (t18).
    #[serde(default)]
    pub fx: Option<FxSpec>,
}

impl Node {
    pub fn new(node_type: NodeType, label: impl Into<String>, device_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_type,
            label: label.into(),
            device_id: device_id.into(),
            exe_name: None,
            fx: None,
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
    /// Stereo balance [-1.0 = full left .. 0.0 = center .. 1.0 = full right], default 0.0.
    /// Applied only to 2-channel routes; ignored for other channel counts.
    #[serde(default)]
    pub pan: f32,
}

impl Route {
    pub fn new(from_node: NodeId, to_node: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            from_node,
            to_node,
            volume: 1.0,
            muted: false,
            pan: 0.0,
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
