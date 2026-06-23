import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import './Graph.css';
import { NodeCard } from './nodes/NodeCard';
import { HubNode } from './nodes/HubNode';
import { EdgePopover } from './EdgePopover';
import type { EdgeModel, HubModel, NodeModel } from './nodes/types';
import type { View } from '../useView';

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
  onRemoveEdge,
  onRemoveHubInput,
  onConnectNewInput,
}: {
  nodes: NodeModel[];
  edges: EdgeModel[];
  hubs?: HubModel[];
  search?: string;
  /** Live per-source levels from the engine (keyed by device id / exe name). */
  levels?: Record<string, number>;
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
  onConnect?: (from: string, to: string, toPort?: string) => void;
  /** Edge popover (R9): per-route volume / mute / balance / delete. */
  onEdgeVolume?: (id: string, volume: number) => void;
  onEdgeMute?: (id: string, muted: boolean) => void;
  onEdgePan?: (id: string, pan: number) => void;
  onRemoveEdge?: (id: string) => void;
  /** Dynamic hub ports (R24). */
  onRemoveHubInput?: (hubId: string, inputId: string) => void;
  /** Drop a wire on a hub's ghost port → create a new input + connect. */
  onConnectNewInput?: (fromNode: string, hubId: string) => void;
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
  const dragFrom = useRef<string | null>(null);
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

    // Output port → start a wire.
    const port = target.closest<HTMLElement>('.node-port');
    if (port && port.dataset.side === 'out' && port.dataset.node) {
      e.preventDefault();
      const r = port.getBoundingClientRect();
      const start = screenToWorld(r.left + r.width / 2, r.top + r.height / 2);
      dragFrom.current = port.dataset.node;
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
        if (port.dataset.add) onConnectNewInput?.(from, port.dataset.node);
        else onConnect?.(from, port.dataset.node, port.dataset.port || undefined);
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
  }, [dragging, onConnect, onConnectNewInput]);

  const worldTransform = `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`;
  // The lone selected node gets the action toolbar (R20). 2+ → SelectionBar.
  const soleSelected = selection.size === 1 ? selection.values().next().value : null;

  // Selected wire midpoint, projected world→screen for the (unscaled) popover.
  const selected = selectedEdge ? edges.find((x) => x.id === selectedEdge) : undefined;
  const selA = selected && ports[key(selected.from, 'out')];
  const selB = selected && ports[key(selected.to, 'in', selected.toPort ?? '')];

  return (
    <div className={`graph ${panning ? 'is-panning' : ''}`} ref={ref} onMouseDown={onGraphMouseDown}>
      <div className="graph-world" style={{ transform: worldTransform }}>
        <svg className="edge-layer" aria-hidden>
          {edges.map((e) => {
            const a = ports[key(e.from, 'out')];
            const b = ports[key(e.to, 'in', e.toPort ?? '')];
            if (!a || !b) return null;
            const cls = [
              'edge',
              e.active ? 'is-active' : '',
              e.muted ? 'is-muted' : '',
              e.id === selectedEdge ? 'is-selected' : '',
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

        {hubs.map((h) => (
          <HubNode
            key={h.id}
            hub={selection.has(h.id) ? { ...h, selected: true } : h}
            search={searchFor(h.name, search)}
            actions={h.id === soleSelected}
            onRemoveInput={onRemoveHubInput}
            onDuplicate={onNodeDuplicate}
            onDelete={onNodeDelete}
            onRename={onNodeRename}
          />
        ))}
        {nodes.map((n) => {
          // Live meter: the engine reports levels by device id or exe name.
          const live = (n.deviceId && levels[n.deviceId]) ?? (n.exeName && levels[n.exeName]);
          const node = {
            ...n,
            ...(typeof live === 'number' ? { level: live } : null),
            selected: n.selected || selection.has(n.id),
          };
          return (
            <NodeCard
              key={n.id}
              node={node}
              search={searchFor(n.name, search)}
              actions={n.id === soleSelected}
              onVolume={onNodeVolume}
              onMute={onNodeMute}
              onSolo={onNodeSolo}
              onDuplicate={onNodeDuplicate}
              onDelete={onNodeDelete}
              onRename={onNodeRename}
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
