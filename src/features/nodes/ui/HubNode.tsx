import { useRef } from 'react';
import './HubNode.css';
import type { HubModel } from '@/features/nodes/types';
import { VolumeSlider } from '@/shared/ui/VolumeSlider';
import { NodeToolbar } from '@/features/nodes/ui/NodeToolbar';
import { EditableName } from '@/shared/ui/EditableName';

/**
 * HubNode — the routing hub, in two mirror roles (R5 + the strict-port redesign):
 *  - 'mixer'    : N inputs (left, each a row) → one mix output (right).
 *  - 'splitter' : one input (left) → N outputs (right, each a row).
 * The dynamic side grows via a trailing "ghost" port: for a mixer you drag a
 * source ONTO the ghost in-port; for a splitter you drag FROM the ghost out-port
 * to a target. Each real port carries one wire (the strict model).
 */
export function HubNode({
  hub,
  search,
  actions,
  chainState,
  onInputVolume,
  onSolo,
  onPin,
  pinned,
  onDuplicate,
  onDelete,
  onRename,
  inSourceColorVars,
  outConnectedPorts,
}: {
  hub: HubModel;
  search?: 'match' | 'dim';
  actions?: boolean;
  /** Solo-chain highlight: 'on' = in the audible chain, 'off' = dimmed by solo. */
  chainState?: 'on' | 'off';
  /** Remove a dynamic port (mixer input / splitter output). */
  onRemoveInput?: (hubId: string, portId: string) => void;
  /** Set a port's level → its route's trim (R18). */
  onInputVolume?: (hubId: string, portId: string, volume: number) => void;
  /** Solo the mixer (audition everything through it). Splitters get no solo. */
  onSolo?: (id: string) => void;
  onPin?: (id: string) => void;
  pinned?: boolean;
  onDuplicate?: (id: string) => void;
  onDelete?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
  /**
   * IN ports carry the accent of the node feeding them — a hub input can come
   * from a source, an FX, a virtual device or another hub. Keyed by port id;
   * '' is the single fixed port (the splitter's input). Missing ⇒ not wired,
   * so the port stays neutral, exactly as on a leaf node. (t28 ports)
   */
  inSourceColorVars?: Record<string, string>;
  /** Per-port: does this OUT port carry an edge? Keyed like the above. */
  outConnectedPorts?: Record<string, boolean>;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);
  const split = (hub.role ?? 'mixer') === 'splitter';

  /** Paint an IN port with the accent of whatever feeds it (same recipe as NodeCard). */
  const inPortStyle = (portId: string) => {
    const v = inSourceColorVars?.[portId];
    return v
      ? {
          background: `var(${v})`,
          boxShadow: `0 0 9px -1px color-mix(in srgb, var(${v}) 60%, transparent)`,
        }
      : undefined;
  };

  const onGlowMove = (e: React.MouseEvent) => {
    if (rafRef.current != null) return;
    const card = cardRef.current;
    if (!card) return;
    const { clientX, clientY } = e;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      const r = card.getBoundingClientRect();
      card.style.setProperty('--mx', `${((clientX - r.left) / r.width) * 100}%`);
      card.style.setProperty('--my', `${((clientY - r.top) / r.height) * 100}%`);
    });
  };

  const classes = [
    'node',
    'node--hub',
    split ? 'node--splitter' : '',
    hub.active ? 'is-active' : '',
    hub.selected ? 'is-selected' : '',
    search === 'match' ? 'is-search-match' : '',
    search === 'dim' ? 'is-search-dim' : '',
    chainState === 'on' ? 'is-solo-on' : '',
    chainState === 'off' ? 'is-solo-off' : '',
  ]
    .filter(Boolean)
    .join(' ');

  const ports = (
    <div className="hub-inputs">
      {hub.inputs.map((p) => (
        <div className="hub-in-row" key={p.id}>
          <span
            className={
              split
                ? `node-port hub-port-out-row${
                    outConnectedPorts?.[p.id] === false ? ' is-unconnected' : ''
                  }`
                : 'node-port hub-port-in'
            }
            data-node={hub.id}
            data-side={split ? 'out' : 'in'}
            data-port={p.id}
            style={split ? undefined : inPortStyle(p.id)}
          />
          <span className="hub-sig" aria-hidden />
          <span className="hub-in-name">{p.label}</span>
          <VolumeSlider
            className="hub-slider"
            value={p.volume ?? 0}
            onChange={onInputVolume ? (v) => onInputVolume(hub.id, p.id, v) : undefined}
            ariaLabel={`${p.label} level`}
          />
        </div>
      ))}

      {/* Mixer Slot (concept E, 1.25×) — a dashed drop-box (＋ + hint) plus the edge
          phantom-port nub. The whole box is the drop (mixer) / drag (splitter) target. */}
      <div className="hub-in-row hub-add-row">
        <span className="hub-add-nub" aria-hidden>
          <svg className="hub-nub-plus" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
            <path d="M12 6v12M6 12h12" />
          </svg>
        </span>
        <span
          className="node-port hub-port-add"
          data-node={hub.id}
          data-side={split ? 'out' : 'in'}
          data-add="1"
        />
        <span className="hub-add-box">
          <svg className="hub-add-plus" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden>
            <path d="M12 6v12M6 12h12" />
          </svg>
          <span className="hub-add-hint">{split ? 'Drag to a target' : 'Drag a source here'}</span>
        </span>
      </div>
    </div>
  );

  return (
    <div className={classes} data-node-id={hub.id} style={{ left: hub.x, top: hub.y }}>
      <div className="node-label">
        <span className="node-label-dot" />
        {split ? 'splitter' : 'mixer'}
        {hub.solo && <span className="node-solo-tag">solo</span>}
        {pinned && <span className="node-pin-tag">pinned</span>}
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onGlowMove}>
        <div className="node-glow" aria-hidden />

        {actions && (
          <NodeToolbar
            soloActive={hub.solo}
            onSolo={!split && onSolo ? () => onSolo(hub.id) : undefined}
            pinActive={pinned}
            onPin={onPin ? () => onPin(hub.id) : undefined}
            onDuplicate={() => onDuplicate?.(hub.id)}
            onDelete={() => onDelete?.(hub.id)}
          />
        )}

        {/* The single fixed port: mixer's mix-output (right) or splitter's input (left). */}
        {split ? (
          <span
            className="node-port hub-port-in-single"
            data-node={hub.id}
            data-side="in"
            data-port=""
            style={inPortStyle('')}
          />
        ) : (
          <span
            className={`node-port hub-port-out${
              outConnectedPorts?.[''] === false ? ' is-unconnected' : ''
            }`}
            data-node={hub.id}
            data-side="out"
            data-port=""
          />
        )}

        <div className="node-head">
          <div className="node-icon hub-icon">
            <HubGlyph />
          </div>
          <div className="node-titles">
            <EditableName
              className="node-name"
              value={hub.name}
              onRename={onRename ? (name) => onRename(hub.id, name) : undefined}
            />
            <div className="node-sub">{hub.subtitle}</div>
          </div>
        </div>

        {ports}
      </div>
    </div>
  );
}

function HubGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
    </svg>
  );
}
