//! Execution plan for the audio graph — the shape the engine should actually run (t33).
//!
//! `resolve_device_routes` flattens the graph into independent source→output routes,
//! inlining Mixer and Splitter away. That makes every node on a shared path exist
//! once PER ROUTE: a gate feeding two outputs becomes two gates with two detectors
//! and two states, writing into one telemetry cell. The graph says one thing and the
//! engine does another — and for a non-linear effect (gate, limiter, compressor) that
//! is not an approximation, it is a different sound. A limiter after a mixer must
//! limit the SUM; three limiters on three branches cannot.
//!
//! This module builds the other thing: a DAG where each graph node appears EXACTLY
//! ONCE, in topological order, so it can be processed once and its result shared.
//!
//! Deliberately pure — no audio, no threads, no WASAPI. The plan is a description
//! that can be asserted against in unit tests; running it is the executor's job.

use std::collections::HashMap;

use super::node::{FxSpec, NodeId, RouteId};

/// Where a bus node gets its signal, and what that edge contributes.
#[derive(Debug, Clone, PartialEq)]
pub struct BusInput {
    /// Index into `BusPlan::nodes` — always < the consuming node's own index,
    /// because the plan is topologically ordered.
    pub from: usize,
    /// The graph edge feeding this input. Its volume is the row fader on a hub
    /// input; the UI addresses that row by this id.
    pub edge: RouteId,
}

/// One unit of work in the plan.
#[derive(Debug, Clone, PartialEq)]
pub enum BusOp {
    /// A capture: an app (`exe`), a device, or a Nodus virtual endpoint.
    Source {
        /// Key the engine shares captures by — `exe:<name>` or the device id.
        key: String,
        device_id: String,
        exe_name: Option<String>,
        is_virtual: bool,
    },
    /// A 1→1 effect. Exactly one per FX node in the graph, which is the whole point.
    Fx {
        node_id: NodeId,
        spec: FxSpec,
        input: BusInput,
    },
    /// N→1 summing point: a Mixer, or an implicit sum where several edges land on a
    /// node that can only take one input (a stricter engine must not silently drop
    /// the extras — that would lose audio the user can see wired up).
    Sum { inputs: Vec<BusInput> },
    /// A pass-through: a Splitter, or a hub with a single input. It owns no work —
    /// fan-out is simply several consumers reading one node's output — but it stays
    /// in the plan so hub rows keep their identity for metering and trims.
    Pass { input: BusInput },
}

/// One node of the execution plan.
#[derive(Debug, Clone, PartialEq)]
pub struct BusNode {
    /// Graph node this came from (a Source/Fx/Mixer/Splitter id).
    pub node_id: NodeId,
    pub op: BusOp,
}

/// An output the plan feeds. Volume/mute/pan are NOT applied by the plan: they belong
/// to the branch, not to the shared upstream work. Muting one output must not silence
/// a gate that also feeds another — which is exactly the bug that made the gate's
/// meter flicker.
#[derive(Debug, Clone, PartialEq)]
pub struct BusSink {
    /// Node whose output this sink renders.
    pub input: usize,
    /// Final edge into the output — carries this branch's pan and is how the UI
    /// addresses the route.
    pub route_id: RouteId,
    /// Every edge traversed source→…→output. Effective volume is their product and
    /// effective mute their OR, so an intermediate fader still reaches this branch.
    pub chain: Vec<RouteId>,
    pub to_device_id: String,
    pub to_is_virtual_mic: bool,
}

/// The graph as the engine should run it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusPlan {
    /// Topologically ordered: every node's inputs have smaller indices.
    pub nodes: Vec<BusNode>,
    pub sinks: Vec<BusSink>,
}

impl BusPlan {
    /// How many times a graph node appears. The invariant this whole module exists
    /// for is that this is never more than 1.
    pub fn count_of(&self, node_id: &str) -> usize {
        self.nodes.iter().filter(|n| n.node_id == node_id).count()
    }

    pub fn index_of(&self, node_id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.node_id == node_id)
    }

    /// Nodes reading a given node's output. One consumer = a chain, several = fan-out.
    pub fn consumers_of(&self, index: usize) -> usize {
        self.nodes
            .iter()
            .filter(|n| match &n.op {
                BusOp::Source { .. } => false,
                BusOp::Fx { input, .. } | BusOp::Pass { input } => input.from == index,
                BusOp::Sum { inputs } => inputs.iter().any(|i| i.from == index),
            })
            .count()
            + self.sinks.iter().filter(|s| s.input == index).count()
    }
}

/// Builder state — kept out of `Graph` so the traversal stays readable.
pub(super) struct PlanBuilder {
    pub(super) nodes: Vec<BusNode>,
    pub(super) sinks: Vec<BusSink>,
    /// Graph node id → plan index. This memo IS the fix: visiting a node twice
    /// (because two outputs pull through it) returns the same instance instead of
    /// building a second one.
    pub(super) built: HashMap<NodeId, usize>,
    /// Nodes currently being resolved — a cycle would otherwise recurse forever.
    /// The graph rejects cycles on apply, so this is a belt-and-braces guard.
    pub(super) visiting: Vec<NodeId>,
}

impl PlanBuilder {
    pub(super) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            sinks: Vec::new(),
            built: HashMap::new(),
            visiting: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, node_id: &NodeId, op: BusOp) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(BusNode { node_id: node_id.clone(), op });
        self.built.insert(node_id.clone(), idx);
        idx
    }

    pub(super) fn finish(self) -> BusPlan {
        BusPlan { nodes: self.nodes, sinks: self.sinks }
    }
}

/// Fx spec fallback for an Fx node with no parameters yet — unity gain, so an
/// unconfigured node passes audio instead of dropping it.
pub(super) fn passthrough_spec() -> FxSpec {
    FxSpec {
        kind: super::node::FxKind::Gain,
        bypassed: true,
        gain_db: 0.0,
        open_db: 0.0,
        close_db: 0.0,
        freq: 0.0,
        q: 0.0,
        eq_bands: [0.0; 5],
        threshold_db: 0.0,
        ceiling_db: 0.0,
        ratio: 0.0,
    }
}
