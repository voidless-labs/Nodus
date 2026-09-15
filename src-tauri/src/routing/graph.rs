use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::bus::{passthrough_spec, BusInput, BusOp, BusPlan, BusSink, PlanBuilder};
use super::node::{FxSpec, Node, NodeId, NodeType, Route, RouteId};

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("node '{0}' not found")]
    NodeNotFound(NodeId),
    #[error("route '{0}' not found")]
    RouteNotFound(RouteId),
    #[error("duplicate route from '{from}' to '{to}'")]
    DuplicateRoute { from: NodeId, to: NodeId },
    #[error("routing cycle detected")]
    CycleDetected,
    #[error("invalid volume {0}: must be in [0.0, 1.0]")]
    InvalidVolume(f32),
}

/// Serializable snapshot of the full routing graph sent from UI.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingGraph {
    pub nodes: Vec<Node>,
    pub routes: Vec<Route>,
}

/// In-memory routing graph with fast lookups.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: HashMap<NodeId, Node>,
    routes: HashMap<RouteId, Route>,
    /// Outgoing routes per source node
    outgoing: HashMap<NodeId, Vec<RouteId>>,
    /// Incoming routes per destination node
    incoming: HashMap<NodeId, Vec<RouteId>>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the entire graph from a serialized snapshot.
    /// Rejects graphs containing a routing cycle (which would feed audio back on
    /// itself), and drops duplicate or dangling edges. Validation runs *before*
    /// any mutation, so a bad snapshot leaves the current graph intact.
    pub fn apply_snapshot(&mut self, snapshot: RoutingGraph) -> Result<(), GraphError> {
        Self::detect_cycle(&snapshot)?;

        self.nodes.clear();
        self.routes.clear();
        self.outgoing.clear();
        self.incoming.clear();

        for node in snapshot.nodes {
            self.nodes.insert(node.id.clone(), node);
        }
        // Insert routes, skipping edges to/from unknown nodes and duplicate from→to pairs.
        let mut seen: std::collections::HashSet<(NodeId, NodeId)> = std::collections::HashSet::new();
        for route in snapshot.routes {
            if !self.nodes.contains_key(&route.from_node)
                || !self.nodes.contains_key(&route.to_node)
            {
                continue;
            }
            if seen.insert((route.from_node.clone(), route.to_node.clone())) {
                self.insert_route_unchecked(route);
            }
        }
        Ok(())
    }

    /// Detect a directed cycle among the snapshot's nodes (iterative DFS, 3-colour).
    fn detect_cycle(snapshot: &RoutingGraph) -> Result<(), GraphError> {
        use std::collections::HashSet;
        let nodes: HashSet<NodeId> = snapshot.nodes.iter().map(|n| n.id.clone()).collect();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for r in &snapshot.routes {
            if nodes.contains(&r.from_node) && nodes.contains(&r.to_node) {
                adj.entry(r.from_node.clone()).or_default().push(r.to_node.clone());
            }
        }
        // colour: 0 = unvisited, 1 = on current DFS stack (grey), 2 = done (black)
        let mut colour: HashMap<NodeId, u8> = HashMap::new();
        for start in &nodes {
            if colour.get(start).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut stack: Vec<(NodeId, usize)> = vec![(start.clone(), 0)];
            colour.insert(start.clone(), 1);
            while let Some((node, idx)) = stack.last().cloned() {
                let children = adj.get(&node).map(|v| v.as_slice()).unwrap_or(&[]);
                if idx < children.len() {
                    if let Some(top) = stack.last_mut() {
                        top.1 += 1;
                    }
                    let next = children[idx].clone();
                    match colour.get(&next).copied().unwrap_or(0) {
                        0 => {
                            colour.insert(next.clone(), 1);
                            stack.push((next, 0));
                        }
                        1 => return Err(GraphError::CycleDetected), // back edge → cycle
                        _ => {}
                    }
                } else {
                    colour.insert(node, 2);
                    stack.pop();
                }
            }
        }
        Ok(())
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Result<(), GraphError> {
        if !self.nodes.contains_key(id) {
            return Err(GraphError::NodeNotFound(id.clone()));
        }
        // Remove all routes touching this node
        let touching: Vec<RouteId> = self
            .routes
            .values()
            .filter(|r| &r.from_node == id || &r.to_node == id)
            .map(|r| r.id.clone())
            .collect();
        for rid in touching {
            let _ = self.remove_route(&rid);
        }
        self.nodes.remove(id);
        Ok(())
    }

    pub fn add_route(&mut self, route: Route) -> Result<(), GraphError> {
        if !self.nodes.contains_key(&route.from_node) {
            return Err(GraphError::NodeNotFound(route.from_node.clone()));
        }
        if !self.nodes.contains_key(&route.to_node) {
            return Err(GraphError::NodeNotFound(route.to_node.clone()));
        }
        // Check for duplicates
        let duplicate = self
            .routes
            .values()
            .any(|r| r.from_node == route.from_node && r.to_node == route.to_node);
        if duplicate {
            return Err(GraphError::DuplicateRoute {
                from: route.from_node.clone(),
                to: route.to_node.clone(),
            });
        }
        self.insert_route_unchecked(route);
        Ok(())
    }

    pub fn remove_route(&mut self, id: &RouteId) -> Result<(), GraphError> {
        let route = self
            .routes
            .remove(id)
            .ok_or_else(|| GraphError::RouteNotFound(id.clone()))?;
        if let Some(out) = self.outgoing.get_mut(&route.from_node) {
            out.retain(|r| r != id);
        }
        if let Some(inc) = self.incoming.get_mut(&route.to_node) {
            inc.retain(|r| r != id);
        }
        Ok(())
    }

    pub fn set_mute(&mut self, route_id: &RouteId, muted: bool) -> Result<(), GraphError> {
        self.routes
            .get_mut(route_id)
            .ok_or_else(|| GraphError::RouteNotFound(route_id.clone()))
            .map(|r| r.muted = muted)
    }

    pub fn set_volume(&mut self, route_id: &RouteId, volume: f32) -> Result<(), GraphError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(GraphError::InvalidVolume(volume));
        }
        self.routes
            .get_mut(route_id)
            .ok_or_else(|| GraphError::RouteNotFound(route_id.clone()))
            .map(|r| r.volume = volume)
    }

    pub fn set_pan(&mut self, route_id: &RouteId, pan: f32) -> Result<(), GraphError> {
        self.routes
            .get_mut(route_id)
            .ok_or_else(|| GraphError::RouteNotFound(route_id.clone()))
            .map(|r| r.pan = pan.clamp(-1.0, 1.0))
    }

    /// Update an FX node's parameters in the graph.
    ///
    /// The live params store is what renderers actually read, but the graph is what a
    /// route is REBUILT from. Without writing through here, a knob turned while a route
    /// waits for its app — its FX node has no params store yet, so the live write is a
    /// no-op — would be silently lost the moment the route finally wires. The route
    /// setters below already write through; this closes the same gap for FX. (t31)
    pub fn set_node_fx(&mut self, node_id: &NodeId, spec: FxSpec) -> Result<(), GraphError> {
        self.nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.clone()))
            .map(|n| n.fx = Some(spec))
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_route(&self, id: &RouteId) -> Option<&Route> {
        self.routes.get(id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.routes.values()
    }

    /// Routes leaving a given source node.
    pub fn routes_from(&self, node_id: &NodeId) -> Vec<&Route> {
        self.outgoing
            .get(node_id)
            .map(|ids| ids.iter().filter_map(|id| self.routes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Routes arriving at a given destination node.
    pub fn routes_to(&self, node_id: &NodeId) -> Vec<&Route> {
        self.incoming
            .get(node_id)
            .map(|ids| ids.iter().filter_map(|id| self.routes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Source nodes (nodes with no incoming routes).
    pub fn source_nodes(&self) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|n| {
                self.incoming
                    .get(&n.id)
                    .map(|v| v.is_empty())
                    .unwrap_or(true)
            })
            .collect()
    }

    fn insert_route_unchecked(&mut self, route: Route) {
        let id = route.id.clone();
        let from = route.from_node.clone();
        let to = route.to_node.clone();
        self.routes.insert(id.clone(), route);
        self.outgoing.entry(from).or_default().push(id.clone());
        self.incoming.entry(to).or_default().push(id);
    }
}

/// One FX node on a resolved route, in signal order (source→…→output). Carries the
/// node id (so the engine keys live params by it) and the current settings. (t18)
#[derive(Debug, Clone)]
pub struct FxInstance {
    pub node_id: NodeId,
    pub spec: FxSpec,
}

/// A point where a route touches a Mixer/Splitter, so the hub's per-input dot can
/// show the signal actually arriving there.
///
/// Resolving inlines hubs away, which is why this has to be recorded here: once the
/// route is flat there is nothing left to say "a Mixer input lives at this edge".
/// Note this is NOT blocked by the missing summing bus (t33) — a hub INPUT carries
/// one source, not a sum; the summing happens after it.
#[derive(Debug, Clone)]
pub struct HubTap {
    /// Edge that lands on (or leaves) the hub — the UI addresses its rows by port,
    /// and each port maps to exactly this edge.
    pub edge: RouteId,
    /// Last FX before this point, whose OUTPUT is the signal here. `None` when the
    /// signal comes straight off the source capture.
    pub after_fx: Option<NodeId>,
}

/// Active routes visible to the engine: flattened list of (from_device_id, to_device_id, volume, muted).
#[derive(Debug, Clone)]
pub struct ActiveRoute {
    pub route_id: RouteId,
    /// FX nodes traversed source→…→output, in order — applied in the render chain (t18).
    pub fx_chain: Vec<FxInstance>,
    /// Every edge id traversed source→…→output for this physical route. The
    /// effective volume is the product of these edges' volumes; the engine
    /// recomputes it live when ANY edge in the chain changes (so a Mixer-input
    /// slider — an intermediate edge — takes effect without a graph restart).
    pub chain: Vec<RouteId>,
    pub from_device_id: String,
    /// Set for app-capture sources (e.g. "spotify.exe") — engine resolves device via session mgr.
    pub exe_name: Option<String>,
    /// True when the source is a Nodus virtual endpoint — the engine reads it from
    /// the kernel driver's ring buffer (VirtualCapture), falling back to WASAPI loopback.
    pub from_is_virtual: bool,
    pub to_device_id: String,
    /// True when the DESTINATION is the Nodus virtual microphone — the engine
    /// writes the route's audio into the kernel driver's mic ring (VirtualRender)
    /// instead of rendering to a WASAPI endpoint. Detected by the destination
    /// node's label via `is_nodus_virtual_mic_name`.
    pub to_is_virtual_mic: bool,
    pub volume: f32,
    pub muted: bool,
    /// Stereo balance of the final edge into the output [-1.0 .. 1.0].
    pub pan: f32,
    /// Hub edges this route passes through, in signal order (t18 wave 6b).
    pub hub_taps: Vec<HubTap>,
}

impl Graph {
    /// Resolve the graph into an execution plan where every node exists ONCE (t33).
    ///
    /// The traversal runs backwards from each output and memoises by node id, so a
    /// node feeding two outputs is built once and shared. That is the whole
    /// difference from `resolve_device_routes`, which walks forwards from sources
    /// and therefore rebuilds the shared part of the path per output.
    pub fn resolve_bus_plan(&self) -> BusPlan {
        let mut b = PlanBuilder::new();
        // Outputs in a stable order so the plan is deterministic (tests, and a
        // reproducible engine are worth more than the map's iteration order).
        let mut outputs: Vec<&Node> = self
            .nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::Output | NodeType::Virtual))
            .filter(|n| !n.device_id.is_empty())
            .collect();
        outputs.sort_by(|a, b| a.id.cmp(&b.id));

        for out in outputs {
            let mut incoming = self.routes_to(&out.id);
            incoming.sort_by(|a, b| a.id.cmp(&b.id));
            for route in incoming {
                // A Virtual node with incoming edges is an output (the virtual mic);
                // one without is a source. `routes_to` being non-empty settles it.
                let Some(from) = self.build_bus_node(&route.from_node, &mut b) else {
                    continue;
                };
                let mut chain = Vec::new();
                self.collect_chain_edges(&route.from_node, &mut chain);
                chain.push(route.id.clone());
                b.sinks.push(BusSink {
                    input: from,
                    route_id: route.id.clone(),
                    chain,
                    to_device_id: out.device_id.clone(),
                    to_is_virtual_mic:
                        crate::virtual_audio::virtual_device::is_nodus_virtual_mic_name(&out.label),
                });
            }
        }
        b.finish()
    }

    /// Every edge upstream of `node`, so a branch still honours intermediate faders.
    /// Order is signal-ish but not guaranteed; only the SET matters (product / OR).
    fn collect_chain_edges(&self, node: &NodeId, out: &mut Vec<RouteId>) {
        for r in self.routes_to(node) {
            if out.contains(&r.id) {
                continue; // shared upstream reached from two branches
            }
            out.push(r.id.clone());
            self.collect_chain_edges(&r.from_node, out);
        }
    }

    /// Build (or reuse) the plan node for a graph node. `None` when the node cannot
    /// produce audio — an unconfigured source, or an output used as an input.
    fn build_bus_node(&self, id: &NodeId, b: &mut PlanBuilder) -> Option<usize> {
        if let Some(&idx) = b.built.get(id) {
            return Some(idx); // already built — THIS is what makes it exist once
        }
        if b.visiting.contains(id) {
            return None; // cycle guard (apply_snapshot rejects these, but be safe)
        }
        let node = self.nodes.get(id)?;

        match node.node_type {
            NodeType::Source | NodeType::Virtual => {
                if node.device_id.is_empty() && node.exe_name.is_none() {
                    return None;
                }
                let key = match &node.exe_name {
                    Some(exe) => format!("exe:{exe}"),
                    None => node.device_id.clone(),
                };
                Some(b.push(
                    id,
                    BusOp::Source {
                        key,
                        device_id: node.device_id.clone(),
                        exe_name: node.exe_name.clone(),
                        is_virtual: crate::virtual_audio::virtual_device::is_nodus_virtual_name(
                            &node.label,
                        ),
                    },
                ))
            }
            NodeType::Fx | NodeType::Mixer | NodeType::Splitter => {
                b.visiting.push(id.clone());
                let mut incoming = self.routes_to(id);
                incoming.sort_by(|a, b| a.id.cmp(&b.id));
                let mut inputs = Vec::new();
                for r in incoming {
                    if let Some(from) = self.build_bus_node(&r.from_node, b) {
                        inputs.push(BusInput { from, edge: r.id.clone() });
                    }
                }
                b.visiting.pop();
                if inputs.is_empty() {
                    return None; // nothing feeds it — it produces nothing
                }
                let op = match node.node_type {
                    // A 1→1 effect fed by several edges still must not drop audio the
                    // user wired up: sum first, then process once.
                    NodeType::Fx if inputs.len() > 1 => {
                        let sum = b.push(&format!("{id}::sum"), BusOp::Sum { inputs });
                        BusOp::Fx {
                            node_id: id.clone(),
                            spec: node.fx.unwrap_or_else(passthrough_spec),
                            input: BusInput { from: sum, edge: String::new() },
                        }
                    }
                    NodeType::Fx => BusOp::Fx {
                        node_id: id.clone(),
                        spec: node.fx.unwrap_or_else(passthrough_spec),
                        input: inputs.remove(0),
                    },
                    // A mixer sums; a splitter (or a single-input mixer) passes through
                    // and fan-out happens by several consumers reading it.
                    NodeType::Mixer if inputs.len() > 1 => BusOp::Sum { inputs },
                    _ => BusOp::Pass { input: inputs.remove(0) },
                };
                Some(b.push(id, op))
            }
            NodeType::Output => None, // an output never feeds anything
        }
    }

    /// Resolve the graph into device-level active routes for the engine.
    /// Splitter/Mixer nodes are inlined — only Source→Output device pairs remain.
    pub fn resolve_device_routes(&self) -> Vec<ActiveRoute> {
        let mut result = Vec::new();

        for node in self.nodes.values() {
            if node.node_type == NodeType::Source || node.node_type == NodeType::Virtual {
                // Skip nodes with neither a device ID nor an exe name
                if node.device_id.is_empty() && node.exe_name.is_none() {
                    continue;
                }
                let is_virtual =
                    crate::virtual_audio::virtual_device::is_nodus_virtual_name(&node.label);
                self.collect_device_routes(
                    &node.id,
                    &node.device_id,
                    node.exe_name.clone(),
                    is_virtual,
                    1.0,
                    false,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    &mut result,
                    0,
                );
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_device_routes(
        &self,
        current: &NodeId,
        source_device: &str,
        source_exe: Option<String>,
        source_is_virtual: bool,
        inherited_volume: f32,
        inherited_mute: bool,
        chain: Vec<RouteId>,
        fx_chain: Vec<FxInstance>,
        hub_taps: Vec<HubTap>,
        out: &mut Vec<ActiveRoute>,
        depth: usize,
    ) {
        if depth > 16 {
            return;
        }
        // Edges LEAVING a splitter are hub rows too — its UI rows are its outputs.
        // A mixer's single output carries the sum, which no row displays, so it gets
        // no tap: publishing a figure nothing reads would only invite misreading it.
        let from_splitter = matches!(
            self.nodes.get(current).map(|n| &n.node_type),
            Some(NodeType::Splitter)
        );
        for route in self.routes_from(current) {
            let volume = inherited_volume * route.volume;
            let muted = inherited_mute || route.muted;
            let mut chain = chain.clone();
            chain.push(route.id.clone());
            let mut hub_taps = hub_taps.clone();
            if from_splitter {
                hub_taps.push(HubTap {
                    edge: route.id.clone(),
                    after_fx: fx_chain.last().map(|f| f.node_id.clone()),
                });
            }

            if let Some(dest) = self.nodes.get(&route.to_node) {
                match dest.node_type {
                    NodeType::Output | NodeType::Virtual => {
                        if !dest.device_id.is_empty() {
                            out.push(ActiveRoute {
                                route_id: route.id.clone(),
                                fx_chain: fx_chain.clone(),
                                chain: chain.clone(),
                                from_device_id: source_device.to_string(),
                                exe_name: source_exe.clone(),
                                from_is_virtual: source_is_virtual,
                                to_device_id: dest.device_id.clone(),
                                to_is_virtual_mic:
                                    crate::virtual_audio::virtual_device::is_nodus_virtual_mic_name(
                                        &dest.label,
                                    ),
                                volume,
                                muted,
                                pan: route.pan,
                                hub_taps: hub_taps.clone(),
                            });
                        }
                    }
                    NodeType::Splitter | NodeType::Mixer => {
                        // The edge landing on the hub IS a row: a mixer input, or a
                        // splitter's single input.
                        let mut hub_taps = hub_taps.clone();
                        hub_taps.push(HubTap {
                            edge: route.id.clone(),
                            after_fx: fx_chain.last().map(|f| f.node_id.clone()),
                        });
                        self.collect_device_routes(
                            &dest.id,
                            source_device,
                            source_exe.clone(),
                            source_is_virtual,
                            volume,
                            muted,
                            chain.clone(),
                            fx_chain.clone(),
                            hub_taps,
                            out,
                            depth + 1,
                        );
                    }
                    NodeType::Fx => {
                        // Pass through, but record the FX (in signal order) so the
                        // engine applies its DSP on the buffer flowing through here.
                        let mut fx_chain = fx_chain.clone();
                        if let Some(spec) = dest.fx {
                            fx_chain.push(FxInstance { node_id: dest.id.clone(), spec });
                        }
                        self.collect_device_routes(
                            &dest.id,
                            source_device,
                            source_exe.clone(),
                            source_is_virtual,
                            volume,
                            muted,
                            chain.clone(),
                            fx_chain,
                            hub_taps,
                            out,
                            depth + 1,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::node::NodeType;

    fn make_node(t: NodeType, dev: &str) -> Node {
        Node::new(t, "test", dev)
    }

    #[test]
    fn add_and_remove_node() {
        let mut g = Graph::new();
        let n = make_node(NodeType::Source, "dev-1");
        let id = n.id.clone();
        g.add_node(n);
        assert!(g.get_node(&id).is_some());
        g.remove_node(&id).unwrap();
        assert!(g.get_node(&id).is_none());
    }

    #[test]
    fn add_route_requires_existing_nodes() {
        let mut g = Graph::new();
        let r = Route::new("ghost-a".into(), "ghost-b".into());
        assert!(matches!(g.add_route(r), Err(GraphError::NodeNotFound(_))));
    }

    #[test]
    fn duplicate_route_rejected() {
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "d1");
        let dst = make_node(NodeType::Output, "d2");
        let sid = src.id.clone();
        let did = dst.id.clone();
        g.add_node(src);
        g.add_node(dst);
        let r1 = Route::new(sid.clone(), did.clone());
        let r2 = Route::new(sid.clone(), did.clone());
        g.add_route(r1).unwrap();
        assert!(matches!(g.add_route(r2), Err(GraphError::DuplicateRoute { .. })));
    }

    #[test]
    fn set_mute_and_volume() {
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "d1");
        let dst = make_node(NodeType::Output, "d2");
        let sid = src.id.clone();
        let did = dst.id.clone();
        g.add_node(src);
        g.add_node(dst);
        let r = Route::new(sid, did);
        let rid = r.id.clone();
        g.add_route(r).unwrap();

        g.set_mute(&rid, true).unwrap();
        assert!(g.get_route(&rid).unwrap().muted);

        g.set_volume(&rid, 0.5).unwrap();
        assert!((g.get_route(&rid).unwrap().volume - 0.5).abs() < f32::EPSILON);

        assert!(matches!(g.set_volume(&rid, 1.5), Err(GraphError::InvalidVolume(_))));

        // Pan defaults to centre and clamps to [-1, 1].
        assert_eq!(g.get_route(&rid).unwrap().pan, 0.0);
        g.set_pan(&rid, -0.5).unwrap();
        assert!((g.get_route(&rid).unwrap().pan + 0.5).abs() < f32::EPSILON);
        g.set_pan(&rid, 2.0).unwrap();
        assert_eq!(g.get_route(&rid).unwrap().pan, 1.0);
    }

    #[test]
    fn apply_snapshot_rejects_cycle() {
        // a → b → a is a cycle and must be rejected (graph left unchanged).
        let a = make_node(NodeType::Source, "da");
        let b = make_node(NodeType::Mixer, "");
        let r1 = Route::new(a.id.clone(), b.id.clone());
        let r2 = Route::new(b.id.clone(), a.id.clone());
        let snap = RoutingGraph { nodes: vec![a, b], routes: vec![r1, r2] };

        let mut g = Graph::new();
        assert!(matches!(g.apply_snapshot(snap), Err(GraphError::CycleDetected)));
        assert_eq!(g.nodes().count(), 0, "rejected snapshot must not mutate the graph");
    }

    #[test]
    fn apply_snapshot_dedups_and_drops_dangling() {
        let a = make_node(NodeType::Source, "da");
        let b = make_node(NodeType::Output, "db");
        let dup1 = Route::new(a.id.clone(), b.id.clone());
        let dup2 = Route::new(a.id.clone(), b.id.clone()); // duplicate from→to
        let dangling = Route::new(a.id.clone(), "ghost".into()); // unknown dest
        let snap = RoutingGraph {
            nodes: vec![a, b],
            routes: vec![dup1, dup2, dangling],
        };
        let mut g = Graph::new();
        g.apply_snapshot(snap).unwrap();
        assert_eq!(g.routes().count(), 1, "duplicate and dangling edges dropped");
    }

    #[test]
    fn resolve_source_to_output() {
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let dst = make_node(NodeType::Output, "dst-dev");
        let sid = src.id.clone();
        let did = dst.id.clone();
        g.add_node(src);
        g.add_node(dst);
        g.add_route(Route::new(sid, did)).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].from_device_id, "src-dev");
        assert_eq!(routes[0].to_device_id, "dst-dev");
    }

    #[test]
    fn mixer_paths_share_the_output_edge_route_id() {
        // Two sources → one Mixer → one Output.
        // Both source paths are traversed through the SAME mixer→output edge,
        // so they resolve to two ActiveRoutes sharing that edge's route_id.
        // The engine must store these as a list under one id (not overwrite),
        // otherwise one mixer input is silently dropped and its renderer leaked.
        let mut g = Graph::new();
        let src_a = make_node(NodeType::Source, "src-a");
        let src_b = make_node(NodeType::Source, "src-b");
        let mix = make_node(NodeType::Mixer, "");
        let out = make_node(NodeType::Output, "out-dev");

        let a = src_a.id.clone();
        let b = src_b.id.clone();
        let m = mix.id.clone();
        let o = out.id.clone();

        g.add_node(src_a);
        g.add_node(src_b);
        g.add_node(mix);
        g.add_node(out);
        g.add_route(Route::new(a, m.clone())).unwrap();
        g.add_route(Route::new(b, m.clone())).unwrap();
        let out_edge = Route::new(m, o);
        let out_edge_id = out_edge.id.clone();
        g.add_route(out_edge).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 2, "both mixer inputs must produce a route");
        assert!(
            routes.iter().all(|r| r.route_id == out_edge_id),
            "both paths must carry the shared mixer→output edge id"
        );
        // Distinct sources, same destination.
        let froms: std::collections::HashSet<_> =
            routes.iter().map(|r| r.from_device_id.as_str()).collect();
        assert_eq!(froms.len(), 2);
        assert!(routes.iter().all(|r| r.to_device_id == "out-dev"));
        // Each path's chain is [source→mixer edge, mixer→output edge], ending with
        // the shared output edge — the engine recomputes effective volume from it,
        // so an intermediate (Mixer-input) edge's volume takes effect live.
        assert!(routes.iter().all(|r| r.chain.len() == 2), "chain = both edges");
        assert!(routes.iter().all(|r| r.chain.last() == Some(&out_edge_id)));
    }

    #[test]
    fn resolve_marks_virtual_mic_destination() {
        // Destination labelled "Nodus Virtual Mic" → to_is_virtual_mic = true;
        // ordinary outputs and the Nodus virtual SPEAKER stay false.
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let mic = Node::new(NodeType::Virtual, "Nodus Virtual Mic", "mic-dev");
        let phones = Node::new(NodeType::Output, "Наушники", "phones-dev");
        let speaker = Node::new(NodeType::Virtual, "Nodus Virtual Speaker", "spk-dev");

        let sid = src.id.clone();
        let mid = mic.id.clone();
        let pid = phones.id.clone();
        let vid = speaker.id.clone();

        g.add_node(src);
        g.add_node(mic);
        g.add_node(phones);
        g.add_node(speaker);
        g.add_route(Route::new(sid.clone(), mid)).unwrap();
        g.add_route(Route::new(sid.clone(), pid)).unwrap();
        g.add_route(Route::new(sid, vid)).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 3);
        for r in &routes {
            match r.to_device_id.as_str() {
                "mic-dev" => assert!(r.to_is_virtual_mic, "virtual mic dest must be flagged"),
                "phones-dev" | "spk-dev" => {
                    assert!(!r.to_is_virtual_mic, "{} must not be flagged", r.to_device_id)
                }
                other => panic!("unexpected destination {other}"),
            }
        }
    }

    fn fx_node(kind: crate::routing::node::FxKind, label: &str) -> Node {
        let mut n = Node::new(NodeType::Fx, label, "");
        n.fx = Some(FxSpec {
            kind,
            bypassed: false,
            gain_db: 0.0,
            open_db: -45.0,
            close_db: -55.0,
            freq: 1000.0,
            q: 1.0,
            eq_bands: [0.0; 5],
            threshold_db: 0.0,
            ceiling_db: 0.0,
            ratio: 0.0,
        });
        n
    }

    /// t33, the exact chain from the user's screenshot:
    /// `Mic → Gate → EQ → Splitter →` (out A) and `→ Mixer →` (out B).
    ///
    /// Today's resolve builds the shared part ONCE PER OUTPUT — two gates, two
    /// detectors, two states writing into one telemetry cell, which is why muting
    /// one output made the gate's meter and badge flicker. The plan must contain
    /// exactly one of each shared node.
    #[test]
    fn bus_plan_builds_a_shared_effect_once_not_once_per_output() {
        use crate::routing::node::FxKind;
        let mut g = Graph::new();
        let mic = make_node(NodeType::Source, "mic-dev");
        let gate = fx_node(FxKind::Gate, "Noise Gate");
        let eq = fx_node(FxKind::Eq, "EQ");
        let split = Node::new(NodeType::Splitter, "Splitter", "");
        let mixer = Node::new(NodeType::Mixer, "Mixer", "");
        let out_a = make_node(NodeType::Output, "headphones");
        let out_b = make_node(NodeType::Output, "cable");
        let (mid, gid, eid, sid, xid, aid, bid) = (
            mic.id.clone(), gate.id.clone(), eq.id.clone(), split.id.clone(),
            mixer.id.clone(), out_a.id.clone(), out_b.id.clone(),
        );
        for n in [mic, gate, eq, split, mixer, out_a, out_b] {
            g.add_node(n);
        }
        g.add_route(Route::new(mid, gid.clone())).unwrap();
        g.add_route(Route::new(gid.clone(), eid.clone())).unwrap();
        g.add_route(Route::new(eid.clone(), sid.clone())).unwrap();
        g.add_route(Route::new(sid.clone(), aid)).unwrap();
        g.add_route(Route::new(sid.clone(), xid.clone())).unwrap();
        g.add_route(Route::new(xid, bid)).unwrap();

        let plan = g.resolve_bus_plan();
        assert_eq!(plan.count_of(&gid), 1, "one gate, not one per output");
        assert_eq!(plan.count_of(&eid), 1, "one EQ, not one per output");
        assert_eq!(plan.sinks.len(), 2, "both outputs are still fed");
        let split_idx = plan.index_of(&sid).expect("splitter is in the plan");
        assert_eq!(plan.consumers_of(split_idx), 2, "splitter fans out to two branches");

        // Contrast with the flat resolve this replaces: it produces the shared work
        // twice, which is the defect — kept as an assertion so the difference is
        // documented rather than asserted from memory.
        let flat = g.resolve_device_routes();
        assert_eq!(flat.len(), 2);
        assert!(
            flat.iter().all(|r| r.fx_chain.iter().any(|f| f.node_id == gid)),
            "the flat resolve puts the SAME gate on both routes — two instances"
        );
    }

    /// A limiter after a mixer must limit the SUM. The plan has to sum first and
    /// process once, or the guarantee is arithmetically impossible.
    #[test]
    fn bus_plan_sums_mixer_inputs_before_the_effect() {
        use crate::routing::node::FxKind;
        let mut g = Graph::new();
        let a = make_node(NodeType::Source, "dev-a");
        let b = make_node(NodeType::Source, "dev-b");
        let c = make_node(NodeType::Source, "dev-c");
        let mixer = Node::new(NodeType::Mixer, "Mixer", "");
        let lim = fx_node(FxKind::Limiter, "Limiter");
        let out = make_node(NodeType::Output, "out-dev");
        let (aid, bid, cid, mid, lid, oid) = (
            a.id.clone(), b.id.clone(), c.id.clone(),
            mixer.id.clone(), lim.id.clone(), out.id.clone(),
        );
        for n in [a, b, c, mixer, lim, out] {
            g.add_node(n);
        }
        for s in [aid, bid, cid] {
            g.add_route(Route::new(s, mid.clone())).unwrap();
        }
        g.add_route(Route::new(mid.clone(), lid.clone())).unwrap();
        g.add_route(Route::new(lid.clone(), oid)).unwrap();

        let plan = g.resolve_bus_plan();
        assert_eq!(plan.count_of(&lid), 1, "one limiter on the bus");
        let mixer_idx = plan.index_of(&mid).expect("mixer is in the plan");
        match &plan.nodes[mixer_idx].op {
            BusOp::Sum { inputs } => assert_eq!(inputs.len(), 3, "all three sources summed"),
            other => panic!("mixer must be a summing point, got {other:?}"),
        }
        // Topological order is what lets the executor process each node once, in one
        // pass, without re-entering an upstream node.
        let lim_idx = plan.index_of(&lid).expect("limiter is in the plan");
        assert!(lim_idx > mixer_idx, "the limiter runs after the sum");
        for (i, n) in plan.nodes.iter().enumerate() {
            match &n.op {
                BusOp::Source { .. } => {}
                BusOp::Fx { input, .. } | BusOp::Pass { input } => assert!(input.from < i),
                BusOp::Sum { inputs } => assert!(inputs.iter().all(|x| x.from < i)),
            }
        }
    }

    /// A hub row must be metered where its signal actually is. Source → Gate → Mixer
    /// has to point at the GATE's output: reading the source instead would light the
    /// row for a microphone sitting behind a shut gate.
    #[test]
    fn hub_taps_point_at_whatever_feeds_the_row() {
        use crate::routing::node::{FxKind, FxSpec};
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let mut gate = Node::new(NodeType::Fx, "Gate", "");
        gate.fx = Some(FxSpec {
            kind: FxKind::Gate,
            bypassed: false,
            gain_db: 0.0,
            open_db: -45.0,
            close_db: -55.0,
            freq: 0.0,
            q: 0.0,
            eq_bands: [0.0; 5],
            threshold_db: 0.0,
            ceiling_db: 0.0,
            ratio: 0.0,
        });
        let mixer = Node::new(NodeType::Mixer, "Mix", "");
        let out = make_node(NodeType::Output, "out-dev");
        let (sid, gid, mid, oid) =
            (src.id.clone(), gate.id.clone(), mixer.id.clone(), out.id.clone());
        g.add_node(src);
        g.add_node(gate);
        g.add_node(mixer);
        g.add_node(out);
        g.add_route(Route::new(sid, gid.clone())).unwrap();
        let into_hub = Route::new(gid.clone(), mid.clone());
        let into_hub_id = into_hub.id.clone();
        g.add_route(into_hub).unwrap();
        g.add_route(Route::new(mid, oid)).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 1);
        let taps = &routes[0].hub_taps;
        assert_eq!(taps.len(), 1, "one hub row on this path (the mixer input)");
        assert_eq!(taps[0].edge, into_hub_id, "the row is addressed by its own edge");
        assert_eq!(
            taps[0].after_fx.as_deref(),
            Some(gid.as_str()),
            "metered at the gate's output, not at the source"
        );
    }

    /// With nothing between source and hub the tap falls back to the capture.
    #[test]
    fn hub_tap_without_fx_reads_the_source_capture() {
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let mixer = Node::new(NodeType::Mixer, "Mix", "");
        let out = make_node(NodeType::Output, "out-dev");
        let (sid, mid, oid) = (src.id.clone(), mixer.id.clone(), out.id.clone());
        g.add_node(src);
        g.add_node(mixer);
        g.add_node(out);
        let into_hub = Route::new(sid, mid.clone());
        let into_hub_id = into_hub.id.clone();
        g.add_route(into_hub).unwrap();
        g.add_route(Route::new(mid, oid)).unwrap();

        let taps = &g.resolve_device_routes()[0].hub_taps;
        assert_eq!(taps.len(), 1);
        assert_eq!(taps[0].edge, into_hub_id);
        assert!(taps[0].after_fx.is_none(), "no FX before the hub");
    }

    #[test]
    fn resolve_collects_fx_chain_in_order() {
        use crate::routing::node::{FxKind, FxSpec};
        // Source → EQ → Gain → Output. The route must carry both FX in signal order.
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let mut eq = Node::new(NodeType::Fx, "EQ", "");
        eq.fx = Some(FxSpec {
            kind: FxKind::Eq,
            bypassed: false,
            gain_db: 6.0,
            open_db: 0.0,
            close_db: 0.0,
            freq: 1000.0,
            q: 1.0,
            eq_bands: [0.0; 5],
            threshold_db: 0.0,
            ceiling_db: 0.0,
            ratio: 0.0,
        });
        let mut gain = Node::new(NodeType::Fx, "Gain", "");
        gain.fx = Some(FxSpec {
            kind: FxKind::Gain,
            bypassed: false,
            gain_db: -3.0,
            open_db: 0.0,
            close_db: 0.0,
            freq: 0.0,
            q: 0.0,
            eq_bands: [0.0; 5],
            threshold_db: 0.0,
            ceiling_db: 0.0,
            ratio: 0.0,
        });
        let out = make_node(NodeType::Output, "out-dev");
        let (sid, eid, gid, oid) =
            (src.id.clone(), eq.id.clone(), gain.id.clone(), out.id.clone());
        g.add_node(src);
        g.add_node(eq);
        g.add_node(gain);
        g.add_node(out);
        g.add_route(Route::new(sid, eid.clone())).unwrap();
        g.add_route(Route::new(eid, gid.clone())).unwrap();
        g.add_route(Route::new(gid, oid)).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 1);
        let fx = &routes[0].fx_chain;
        assert_eq!(fx.len(), 2, "both FX collected");
        assert_eq!(fx[0].spec.kind, FxKind::Eq, "EQ first (upstream)");
        assert_eq!(fx[1].spec.kind, FxKind::Gain, "Gain second (downstream)");
    }

    #[test]
    fn resolve_splitter_fanout() {
        let mut g = Graph::new();
        let src = make_node(NodeType::Source, "src-dev");
        let spl = make_node(NodeType::Splitter, "");
        let out1 = make_node(NodeType::Output, "out-1");
        let out2 = make_node(NodeType::Output, "out-2");

        let sid = src.id.clone();
        let spid = spl.id.clone();
        let o1 = out1.id.clone();
        let o2 = out2.id.clone();

        g.add_node(src);
        g.add_node(spl);
        g.add_node(out1);
        g.add_node(out2);
        g.add_route(Route::new(sid, spid.clone())).unwrap();
        g.add_route(Route::new(spid.clone(), o1)).unwrap();
        g.add_route(Route::new(spid, o2)).unwrap();

        let routes = g.resolve_device_routes();
        assert_eq!(routes.len(), 2);
    }
}
