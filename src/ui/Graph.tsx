import { useLayoutEffect, useRef, useState } from 'react';
import './Graph.css';
import { NodeCard } from './nodes/NodeCard';
import type { EdgeModel, NodeModel } from './nodes/types';

/**
 * Graph — the node cards plus the wires between them (R8).
 *
 * Edge endpoints are measured from the live DOM (each `.node-port` carries
 * data-node/data-side) rather than computed from card geometry, so the wires
 * stay correct regardless of card height. Measurement runs once per layout
 * change (node set / window resize) — no per-frame work, keeping the weak-CPU
 * budget. Pan/zoom and dragging come later; positions are static for now.
 */
type PortMap = Record<string, { x: number; y: number }>;

function edgePath(x1: number, y1: number, x2: number, y2: number): string {
  // Horizontal-biased cubic bezier, like the reference graph lines.
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

export function Graph({ nodes, edges }: { nodes: NodeModel[]; edges: EdgeModel[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const [ports, setPorts] = useState<PortMap>({});

  useLayoutEffect(() => {
    const measure = () => {
      const container = ref.current;
      if (!container) return;
      const base = container.getBoundingClientRect();
      const map: PortMap = {};
      container.querySelectorAll<HTMLElement>('.node-port').forEach((p) => {
        const r = p.getBoundingClientRect();
        const id = p.dataset.node;
        const side = p.dataset.side;
        if (id && side) {
          map[`${id}:${side}`] = {
            x: r.left + r.width / 2 - base.left,
            y: r.top + r.height / 2 - base.top,
          };
        }
      });
      setPorts(map);
    };
    measure();
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, [nodes]);

  return (
    <div className="graph" ref={ref}>
      <svg className="edge-layer" aria-hidden>
        {edges.map((e) => {
          const a = ports[`${e.from}:out`];
          const b = ports[`${e.to}:in`];
          if (!a || !b) return null;
          const cls = ['edge', e.active ? 'is-active' : '', e.muted ? 'is-muted' : '']
            .filter(Boolean)
            .join(' ');
          return <path key={e.id} className={cls} d={edgePath(a.x, a.y, b.x, b.y)} />;
        })}
      </svg>

      {nodes.map((n) => (
        <NodeCard key={n.id} node={n} />
      ))}
    </div>
  );
}
