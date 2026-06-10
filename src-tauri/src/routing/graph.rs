use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::node::{Node, NodeId, NodeType, Route, RouteId};

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

/// Active routes visible to the engine: flattened list of (from_device_id, to_device_id, volume, muted).
#[derive(Debug, Clone)]
pub struct ActiveRoute {
    pub route_id: RouteId,
    pub from_device_id: String,
    /// Set for app-capture sources (e.g. "spotify.exe") — engine resolves device via session mgr.
    pub exe_name: Option<String>,
    /// True when the source is a Nodus virtual endpoint — the engine reads it from
    /// the kernel driver's ring buffer (VirtualCapture), falling back to WASAPI loopback.
    pub from_is_virtual: bool,
    pub to_device_id: String,
    pub volume: f32,
    pub muted: bool,
    /// Stereo balance of the final edge into the output [-1.0 .. 1.0].
    pub pan: f32,
}

impl Graph {
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
                    crate::audio::virtual_device::is_nodus_virtual_name(&node.label);
                self.collect_device_routes(
                    &node.id,
                    &node.device_id,
                    node.exe_name.clone(),
                    is_virtual,
                    1.0,
                    false,
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
        out: &mut Vec<ActiveRoute>,
        depth: usize,
    ) {
        if depth > 16 {
            return;
        }
        for route in self.routes_from(current) {
            let volume = inherited_volume * route.volume;
            let muted = inherited_mute || route.muted;

            if let Some(dest) = self.nodes.get(&route.to_node) {
                match dest.node_type {
                    NodeType::Output | NodeType::Virtual => {
                        if !dest.device_id.is_empty() {
                            out.push(ActiveRoute {
                                route_id: route.id.clone(),
                                from_device_id: source_device.to_string(),
                                exe_name: source_exe.clone(),
                                from_is_virtual: source_is_virtual,
                                to_device_id: dest.device_id.clone(),
                                volume,
                                muted,
                                pan: route.pan,
                            });
                        }
                    }
                    NodeType::Splitter | NodeType::Mixer => {
                        self.collect_device_routes(
                            &dest.id,
                            source_device,
                            source_exe.clone(),
                            source_is_virtual,
                            volume,
                            muted,
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
