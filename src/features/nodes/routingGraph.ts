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
import type { BackendNode, BackendNodeType, RoutingGraph } from '@/shared/bridge';
import type { EdgeModel, HubModel, NodeModel } from '@/features/nodes/types';
import type { Scene } from '@/features/nodes/scenes';

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

/** Minimal graph shape solo needs — satisfied by a full Scene and by the canvas's
 *  raw nodes/hubs/edges props, so the same helper serves the engine and the UI. */
type GraphLike = { nodes: NodeModel[]; hubs: HubModel[]; edges: EdgeModel[] };

/** Nodes that can trigger solo: any node EXCEPT splitters (solo there is redundant
 *  — one input fanned out) and logic (not audio). A node/hub with solo===true fires. */
function soloTriggers(scene: GraphLike): string[] {
  const ids: string[] = [];
  for (const n of scene.nodes) if (n.solo && n.kind !== 'logic') ids.push(n.id);
  for (const h of scene.hubs) if (h.solo && (h.role ?? 'mixer') !== 'splitter') ids.push(h.id);
  return ids;
}

/**
 * Solo sets for auditioning the chain THROUGH the soloed node(s). The soloed node
 * is the LISTENING POINT: you hear the signal exactly as it is there.
 *  - `applied` = the soloed node(s) + everything UPSTREAM (ancestors). These are
 *    already "printed" into the signal you hear, so their FX are applied.
 *  - `chain`   = `applied` + everything DOWNSTREAM (descendants) — needed so the
 *    signal still reaches an output. A route plays iff BOTH endpoints ∈ chain.
 * An FX node that is in `chain` but NOT in `applied` sits below the listening point,
 * so its effect is bypassed while solo is on ("skipped by solo"). Multiple solos
 * union their sets. Splitter/logic never trigger solo (but are traversed).
 * Same helper drives the engine mute AND the UI highlight/skip tags — never disagree.
 */
export function soloSets(scene: GraphLike): { chain: Set<string>; applied: Set<string> } {
  const triggers = soloTriggers(scene);
  const chain = new Set<string>();
  const applied = new Set<string>();
  if (triggers.length === 0) return { chain, applied };

  const push = (m: Map<string, string[]>, k: string, v: string) => {
    const a = m.get(k);
    if (a) a.push(v);
    else m.set(k, [v]);
  };
  const fwd = new Map<string, string[]>(); // from → [to]  (downstream)
  const rev = new Map<string, string[]>(); // to   → [from] (upstream)
  for (const e of scene.edges) {
    push(fwd, e.from, e.to);
    push(rev, e.to, e.from);
  }
  const walk = (start: string, adj: Map<string, string[]>, into: Set<string>) => {
    const stack = [start];
    const visited = new Set<string>();
    while (stack.length) {
      const cur = stack.pop()!;
      if (visited.has(cur)) continue;
      visited.add(cur);
      into.add(cur);
      for (const next of adj.get(cur) ?? []) if (!visited.has(next)) stack.push(next);
    }
  };
  for (const t of triggers) {
    walk(t, rev, applied); // ancestors + self → FX here are applied
    walk(t, fwd, chain); // descendants + self → routed onward
  }
  for (const a of applied) chain.add(a); // applied ⊆ chain
  return { chain, applied };
}

/** Just the audible-chain set (engine route mute). See soloSets. */
export function soloChainNodes(scene: GraphLike): Set<string> {
  return soloSets(scene).chain;
}

export function buildRoutingGraph(scene: Scene): RoutingGraph {
  const { nodes, hubs, edges } = scene;

  // Index nodes + hubs by id so edges can resolve their endpoints.
  const byId = new Map<string, NodeModel | HubModel>();
  nodes.forEach((n) => byId.set(n.id, n));
  hubs.forEach((h) => byId.set(h.id, h));

  // Solo = audition the chain(s) through the soloed node(s). Anything outside the
  // chain is muted. Same helper feeds the UI highlight (see soloChainNodes).
  const chain = soloChainNodes(scene);
  const anySolo = chain.size > 0;

  // ── Nodes ────────────────────────────────────────────────────────────────
  const backendNodes: BackendNode[] = [];
  const included = new Set<string>();

  for (const n of nodes) {
    const type = BACKEND_TYPE[n.kind];
    if (!type) continue; // logic/control nodes carry no audio
    backendNodes.push({
      id: n.id,
      node_type: type,
      // The engine flags the Nodus mic destination by matching this label
      // (is_nodus_virtual_mic_name → writes into the kernel mic ring). deviceNode
      // strips the "(Nodus …)" suffix from the display name, so a mic-sink node's
      // n.name is e.g. "Микрофон" and no longer matches → the route was silently
      // treated as a normal WASAPI render into a capture endpoint (no audio). Send
      // the canonical marker label for our mic sinks so detection is robust; the
      // visible node name (n.name) is unaffected. (mic regression fix)
      // virtualSource (our virtual OUTPUT used as a source) gets a canonical Nodus
      // marker too so is_nodus_virtual_name detects it after a rename → the engine
      // reads its render ring (VirtualCapture) by the device_id `nodus:<N>`. (t8)
      label: n.micSink
        ? 'Nodus Virtual Mic'
        : n.virtualSource
          ? 'Nodus Virtual Speaker'
          : n.name,
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
  // A node's OWN explicit mute (leaf nodes only; hubs have none).
  const nodeMuted = (id: string): boolean =>
    !!(byId.get(id) as { muted?: boolean } | undefined)?.muted;

  const routes = edges
    .filter((e: EdgeModel) => included.has(e.from) && included.has(e.to))
    .map((e: EdgeModel) => ({
      id: e.id,
      from_node: e.from,
      to_node: e.to,
      volume: e.volume ?? 1,
      // Muted by: the edge itself, either endpoint's explicit mute, or (when solo
      // is active) being outside the soloed chain.
      muted:
        e.muted ||
        nodeMuted(e.from) ||
        nodeMuted(e.to) ||
        (anySolo && (!chain.has(e.from) || !chain.has(e.to))),
      pan: e.pan ?? 0,
    }));

  return { nodes: backendNodes, routes };
}
