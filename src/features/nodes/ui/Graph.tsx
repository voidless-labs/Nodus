import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import './Graph.css';
import { NodeCard } from '@/features/nodes/ui/NodeCard';
import { FxNode } from '@/features/nodes/ui/FxNode';
import { HubNode } from '@/features/nodes/ui/HubNode';
import { EdgePopover } from '@/features/nodes/ui/EdgePopover';
import type { FxSpec } from '@/shared/bridge';
import type { EdgeModel, HubModel, LinkStatus, NodeModel } from '@/features/nodes/types';
import { KIND_COLOR_VAR } from '@/features/nodes/types';
import { LINK_OFFLINE, LINK_ONLINE, LINK_RECONNECTING } from '@/shared/bridge';
import { soloSets } from '@/features/nodes/routingGraph';
import type { View } from '@/shared/hooks/useView';

/** Live connection status of a source/output node, from the engine's live data.
 *  App sources → running process? · device/output nodes → the render link state
 *  when the engine is driving it, else simple device presence. Hubs/fx have none. */
const EMPTY_SET: Set<string> = new Set();

function nodeStatus(
  n: NodeModel,
  links: Record<string, number>,
  presentDevices: Set<string>,
  runningApps: Set<string>,
): LinkStatus | undefined {
  if (n.kind !== 'source' && n.kind !== 'output' && n.kind !== 'virtual') return undefined;
  if (n.exeName) {
    return runningApps.has(n.exeName.toLowerCase()) ? 'online' : 'offline';
  }
  if (n.deviceId) {
    const code = links[n.deviceId];
    if (code === LINK_RECONNECTING) return 'reconnecting';
    if (code === LINK_OFFLINE) return 'offline';
    if (code === LINK_ONLINE) return 'online';
    // No live render link (engine idle, or an input device) → fall back to presence.
    return presentDevices.has(n.deviceId) ? 'online' : 'offline';
  }
  return undefined;
}

/**
 * Graph — the node cards plus the wires between them (R8 + R5 hub + R21 pan/zoom).
 *
 * Nodes and wires live in a transformed "world" layer (translate + scale = the
 * view). Port endpoints are measured from the live DOM and converted to WORLD
 * coordinates, which are invariant to pan/zoom — so the wire SVG (also in the
 * world layer) stays attached without re-measuring on every pan. The popover is
 * rendered outside the world layer and positioned by projecting world→screen, so
 * it never scales. Measurement runs once per layout change (weak-CPU budget).
 */
type PortMap = Record<string, { x: number; y: number }>;

const ZMIN = 0.3;
const ZMAX = 2.2;
const clampZoom = (z: number) => Math.max(ZMIN, Math.min(ZMAX, z));

function key(node: string, side: string, port = '') {
  return `${node}:${side}:${port}`;
}

function edgePath(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

/** none = no search; match = name contains the query; dim = search active, no match. */
export type SearchState = 'match' | 'dim' | undefined;

function searchFor(name: string, q: string): SearchState {
  if (!q.trim()) return undefined;
  return name.toLowerCase().includes(q.trim().toLowerCase()) ? 'match' : 'dim';
}

export function Graph({
  nodes,
  edges,
  hubs = [],
  search = '',
  levels = {},
  fxLevels = {},
  hubLevels = {},
  links = {},
  presentDevices,
  runningApps,
  view,
  setView,
  selection,
  setSelection,
  onNodeVolume,
  onNodeMute,
  onNodesMove,
  onNodeSolo,
  onNodeDuplicate,
  onNodeDelete,
  onNodeRename,
  onConnect,
  onEdgeVolume,
  onEdgeMute,
  onEdgePan,
  onFxParams,
  onFxAdvanced,
  onRemoveEdge,
  onRemoveHubInput,
  onHubInputVolume,
  onConnectNewInput,
  onConnectNewOutput,
  onConnectNewBoth,
  pinned,
  onPin,
}: {
  nodes: NodeModel[];
  edges: EdgeModel[];
  hubs?: HubModel[];
  search?: string;
  /** Live per-source levels from the engine (keyed by device id / exe name). */
  levels?: Record<string, number>;
  /** Live FX telemetry per node id: decision level, gain reduction, engine state. */
  fxLevels?: Record<
    string,
    { reduction_db: number; input_level: number; active: boolean; spectrum?: number[] }
  >;
  /** Signal at each hub row, keyed by that row's EDGE id (engine, t18 6b). */
  hubLevels?: Record<string, number>;
  /** Live per-output-device link health, keyed by device id (LINK_* code). */
  links?: Record<string, number>;
  /** Ids of devices currently present (enumerated) — for the node status dot. */
  presentDevices?: Set<string>;
  /** Lowercased exe names of currently-running audio apps — for the status dot. */
  runningApps?: Set<string>;
  /** Canvas pan/zoom transform (R21). */
  view: View;
  setView: React.Dispatch<React.SetStateAction<View>>;
  /** Selected node/hub ids (R23 multi-select). */
  selection: Set<string>;
  setSelection: React.Dispatch<React.SetStateAction<Set<string>>>;
  onNodeVolume?: (id: string, volume: number) => void;
  onNodeMute?: (id: string) => void;
  /** Move one or more nodes/hubs to absolute positions (single or group drag). */
  onNodesMove?: (updates: { id: string; x: number; y: number }[]) => void;
  /** Single-node actions (R20 toolbar). */
  onNodeSolo?: (id: string) => void;
  onNodeDuplicate?: (id: string) => void;
  onNodeDelete?: (id: string) => void;
  onNodeRename?: (id: string, name: string) => void;
  /** Drag-connect: a wire dragged from an output port onto an input port. */
  onConnect?: (from: string, to: string, toPort?: string, fromPort?: string) => void;
  /** Edge popover (R9): per-route volume / mute / balance / delete. */
  onEdgeVolume?: (id: string, volume: number) => void;
  onEdgeMute?: (id: string, muted: boolean) => void;
  onEdgePan?: (id: string, pan: number) => void;
  /** Live FX parameter change from the FX inspector (t18). */
  onFxParams?: (id: string, fx: FxSpec) => void;
  onFxAdvanced?: (id: string) => void;
  onRemoveEdge?: (id: string) => void;
  /** Dynamic hub ports (R24). */
  onRemoveHubInput?: (hubId: string, inputId: string) => void;
  /** Live hub-input slider → feeding route trim (R18). */
  onHubInputVolume?: (hubId: string, inputId: string, volume: number) => void;
  /** Drop a wire on a mixer ghost in-port → new input + connect (fromPort = splitter output). */
  onConnectNewInput?: (fromNode: string, hubId: string, fromPort?: string) => void;
  /** Drag from a splitter ghost out-port to a target → new output + connect. */
  onConnectNewOutput?: (splitterId: string, toNode: string, toPort?: string) => void;
  /** Splitter ghost-out dropped on a mixer ghost-in → new output + input + edge. */
  onConnectNewBoth?: (splitterId: string, mixerId: string) => void;
  /** Pinned node/hub ids + toggle (t13 quick-controls). */
  pinned?: Set<string>;
  onPin?: (id: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [ports, setPorts] = useState<PortMap>({});
  // Rubber-band wire while dragging from an output port (null = not dragging).
  const [drag, setDrag] = useState<{
    from: string;
    x1: number;
    y1: number;
    x2: number;
    y2: number;
  } | null>(null);
  // The output port a wire is being dragged from: node id, its port id ('' for a
  // node's single output / a splitter output id), and whether it's the splitter
  // ghost "+" (a request to make a new output).
  const dragFrom = useRef<{ node: string; port?: string; add?: boolean } | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<string | null>(null);
  const [panning, setPanning] = useState(false);
  // Rubber-band selection rectangle (screen-local px); null = not marqueeing.
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(
    null,
  );

  // Latest view in a ref so listeners/measurement read it without re-subscribing.
  const viewRef = useRef(view);
  viewRef.current = view;
  // Space held = pan modifier (replicates the old canvas).
  const spaceRef = useRef(false);
  // Latest selection + positions, read by the (closure-captured) drag handlers.
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const posRef = useRef<Record<string, { x: number; y: number }>>({});
  posRef.current = Object.fromEntries(
    [...nodes, ...hubs].map((n) => [n.id, { x: n.x, y: n.y }]),
  );

  /** Screen (client) coordinate → world coordinate (cancels pan + zoom). */
  const screenToWorld = (clientX: number, clientY: number) => {
    const base = ref.current?.getBoundingClientRect();
    const v = viewRef.current;
    return {
      x: (clientX - (base?.left ?? 0) - v.x) / v.zoom,
      y: (clientY - (base?.top ?? 0) - v.y) / v.zoom,
    };
  };

  // Measure each port's centre in WORLD coords (invariant to pan/zoom).
  useLayoutEffect(() => {
    const measure = () => {
      const container = ref.current;
      if (!container) return;
      const map: PortMap = {};
      container.querySelectorAll<HTMLElement>('.node-port').forEach((p) => {
        const r = p.getBoundingClientRect();
        const id = p.dataset.node;
        const side = p.dataset.side;
        if (id && side) {
          map[key(id, side, p.dataset.port ?? '')] = screenToWorld(
            r.left + r.width / 2,
            r.top + r.height / 2,
          );
        }
      });
      setPorts(map);
    };
    measure();
    window.addEventListener('resize', measure);
    // Re-measure once webfonts settle — late font load shifts node heights and
    // would otherwise leave wires detached until the first resize.
    if (document.fonts?.ready) document.fonts.ready.then(measure).catch(() => {});
    return () => window.removeEventListener('resize', measure);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, hubs]);

  // Space key → pan modifier.
  useEffect(() => {
    const kd = (e: KeyboardEvent) => {
      if (e.code !== 'Space') return;
      const tag = (e.target as HTMLElement)?.tagName?.toLowerCase();
      if (tag !== 'input' && tag !== 'textarea') spaceRef.current = true;
    };
    const ku = (e: KeyboardEvent) => {
      if (e.code === 'Space') spaceRef.current = false;
    };
    window.addEventListener('keydown', kd);
    window.addEventListener('keyup', ku);
    return () => {
      window.removeEventListener('keydown', kd);
      window.removeEventListener('keyup', ku);
    };
  }, []);

  // Wheel = zoom toward the cursor (native non-passive listener so we can preventDefault).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const base = el.getBoundingClientRect();
      const fx = e.clientX - base.left;
      const fy = e.clientY - base.top;
      const factor = Math.exp(-e.deltaY * 0.0015);
      setView((v) => {
        const zoom = clampZoom(v.zoom * factor);
        const k = zoom / v.zoom;
        return { zoom, x: fx - (fx - v.x) * k, y: fy - (fy - v.y) * k };
      });
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [setView]);

  const startPan = (e: React.MouseEvent) => {
    e.preventDefault();
    setPanning(true);
    const sx = e.clientX;
    const sy = e.clientY;
    const start = viewRef.current;
    const move = (ev: MouseEvent) =>
      setView((v) => ({ ...v, x: start.x + (ev.clientX - sx), y: start.y + (ev.clientY - sy) }));
    const up = () => {
      setPanning(false);
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  // Drag a node by its body — moves the whole selection if the node is in it,
  // otherwise just that node (and selects only it). Deltas are divided by zoom.
  const startNodeDrag = (e: React.MouseEvent, nodeId: string) => {
    e.preventDefault();
    const sel = selectionRef.current;
    const movingIds = sel.has(nodeId) && sel.size > 1 ? [...sel] : [nodeId];
    if (!(sel.has(nodeId) && sel.size > 1)) setSelection(new Set([nodeId]));
    const snap = posRef.current;
    const startPos = new Map(movingIds.map((id) => [id, { ...snap[id] }]));
    const sx = e.clientX;
    const sy = e.clientY;
    const move = (ev: MouseEvent) => {
      const z = viewRef.current.zoom;
      const dx = (ev.clientX - sx) / z;
      const dy = (ev.clientY - sy) / z;
      onNodesMove?.(
        movingIds
          .filter((id) => startPos.has(id))
          .map((id) => ({ id, x: startPos.get(id)!.x + dx, y: startPos.get(id)!.y + dy })),
      );
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  // Rubber-band select on an empty-surface drag. Shift adds to the selection.
  const startMarquee = (e: React.MouseEvent) => {
    const base = ref.current?.getBoundingClientRect();
    if (!base) return;
    const x0 = e.clientX - base.left;
    const y0 = e.clientY - base.top;
    const prev = e.shiftKey ? new Set(selectionRef.current) : new Set<string>();
    setMarquee({ x0, y0, x1: x0, y1: y0 });
    const move = (ev: MouseEvent) =>
      setMarquee((m) => (m ? { ...m, x1: ev.clientX - base.left, y1: ev.clientY - base.top } : m));
    const up = (ev: MouseEvent) => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      setMarquee(null);
      const x1 = ev.clientX - base.left;
      const y1 = ev.clientY - base.top;
      const minX = Math.min(x0, x1);
      const maxX = Math.max(x0, x1);
      const minY = Math.min(y0, y1);
      const maxY = Math.max(y0, y1);
      // A click (no drag) clears the selection.
      if (maxX - minX < 4 && maxY - minY < 4) {
        if (!e.shiftKey) setSelection(new Set());
        return;
      }
      const ids = new Set(prev);
      ref.current?.querySelectorAll<HTMLElement>('.node[data-node-id]').forEach((el) => {
        const b = el.getBoundingClientRect();
        const nx0 = b.left - base.left;
        const ny0 = b.top - base.top;
        const nx1 = b.right - base.left;
        const ny1 = b.bottom - base.top;
        if (nx0 < maxX && nx1 > minX && ny0 < maxY && ny1 > minY && el.dataset.nodeId) {
          ids.add(el.dataset.nodeId);
        }
      });
      setSelection(ids);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  // Press on the graph surface: pan, drag-connect, drag a node, marquee-select, or
  // dismiss an open edge popover.
  const onGraphMouseDown = (e: React.MouseEvent) => {
    const target = e.target as HTMLElement;
    const onChrome =
      target.closest('.node') || target.closest('.edge-hit') || target.closest('.edge-popover');
    if (e.button === 1 || (e.button === 0 && spaceRef.current && !onChrome)) {
      startPan(e);
      return;
    }
    if (e.button !== 0) return;

    // Output port (incl. the splitter ghost "+") → start a wire.
    const port = target.closest<HTMLElement>('.node-port');
    if (port && port.dataset.side === 'out' && port.dataset.node) {
      e.preventDefault();
      const r = port.getBoundingClientRect();
      const start = screenToWorld(r.left + r.width / 2, r.top + r.height / 2);
      dragFrom.current = {
        node: port.dataset.node,
        port: port.dataset.port || '',
        add: !!port.dataset.add,
      };
      setDrag({ from: port.dataset.node, x1: start.x, y1: start.y, x2: start.x, y2: start.y });
      return;
    }

    // Node body (not an interactive control / port) → drag the node / group.
    const nodeEl = target.closest<HTMLElement>('.node[data-node-id]');
    if (
      nodeEl?.dataset.nodeId &&
      !target.closest('.node-port') &&
      !target.closest('.vslider') &&
      !target.closest('button') &&
      !target.closest('input')
    ) {
      startNodeDrag(e, nodeEl.dataset.nodeId);
      return;
    }

    // Empty surface → close popover and start a marquee selection. Presses that
    // landed inside a node (a control/port that didn't match above) do nothing.
    if (
      !target.closest('.node') &&
      !target.closest('.edge-popover') &&
      !target.closest('.edge-hit')
    ) {
      setSelectedEdge(null);
      startMarquee(e);
    }
  };

  // Close the popover with Escape.
  useEffect(() => {
    if (!selectedEdge) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setSelectedEdge(null);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selectedEdge]);

  // Drop the popover if its wire disappears (e.g. an endpoint node was removed).
  useEffect(() => {
    if (selectedEdge && !edges.some((e) => e.id === selectedEdge)) setSelectedEdge(null);
  }, [edges, selectedEdge]);

  // Drag-connect: follow the cursor (world coords); on release, connect if over an
  // input port. Listeners attach ONCE per drag (boolean gate) for a reliable mouseup.
  const dragging = drag !== null;
  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const p = screenToWorld(e.clientX, e.clientY);
      setDrag((d) => (d ? { ...d, x2: p.x, y2: p.y } : d));
    };
    const onUp = (e: MouseEvent) => {
      const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
      const port = el?.closest<HTMLElement>('.node-port');
      const from = dragFrom.current;
      if (from && port && port.dataset.side === 'in' && port.dataset.node) {
        const toNode = port.dataset.node;
        const toAdd = !!port.dataset.add; // mixer ghost in-port
        const toPort = port.dataset.port || undefined;
        if (from.add && toAdd) onConnectNewBoth?.(from.node, toNode); // splitter+ → mixer+
        else if (toAdd) onConnectNewInput?.(from.node, toNode, from.port); // → mixer +
        else if (from.add) onConnectNewOutput?.(from.node, toNode, toPort); // splitter + →
        else onConnect?.(from.node, toNode, toPort, from.port); // plain wire
      }
      dragFrom.current = null;
      setDrag(null);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging, onConnect, onConnectNewInput, onConnectNewOutput, onConnectNewBoth]);

  const worldTransform = `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`;
  // The lone selected node gets the action toolbar (R20). 2+ → SelectionBar.
  const soleSelected = selection.size === 1 ? selection.values().next().value : null;

  // Solo highlight: same sets the engine mutes/bypasses by, so UI can't disagree.
  //  - chain: audible path (lights up); outside ⇒ dimmed.
  //  - applied: chain nodes at/above the listening point → their FX are applied;
  //    an FX in chain but NOT applied is bypassed → "skipped by solo" tag.
  const { chain: soloChain, applied: soloApplied } = useMemo(
    () => soloSets({ nodes, hubs, edges }),
    [nodes, hubs, edges],
  );
  const anySolo = soloChain.size > 0;

  // Per-port route colouring (t28): a node's OUT is "connected" if it feeds any
  // edge; its IN shows the SOURCE node's accent (first incoming edge). Sources may
  // be nodes or hubs → one id→accent-var map covers both.
  const portConn = useMemo(() => {
    const kindVar: Record<string, string> = {};
    nodes.forEach((n) => (kindVar[n.id] = KIND_COLOR_VAR[n.kind]));
    hubs.forEach((h) => (kindVar[h.id] = KIND_COLOR_VAR.hub));
    const inFrom: Record<string, string> = {};
    const outHas = new Set<string>();
    // Endpoints of at least one edge — the nodes that actually take part in a
    // routing chain. Needed for the live meter: see the note at its use site.
    const wired = new Set<string>();
    // Hubs are wired per PORT, not per node: a mixer row and a splitter row are
    // each their own port, so their accents must be resolved per port. Keyed
    // `${nodeId}:${portId}`, where '' is the single fixed port. (t28 ports)
    const inFromPort: Record<string, string> = {};
    const outHasPort = new Set<string>();
    for (const e of edges) {
      if (!(e.to in inFrom)) inFrom[e.to] = e.from;
      outHas.add(e.from);
      wired.add(e.from);
      wired.add(e.to);
      const inKey = `${e.to}:${e.toPort ?? ''}`;
      if (!(inKey in inFromPort)) inFromPort[inKey] = e.from;
      outHasPort.add(`${e.from}:${e.fromPort ?? ''}`);
    }
    return { kindVar, inFrom, outHas, wired, inFromPort, outHasPort };
  }, [nodes, hubs, edges]);
  // Hub row signal, re-keyed from EDGE (how the engine reports it) to PORT (how a
  // row is identified on screen): a mixer row owns the edge landing on its in-port,
  // a splitter row the edge leaving its out-port. Nothing is computed here — the
  // level is the engine's, this only says which row it belongs to. (t18 6b)
  const hubRowLevels = useMemo(() => {
    const byHub: Record<string, Record<string, number>> = {};
    for (const e of edges) {
      const lvl = hubLevels[e.id];
      if (lvl == null) continue;
      if (e.toPort) (byHub[e.to] ??= {})[e.toPort] = lvl;
      if (e.fromPort) (byHub[e.from] ??= {})[e.fromPort] = lvl;
    }
    return byHub;
  }, [edges, hubLevels]);
  // 'on' = part of the audible chain, 'off' = dimmed (muted by solo), undefined = no solo.
  const chainState = (id: string): 'on' | 'off' | undefined =>
    anySolo ? (soloChain.has(id) ? 'on' : 'off') : undefined;
  // An FX node downstream of the listening point: routed through, but its effect is
  // bypassed while solo is active (real DSP bypass lands with t18; tag informs now).
  const isSoloSkipped = (n: NodeModel): boolean =>
    anySolo && n.kind === 'fx' && soloChain.has(n.id) && !soloApplied.has(n.id);

  // Selected wire midpoint, projected world→screen for the (unscaled) popover.
  const selected = selectedEdge ? edges.find((x) => x.id === selectedEdge) : undefined;
  const selA = selected && ports[key(selected.from, 'out', selected.fromPort ?? '')];
  const selB = selected && ports[key(selected.to, 'in', selected.toPort ?? '')];

  return (
    <div className={`graph ${panning ? 'is-panning' : ''}`} ref={ref} onMouseDown={onGraphMouseDown}>
      <div className="graph-world" style={{ transform: worldTransform }}>
        <svg className="edge-layer" aria-hidden>
          {edges.map((e) => {
            const a = ports[key(e.from, 'out', e.fromPort ?? '')];
            const b = ports[key(e.to, 'in', e.toPort ?? '')];
            if (!a || !b) return null;
            // An edge is in the solo chain only if BOTH endpoints are (matches the
            // engine's route mute). Otherwise it's dimmed while any solo is active.
            const edgeSolo = anySolo
              ? soloChain.has(e.from) && soloChain.has(e.to)
                ? 'is-solo-on'
                : 'is-solo-off'
              : '';
            const cls = [
              'edge',
              e.active ? 'is-active' : '',
              e.muted ? 'is-muted' : '',
              e.id === selectedEdge ? 'is-selected' : '',
              edgeSolo,
            ]
              .filter(Boolean)
              .join(' ');
            const d = edgePath(a.x, a.y, b.x, b.y);
            return (
              <g key={e.id}>
                <path className={cls} d={d} />
                <path
                  className="edge-hit"
                  d={d}
                  fill="none"
                  stroke="transparent"
                  strokeWidth={14}
                  style={{ pointerEvents: 'stroke', cursor: 'pointer' }}
                  onClick={() => setSelectedEdge(e.id)}
                />
              </g>
            );
          })}
          {drag && (
            <path className="edge is-active edge--dragging" d={edgePath(drag.x1, drag.y1, drag.x2, drag.y2)} />
          )}
        </svg>

        {hubs.map((h) => {
          // A hub port's colour follows what is actually wired to it: IN takes the
          // accent of the feeding node — which may be a source, an FX, a virtual
          // device or another hub, so it cannot be the fixed "source" colour the
          // CSS used to assume — and OUT is the hub's own accent, muted while
          // nothing is wired. Resolved per port id; '' is the single fixed port.
          const inSourceColorVars: Record<string, string> = {};
          const outConnectedPorts: Record<string, boolean> = {};
          const resolvePort = (portId: string) => {
            const from = portConn.inFromPort[`${h.id}:${portId}`];
            if (from && portConn.kindVar[from]) inSourceColorVars[portId] = portConn.kindVar[from];
            outConnectedPorts[portId] = portConn.outHasPort.has(`${h.id}:${portId}`);
          };
          h.inputs.forEach((p) => resolvePort(p.id));
          resolvePort(''); // the single fixed port (mixer mix-out / splitter in)
          return (
            <HubNode
              key={h.id}
              hub={selection.has(h.id) ? { ...h, selected: true } : h}
              search={searchFor(h.name, search)}
              actions={h.id === soleSelected}
              chainState={chainState(h.id)}
              onRemoveInput={onRemoveHubInput}
              onInputVolume={onHubInputVolume}
              onSolo={onNodeSolo}
              onDuplicate={onNodeDuplicate}
              onDelete={onNodeDelete}
              onRename={onNodeRename}
              onPin={onPin}
              pinned={pinned?.has(h.id)}
              inSourceColorVars={inSourceColorVars}
              outConnectedPorts={outConnectedPorts}
              rowLevels={hubRowLevels[h.id]}
            />
          );
        })}
        {nodes.map((n) => {
          // Live meter: the engine reports levels per device id / exe name, never
          // per node — so a second card for the same app or device would mirror the
          // meter of the one that is actually routed. Only a node wired into the
          // graph can carry audio, so only a wired node shows a live level.
          const live = portConn.wired.has(n.id)
            ? (n.deviceId && levels[n.deviceId]) ?? (n.exeName && levels[n.exeName])
            : undefined;
          // Live connection status shown persistently on the node (t19).
          const status = nodeStatus(n, links, presentDevices ?? EMPTY_SET, runningApps ?? EMPTY_SET);
          const node = {
            ...n,
            ...(typeof live === 'number' ? { level: live } : null),
            selected: n.selected || selection.has(n.id),
          };
          if (n.kind === 'fx') {
            return (
              <FxNode
                key={n.id}
                node={node}
                status={status}
                search={searchFor(n.name, search)}
                actions={n.id === soleSelected}
                chainState={chainState(n.id)}
                soloSkipped={isSoloSkipped(n)}
                onSolo={onNodeSolo}
                onDuplicate={onNodeDuplicate}
                onDelete={onNodeDelete}
                onRename={onNodeRename}
                onPin={onPin}
                pinned={pinned?.has(n.id)}
                onFxParams={onFxParams}
                onAdvanced={onFxAdvanced}
                outConnected={portConn.outHas.has(n.id)}
                inSourceColorVar={
                  portConn.inFrom[n.id] ? portConn.kindVar[portConn.inFrom[n.id]] : undefined
                }
                reductionDb={fxLevels[n.id]?.reduction_db ?? 0}
                inputLevel={fxLevels[n.id]?.input_level}
                active={fxLevels[n.id]?.active}
                spectrum={fxLevels[n.id]?.spectrum}
              />
            );
          }
          return (
            <NodeCard
              key={n.id}
              node={node}
              status={status}
              search={searchFor(n.name, search)}
              actions={n.id === soleSelected}
              chainState={chainState(n.id)}
              soloSkipped={isSoloSkipped(n)}
              onVolume={onNodeVolume}
              onMute={onNodeMute}
              onSolo={
                // Solo auditions the chain through a node — meaningful on every
                // audio node (source/output/virtual/fx); logic carries no audio.
                n.kind !== 'logic' ? onNodeSolo : undefined
              }
              onDuplicate={onNodeDuplicate}
              onDelete={onNodeDelete}
              onRename={onNodeRename}
              onPin={onPin}
              pinned={pinned?.has(n.id)}
              outConnected={portConn.outHas.has(n.id)}
              inSourceColorVar={
                portConn.inFrom[n.id] ? portConn.kindVar[portConn.inFrom[n.id]] : undefined
              }
            />
          );
        })}
      </div>

      {marquee && (
        <div
          className="marquee"
          style={{
            left: Math.min(marquee.x0, marquee.x1),
            top: Math.min(marquee.y0, marquee.y1),
            width: Math.abs(marquee.x1 - marquee.x0),
            height: Math.abs(marquee.y1 - marquee.y0),
          }}
        />
      )}

      {selected && selA && selB && (
        <EdgePopover
          edge={selected}
          x={view.x + ((selA.x + selB.x) / 2) * view.zoom}
          y={view.y + ((selA.y + selB.y) / 2) * view.zoom}
          onVolume={(id, v) => onEdgeVolume?.(id, v)}
          onMute={(id, m) => onEdgeMute?.(id, m)}
          onPan={(id, p) => onEdgePan?.(id, p)}
          onRemove={(id) => {
            onRemoveEdge?.(id);
            setSelectedEdge(null);
          }}
          onClose={() => setSelectedEdge(null)}
        />
      )}
    </div>
  );
}
