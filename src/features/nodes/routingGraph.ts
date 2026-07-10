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

/** Audio-source nodes (the "channels" solo isolates): app/device sources, plus
 *  our virtual OUTPUT used as a source. NOT outputs, hubs or mic-sinks. */
function isAudioSource(node: { kind?: string; virtualSource?: boolean }): boolean {
  return node.kind === 'source' || (node.kind === 'virtual' && !!node.virtualSource);
}

/** Effective mute of a node: its own mute, OR — when any source is soloed — a
 *  non-soloed SOURCE channel. Solo isolates sources: it must NOT mute hubs or
 *  outputs, otherwise the soloed source's own path THROUGH a mixer TO the output
 *  gets cut and you hear nothing (the whole point of solo is to hear that source). */
function effectiveMuted(
  node: { muted?: boolean; solo?: boolean; kind?: string; virtualSource?: boolean },
  anySolo: boolean,
): boolean {
  if (node.muted) return true;
  return anySolo && isAudioSource(node) && !node.solo;
}

export function buildRoutingGraph(scene: Scene): RoutingGraph {
  const { nodes, hubs, edges } = scene;

  // Index nodes + hubs by id so edges can resolve their endpoints.
  const byId = new Map<string, NodeModel | HubModel>();
  nodes.forEach((n) => byId.set(n.id, n));
  hubs.forEach((h) => byId.set(h.id, h));

  // Solo isolates source channels: only a soloed *source* engages solo (soloing
  // an output/hub must not silence the whole scene).
  const anySolo = nodes.some((n) => isAudioSource(n) && n.solo);

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
