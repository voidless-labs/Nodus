/**
 * routingGraph.ts — turn a canvas Scene into the engine's RoutingGraph (R18).
 *
 * Ported from the proven logic in the legacy app.jsx (buildRoutingGraph):
 * - volume and pan are PER-ROUTE (they live on the edge), matching the Rust
 *   engine which is strictly per-route;
 * - a node's mute/solo folds into an *effective* mute on every adjacent route
 *   (a route is muted if either of its endpoints is effectively muted);
 * - solo: if any node is soloed, every non-soloed node is effectively muted.
 *
 * This file is pure (no React, no Tauri) so it can be unit-tested and reused by
 * both "apply on engine start" and the live re-apply path.
 */
import type { BackendNode, BackendNodeType, RoutingGraph } from './bridge';
import type { EdgeModel, HubModel, NodeModel } from './ui/nodes/types';
import type { Scene } from './scenes';

/** Map a UI node kind to the backend node type. `logic` is control-only → skipped. */
const BACKEND_TYPE: Record<string, BackendNodeType | null> = {
  source: 'source',
  output: 'output',
  virtual: 'virtual',
  hub: 'mixer',
  splitter: 'splitter',
  fx: 'mixer', // passthrough mixer (no DSP yet — MVP)
  logic: null, // control-only, not in the audio graph
};

/** A node is effectively muted by its own mute, or by another node's solo. */
function effectiveMuted(
  node: { muted?: boolean; solo?: boolean; kind?: string },
  anySolo: boolean,
): boolean {
  return !!(node.muted || (anySolo && !node.solo));
}

export function buildRoutingGraph(scene: Scene): RoutingGraph {
  const { nodes, hubs, edges } = scene;

  // Index nodes + hubs by id so edges can resolve their endpoints.
  const byId = new Map<string, NodeModel | HubModel>();
  nodes.forEach((n) => byId.set(n.id, n));
  hubs.forEach((h) => byId.set(h.id, h));

  // Only leaf nodes carry solo; hubs do not.
  const anySolo = nodes.some((n) => n.solo);

  // ── Nodes ────────────────────────────────────────────────────────────────
  const backendNodes: BackendNode[] = [];
  const included = new Set<string>();

  for (const n of nodes) {
    const type = BACKEND_TYPE[n.kind];
    if (!type) continue; // logic/control nodes carry no audio
    backendNodes.push({
      id: n.id,
      node_type: type,
      label: n.name,
      device_id: n.deviceId ?? '',
      exe_name: n.exeName ?? null,
    });
    included.add(n.id);
  }
  for (const h of hubs) {
    backendNodes.push({
      id: h.id,
      node_type: h.role === 'splitter' ? 'splitter' : 'mixer',
      label: h.name,
      device_id: '',
      exe_name: null,
    });
    included.add(h.id);
  }

  // ── Routes ───────────────────────────────────────────────────────────────
  const muted = (id: string): boolean => {
    const node = byId.get(id);
    if (!node) return false;
    // Hubs have no mute of their own; only leaf nodes carry mute/solo.
    return effectiveMuted(node as NodeModel, anySolo);
  };

  const routes = edges
    .filter((e: EdgeModel) => included.has(e.from) && included.has(e.to))
    .map((e: EdgeModel) => ({
      id: e.id,
      from_node: e.from,
      to_node: e.to,
      volume: e.volume ?? 1,
      muted: e.muted || muted(e.from) || muted(e.to),
      pan: e.pan ?? 0,
    }));

  return { nodes: backendNodes, routes };
}
