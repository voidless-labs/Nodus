import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  applyRoutingGraph,
  setRouteMute as bridgeSetRouteMute,
  setRoutePan as bridgeSetRoutePan,
  setRouteVolume as bridgeSetRouteVolume,
  type AudioDevice,
  type AudioProcess,
} from './bridge';
import { buildRoutingGraph } from './routingGraph';
import { EMPTY_SCENE, buildPreset, type PresetId, type Scene } from './scenes';
import type { EdgeModel, HubModel, NodeModel } from './ui/nodes/types';

/**
 * useScene — the canvas scene store and its engine sync (R18).
 *
 * Holds the single mutable scene (nodes / hubs / edges) and exposes the
 * mutations the UI needs: create a node from a real device/process, toggle
 * mute/solo, set node and edge volume/pan, add/remove edges, load a preset.
 *
 * Engine sync mirrors the legacy app.jsx:
 * - structural changes (nodes/edges, mute, solo) → debounced full re-apply of
 *   the RoutingGraph (the engine settles WASAPI over ~80ms, so we don't do it
 *   per keystroke);
 * - volume / pan changes → fast per-route setters (no engine restart).
 * All engine calls are no-ops outside Tauri, so the browser preview just edits
 * the scene.
 */

const DEBOUNCE_MS = 120;

/** Place a new node in a free-ish slot: sources on the left, targets on the right. */
function placeFor(scene: Scene, side: 'left' | 'right'): { x: number; y: number } {
  const x = side === 'left' ? 40 : 760;
  const sameSide = scene.nodes.filter((n) =>
    side === 'left' ? n.hasOutput && !n.hasInput : n.hasInput && !n.hasOutput,
  ).length;
  return { x, y: 96 + sameSide * 176 };
}

let _seq = 0;
const nid = (prefix: string) => `${prefix}-${Date.now().toString(36)}-${(_seq++).toString(36)}`;

type Pos = { x: number; y: number };

function deviceNode(d: AudioDevice, scene: Scene, pos?: Pos): NodeModel {
  const isVirtual = d.device_type === 'virtual';
  const isInput = d.device_type === 'input';
  const low = d.name.toLowerCase();
  const micSink = isVirtual && low.includes('mic');
  // input device = source (left); output/virtual = target (right).
  const side = isInput ? 'left' : 'right';
  return {
    id: nid('n'),
    kind: isInput ? 'source' : isVirtual ? 'virtual' : 'output',
    micSink: micSink || undefined,
    name: d.name.replace(/\s*\([^)]*\)\s*$/, '').trim() || d.name,
    subtitle: d.original_name ?? d.device_type,
    deviceId: d.id,
    level: 0,
    volume: 1,
    active: true,
    hasInput: !isInput,
    hasOutput: isInput,
    ...(pos ?? placeFor(scene, side)),
  };
}

function processNode(p: AudioProcess, scene: Scene, pos?: Pos): NodeModel {
  return {
    id: nid('n'),
    kind: 'source',
    name: p.display_name,
    subtitle: p.source_type === 'unknown' ? 'app' : `${p.source_type} · app`,
    avatar: p.display_name.charAt(0).toUpperCase(),
    icon: p.icon ?? undefined,
    exeName: p.exe_name,
    level: 0,
    volume: 1,
    active: true,
    hasInput: false,
    hasOutput: true,
    ...(pos ?? placeFor(scene, 'left')),
  };
}

/** A node/hub created from a BottomBar catalog type (R5). */
type CatalogSpec = {
  kind: NodeModel['kind'];
  name: string;
  subtitle: string;
  hasInput: boolean;
  hasOutput: boolean;
};
const CATALOG: Record<string, CatalogSpec> = {
  source: { kind: 'source', name: 'Source', subtitle: 'unbound', hasInput: false, hasOutput: true },
  output: { kind: 'output', name: 'Output', subtitle: 'unbound', hasInput: true, hasOutput: false },
  splitter: { kind: 'splitter', name: 'Splitter', subtitle: '1 → many', hasInput: true, hasOutput: true },
  virtual: { kind: 'virtual', name: 'Virtual', subtitle: 'Nodus device', hasInput: true, hasOutput: false },
  gate: { kind: 'fx', name: 'Noise Gate', subtitle: 'effect', hasInput: true, hasOutput: true },
  comp: { kind: 'fx', name: 'Compressor', subtitle: 'effect', hasInput: true, hasOutput: true },
  limiter: { kind: 'fx', name: 'Limiter', subtitle: 'effect', hasInput: true, hasOutput: true },
  eq: { kind: 'fx', name: 'EQ', subtitle: 'effect', hasInput: true, hasOutput: true },
  gain: { kind: 'fx', name: 'Gain', subtitle: 'effect', hasInput: true, hasOutput: true },
  duck: { kind: 'fx', name: 'Ducking', subtitle: 'effect', hasInput: true, hasOutput: true },
  trigger: { kind: 'logic', name: 'Push-to-Talk', subtitle: 'hold a key', hasInput: false, hasOutput: true },
};

/** Build a node (or a hub for `mixer`) of a catalog type at `pos`. */
function nodeFromType(typeId: string, pos: Pos): { node?: NodeModel; hub?: HubModel } {
  if (typeId === 'mixer') {
    return {
      hub: {
        id: nid('h'),
        name: 'Mixer',
        subtitle: 'routing engine',
        inputs: [
          { id: nid('in'), label: 'in 1', volume: 1 },
          { id: nid('in'), label: 'in 2', volume: 1 },
        ],
        level: 0,
        active: true,
        ...pos,
      },
    };
  }
  const spec = CATALOG[typeId];
  if (!spec) return {};
  return {
    node: {
      id: nid('n'),
      kind: spec.kind,
      name: spec.name,
      subtitle: spec.subtitle,
      level: 0,
      volume: 1,
      active: true,
      hasInput: spec.hasInput,
      hasOutput: spec.hasOutput,
      ...pos,
    },
  };
}

export interface SceneStore {
  scene: Scene;
  isEmpty: boolean;
  nodeCount: number;
  routeCount: number;
  /** All scenes (tabs) and the active one (R22 multi-scene). */
  scenes: { id: string; name: string }[];
  activeSceneId: string;
  switchScene: (id: string) => void;
  newScene: () => void;
  closeScene: (id: string) => void;
  renameScene: (id: string, name: string) => void;
  loadPreset: (id: PresetId) => void;
  replaceScene: (scene: Scene) => void;
  /** Add a node from a device/process; optional world position (drag-drop). */
  addDevice: (d: AudioDevice, pos?: { x: number; y: number }) => void;
  addProcess: (p: AudioProcess, pos?: { x: number; y: number }) => void;
  /** Create a node/hub of a catalog type (BottomBar drag) at a position (R5). */
  addNodeType: (typeId: string, pos: { x: number; y: number }) => void;
  removeNode: (id: string) => void;
  /** Remove several nodes/hubs (and their edges) at once — multi-select delete. */
  removeNodes: (ids: string[]) => void;
  /** Rename a node or hub (R20). */
  renameNode: (id: string, name: string) => void;
  /** Duplicate a node or hub (offset copy, no edges) (R20). */
  duplicateNode: (id: string) => void;
  /** Move a node or hub on the canvas (visual only — not sent to the engine). */
  moveNode: (id: string, x: number, y: number) => void;
  /** Move several nodes/hubs to absolute positions at once (group drag). */
  moveNodes: (updates: { id: string; x: number; y: number }[]) => void;
  setNodeVolume: (id: string, volume: number) => void;
  toggleNodeMute: (id: string) => void;
  /** Set mute on several nodes at once (Mute all / Unmute all). */
  setNodesMuted: (ids: string[], muted: boolean) => void;
  toggleNodeSolo: (id: string) => void;
  addEdge: (edge: EdgeModel) => void;
  /** Connect two nodes by dragging a wire (out port → in port). */
  connect: (from: string, to: string, toPort?: string) => void;
  removeEdge: (id: string) => void;
  setEdgeVolume: (id: string, volume: number) => void;
  setEdgeMute: (id: string, muted: boolean) => void;
  setEdgePan: (id: string, pan: number) => void;
  /** Add an input port to a hub / remove one (R24 dynamic ports). */
  addHubInput: (hubId: string) => void;
  removeHubInput: (hubId: string, inputId: string) => void;
  /** Connect a source to a hub's "ghost" port → new input + edge (auto-grow). */
  connectNewInput: (fromNode: string, hubId: string) => void;
  /** Push the current graph to the engine (called when the engine turns on). */
  applyNow: () => void;
}

interface SceneTab {
  id: string;
  name: string;
  data: Scene;
}

let _sid = 0;
const newSceneId = () => `scene-${Date.now().toString(36)}-${(_sid++).toString(36)}`;

export function useScene(live: boolean): SceneStore {
  // Multiple named scenes; one is active. Mutations target the active scene via
  // `setScene`, which keeps the single-scene signature so all existing mutators
  // work unchanged.
  const [tabs, setTabs] = useState<SceneTab[]>(() => [
    { id: 'scene-1', name: 'Scene 1', data: EMPTY_SCENE },
  ]);
  const [activeId, setActiveId] = useState('scene-1');
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  const active = tabs.find((t) => t.id === activeId) ?? tabs[0];
  const scene = active.data;

  const sceneRef = useRef(scene);
  sceneRef.current = scene;
  const liveRef = useRef(live);
  liveRef.current = live;
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const setScene = useCallback((updater: Scene | ((s: Scene) => Scene)) => {
    setTabs((ts) =>
      ts.map((t) =>
        t.id === activeIdRef.current
          ? { ...t, data: typeof updater === 'function' ? (updater as (s: Scene) => Scene)(t.data) : updater }
          : t,
      ),
    );
  }, []);

  const applyNow = useCallback(() => {
    void applyRoutingGraph(buildRoutingGraph(sceneRef.current)).catch((e) =>
      console.error('apply_routing_graph:', e),
    );
  }, []);

  // Debounced re-apply for structural changes while live.
  const applyLater = useCallback(() => {
    if (!liveRef.current) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(applyNow, DEBOUNCE_MS);
  }, [applyNow]);

  useEffect(
    () => () => {
      if (timer.current != null) clearTimeout(timer.current);
    },
    [],
  );

  // ── scene management (R22 multi-scene) ─────────────────────────────────
  const switchScene = useCallback(
    (id: string) => {
      setActiveId(id);
      applyLater(); // re-push the now-active scene to the engine if live
    },
    [applyLater],
  );
  const newScene = useCallback(() => {
    const id = newSceneId();
    setTabs((ts) => [...ts, { id, name: `Scene ${ts.length + 1}`, data: EMPTY_SCENE }]);
    setActiveId(id);
  }, []);
  const closeScene = useCallback(
    (id: string) => {
      setTabs((ts) => {
        if (ts.length <= 1) return ts;
        const idx = ts.findIndex((t) => t.id === id);
        const next = ts.filter((t) => t.id !== id);
        if (activeIdRef.current === id) setActiveId(next[Math.max(0, idx - 1)].id);
        return next;
      });
      applyLater();
    },
    [applyLater],
  );
  const renameScene = useCallback((id: string, name: string) => {
    setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, name } : t)));
  }, []);

  // ── scene mutators ─────────────────────────────────────────────────────
  const patchNodes = useCallback(
    (fn: (nodes: NodeModel[]) => NodeModel[]) =>
      setScene((s) => ({ ...s, nodes: fn(s.nodes) })),
    [],
  );
  const patchEdges = useCallback(
    (fn: (edges: EdgeModel[]) => EdgeModel[]) =>
      setScene((s) => ({ ...s, edges: fn(s.edges) })),
    [],
  );

  const loadPreset = useCallback((id: PresetId) => setScene(buildPreset(id)), []);
  const replaceScene = useCallback(
    (next: Scene) => {
      setScene(next);
      applyLater();
    },
    [applyLater],
  );

  const addDevice = useCallback(
    (d: AudioDevice, pos?: Pos) =>
      setScene((s) => ({ ...s, nodes: [...s.nodes, deviceNode(d, s, pos)] })),
    [],
  );
  const addProcess = useCallback(
    (p: AudioProcess, pos?: Pos) =>
      setScene((s) => ({ ...s, nodes: [...s.nodes, processNode(p, s, pos)] })),
    [],
  );

  // Create a node/hub from a BottomBar catalog type at a drop position (R5).
  const addNodeType = useCallback(
    (typeId: string, pos: Pos) => {
      const { node, hub } = nodeFromType(typeId, pos);
      if (!node && !hub) return;
      setScene((s) => ({
        ...s,
        nodes: node ? [...s.nodes, node] : s.nodes,
        hubs: hub ? [...s.hubs, hub] : s.hubs,
      }));
    },
    [],
  );

  const removeNode = useCallback(
    (id: string) => {
      setScene((s) => ({
        ...s,
        nodes: s.nodes.filter((n) => n.id !== id),
        hubs: s.hubs.filter((h) => h.id !== id),
        edges: s.edges.filter((e) => e.from !== id && e.to !== id),
      }));
      applyLater();
    },
    [applyLater],
  );

  const removeNodes = useCallback(
    (ids: string[]) => {
      if (!ids.length) return;
      const set = new Set(ids);
      setScene((s) => ({
        ...s,
        nodes: s.nodes.filter((n) => !set.has(n.id)),
        hubs: s.hubs.filter((h) => !set.has(h.id)),
        edges: s.edges.filter((e) => !set.has(e.from) && !set.has(e.to)),
      }));
      applyLater();
    },
    [applyLater],
  );

  const moveNode = useCallback(
    (id: string, x: number, y: number) =>
      setScene((s) => ({
        ...s,
        nodes: s.nodes.map((n) => (n.id === id ? { ...n, x, y } : n)),
        hubs: s.hubs.map((h) => (h.id === id ? { ...h, x, y } : h)),
      })),
    [],
  );

  const renameNode = useCallback((id: string, name: string) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    setScene((s) => ({
      ...s,
      nodes: s.nodes.map((n) => (n.id === id ? { ...n, name: trimmed } : n)),
      hubs: s.hubs.map((h) => (h.id === id ? { ...h, name: trimmed } : h)),
    }));
  }, []);

  // Duplicate a node/hub offset by a little, without its edges (R20).
  const duplicateNode = useCallback((id: string) => {
    setScene((s) => {
      const node = s.nodes.find((n) => n.id === id);
      if (node) {
        const copy: NodeModel = {
          ...node,
          id: nid('n'),
          name: `${node.name} copy`,
          x: node.x + 36,
          y: node.y + 36,
          selected: false,
        };
        return { ...s, nodes: [...s.nodes, copy] };
      }
      const hub = s.hubs.find((h) => h.id === id);
      if (hub) {
        const copy: HubModel = {
          ...hub,
          id: nid('h'),
          name: `${hub.name} copy`,
          x: hub.x + 36,
          y: hub.y + 36,
          selected: false,
          inputs: hub.inputs.map((inp) => ({ ...inp, id: nid('in') })),
        };
        return { ...s, hubs: [...s.hubs, copy] };
      }
      return s;
    });
  }, []);

  const moveNodes = useCallback((updates: { id: string; x: number; y: number }[]) => {
    if (!updates.length) return;
    const pos = new Map(updates.map((u) => [u.id, u]));
    setScene((s) => ({
      ...s,
      nodes: s.nodes.map((n) => (pos.has(n.id) ? { ...n, ...pos.get(n.id)! } : n)),
      hubs: s.hubs.map((h) => (pos.has(h.id) ? { ...h, ...pos.get(h.id)! } : h)),
    }));
  }, []);

  // Adjacent routes carrying this node's audio (source: outgoing, target: incoming).
  const adjacentEdges = useCallback((id: string): EdgeModel[] => {
    const node = sceneRef.current.nodes.find((n) => n.id === id);
    if (!node) return [];
    const outgoing = node.hasOutput !== false && node.hasInput === false;
    return sceneRef.current.edges.filter((e) => (outgoing ? e.from === id : e.to === id));
  }, []);

  // Node slider = master for this node: mirror it onto every adjacent route's trim.
  const setNodeVolume = useCallback(
    (id: string, volume: number) => {
      patchNodes((ns) => ns.map((n) => (n.id === id ? { ...n, volume } : n)));
      const adj = adjacentEdges(id);
      patchEdges((es) =>
        es.map((e) => (adj.some((a) => a.id === e.id) ? { ...e, volume } : e)),
      );
      if (liveRef.current) {
        adj.forEach((e) =>
          void bridgeSetRouteVolume(e.id, volume).catch((err) =>
            console.error('set_route_volume:', err),
          ),
        );
      }
    },
    [patchNodes, patchEdges, adjacentEdges],
  );

  const setNodesMuted = useCallback(
    (ids: string[], muted: boolean) => {
      if (!ids.length) return;
      const set = new Set(ids);
      patchNodes((ns) => ns.map((n) => (set.has(n.id) ? { ...n, muted } : n)));
      applyLater();
    },
    [patchNodes, applyLater],
  );

  const toggleNodeMute = useCallback(
    (id: string) => {
      patchNodes((ns) => ns.map((n) => (n.id === id ? { ...n, muted: !n.muted } : n)));
      applyLater(); // mute folds into effective route mute → re-apply
    },
    [patchNodes, applyLater],
  );

  const toggleNodeSolo = useCallback(
    (id: string) => {
      patchNodes((ns) => ns.map((n) => (n.id === id ? { ...n, solo: !n.solo } : n)));
      applyLater(); // solo changes effective mute of every other route
    },
    [patchNodes, applyLater],
  );

  const addEdge = useCallback(
    (edge: EdgeModel) => {
      patchEdges((es) => [...es, edge]);
      applyLater();
    },
    [patchEdges, applyLater],
  );

  const connect = useCallback(
    (from: string, to: string, toPort?: string) => {
      if (from === to) return;
      setScene((s) => {
        const dup = s.edges.some(
          (e) => e.from === from && e.to === to && (e.toPort ?? '') === (toPort ?? ''),
        );
        if (dup) return s;
        const edge: EdgeModel = { id: nid('e'), from, to, toPort, volume: 1, active: true };
        return { ...s, edges: [...s.edges, edge] };
      });
      applyLater();
    },
    [applyLater],
  );
  const removeEdge = useCallback(
    (id: string) => {
      patchEdges((es) => es.filter((e) => e.id !== id));
      applyLater();
    },
    [patchEdges, applyLater],
  );

  const setEdgeVolume = useCallback(
    (id: string, volume: number) => {
      patchEdges((es) => es.map((e) => (e.id === id ? { ...e, volume } : e)));
      if (liveRef.current)
        void bridgeSetRouteVolume(id, volume).catch((e) => console.error('set_route_volume:', e));
    },
    [patchEdges],
  );
  const setEdgeMute = useCallback(
    (id: string, muted: boolean) => {
      patchEdges((es) => es.map((e) => (e.id === id ? { ...e, muted } : e)));
      if (liveRef.current)
        void bridgeSetRouteMute(id, muted).catch((e) => console.error('set_route_mute:', e));
    },
    [patchEdges],
  );
  const setEdgePan = useCallback(
    (id: string, pan: number) => {
      patchEdges((es) => es.map((e) => (e.id === id ? { ...e, pan } : e)));
      if (liveRef.current)
        void bridgeSetRoutePan(id, pan).catch((e) => console.error('set_route_pan:', e));
    },
    [patchEdges],
  );

  // ── Dynamic hub inputs (R24) ───────────────────────────────────────────
  // Auto-grow: dragging a source onto a hub's trailing "ghost" port materialises
  // a new input AND connects to it in one step (a fresh ghost then renders below).
  const connectNewInput = useCallback(
    (fromNode: string, hubId: string) => {
      if (fromNode === hubId) return;
      const inputId = nid('in');
      const src = sceneRef.current.nodes.find((n) => n.id === fromNode);
      const label = (src?.name ?? 'input').toLowerCase();
      setScene((s) => ({
        ...s,
        hubs: s.hubs.map((h) =>
          h.id === hubId ? { ...h, inputs: [...h.inputs, { id: inputId, label, volume: 1 }] } : h,
        ),
        edges: [
          ...s.edges,
          { id: nid('e'), from: fromNode, to: hubId, toPort: inputId, volume: 1, active: true },
        ],
      }));
      applyLater();
    },
    [applyLater],
  );

  const addHubInput = useCallback((hubId: string) => {
    setScene((s) => ({
      ...s,
      hubs: s.hubs.map((h) =>
        h.id === hubId
          ? { ...h, inputs: [...h.inputs, { id: nid('in'), label: `in ${h.inputs.length + 1}`, volume: 1 }] }
          : h,
      ),
    }));
  }, []);
  const removeHubInput = useCallback(
    (hubId: string, inputId: string) => {
      setScene((s) => ({
        ...s,
        hubs: s.hubs.map((h) =>
          h.id === hubId ? { ...h, inputs: h.inputs.filter((i) => i.id !== inputId) } : h,
        ),
        edges: s.edges.filter((e) => !(e.to === hubId && (e.toPort ?? '') === inputId)),
      }));
      applyLater(); // removing an input drops its route
    },
    [applyLater],
  );

  const isEmpty = scene.nodes.length === 0 && scene.hubs.length === 0;
  const nodeCount = scene.nodes.length + scene.hubs.length;

  return useMemo(
    () => ({
      scene,
      isEmpty,
      nodeCount,
      routeCount: scene.edges.length,
      scenes: tabs.map((t) => ({ id: t.id, name: t.name })),
      activeSceneId: activeId,
      switchScene,
      newScene,
      closeScene,
      renameScene,
      loadPreset,
      replaceScene,
      addDevice,
      addProcess,
      addNodeType,
      removeNode,
      removeNodes,
      renameNode,
      duplicateNode,
      moveNode,
      moveNodes,
      setNodeVolume,
      toggleNodeMute,
      setNodesMuted,
      toggleNodeSolo,
      addEdge,
      connect,
      removeEdge,
      setEdgeVolume,
      setEdgeMute,
      setEdgePan,
      addHubInput,
      removeHubInput,
      connectNewInput,
      applyNow,
    }),
    [
      scene,
      isEmpty,
      nodeCount,
      tabs,
      activeId,
      switchScene,
      newScene,
      closeScene,
      renameScene,
      loadPreset,
      replaceScene,
      addDevice,
      addProcess,
      addNodeType,
      removeNode,
      removeNodes,
      renameNode,
      duplicateNode,
      moveNode,
      moveNodes,
      setNodeVolume,
      toggleNodeMute,
      setNodesMuted,
      toggleNodeSolo,
      addEdge,
      connect,
      removeEdge,
      setEdgeVolume,
      setEdgeMute,
      setEdgePan,
      addHubInput,
      removeHubInput,
      connectNewInput,
      applyNow,
    ],
  );
}
