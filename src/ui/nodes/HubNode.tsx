import { useRef } from 'react';
import './HubNode.css';
import type { HubModel } from './types';
import { VolumeSlider } from './VolumeSlider';
import { NodeToolbar } from './NodeToolbar';
import { EditableName } from './EditableName';

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
  onRemoveInput,
  onInputVolume,
  onPin,
  pinned,
  onDuplicate,
  onDelete,
  onRename,
}: {
  hub: HubModel;
  search?: 'match' | 'dim';
  actions?: boolean;
  /** Remove a dynamic port (mixer input / splitter output). */
  onRemoveInput?: (hubId: string, portId: string) => void;
  /** Set a port's level → its route's trim (R18). */
  onInputVolume?: (hubId: string, portId: string, volume: number) => void;
  onPin?: (id: string) => void;
  pinned?: boolean;
  onDuplicate?: (id: string) => void;
  onDelete?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);
  const split = (hub.role ?? 'mixer') === 'splitter';

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
  ]
    .filter(Boolean)
    .join(' ');

  const ports = (
    <div className="hub-inputs">
      <div className="hub-inputs-h">
        {split ? 'outputs' : 'inputs'} · {hub.inputs.length}
      </div>
      {hub.inputs.map((p) => (
        <div className="hub-in-row" key={p.id}>
          <span
            className={`node-port ${split ? 'hub-port-out-row' : 'hub-port-in'}`}
            data-node={hub.id}
            data-side={split ? 'out' : 'in'}
            data-port={p.id}
          />
          <span className="hub-in-name">{p.label}</span>
          <VolumeSlider
            className="hub-slider"
            value={p.volume ?? 0}
            onChange={onInputVolume ? (v) => onInputVolume(hub.id, p.id, v) : undefined}
            ariaLabel={`${p.label} level`}
          />
          <button
            className="hub-in-x"
            aria-label={`remove ${p.label}`}
            title="remove"
            onClick={() => onRemoveInput?.(hub.id, p.id)}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      ))}

      {/* Trailing "ghost" port: mixer = drop a source here; splitter = drag from here. */}
      <div className="hub-in-row hub-add-row">
        <span
          className={`node-port hub-port-add ${split ? 'hub-port-add-out' : ''}`}
          data-node={hub.id}
          data-side={split ? 'out' : 'in'}
          data-add="1"
        >
          <svg className="hub-add-plus" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" aria-hidden>
            <path d="M12 6v12M6 12h12" />
          </svg>
        </span>
        <span className="hub-add-hint">{split ? 'drag to a target' : 'drag a source here'}</span>
      </div>
    </div>
  );

  const meter = (label: string) => (
    <div className="hub-out">
      <span className="hub-out-label">{label}</span>
      <div className="hub-meter" aria-hidden>
        <div className="hub-meter-fill" style={{ width: `${hub.level * 100}%` }} />
      </div>
    </div>
  );

  return (
    <div className={classes} data-node-id={hub.id} style={{ left: hub.x, top: hub.y }}>
      <div className="node-label">
        <span className="node-label-dot" />
        {split ? 'splitter' : 'mixer'}
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onGlowMove}>
        <div className="node-glow" aria-hidden />

        {actions && (
          <NodeToolbar
            pinActive={pinned}
            onPin={onPin ? () => onPin(hub.id) : undefined}
            onDuplicate={() => onDuplicate?.(hub.id)}
            onDelete={() => onDelete?.(hub.id)}
          />
        )}

        {/* The single fixed port: mixer's mix-output (right) or splitter's input (left). */}
        {split ? (
          <span className="node-port hub-port-in-single" data-node={hub.id} data-side="in" data-port="" />
        ) : (
          <span className="node-port hub-port-out" data-node={hub.id} data-side="out" data-port="" />
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

        {split ? (
          <>
            {meter('in')}
            {ports}
          </>
        ) : (
          <>
            {ports}
            {meter('mix')}
          </>
        )}
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
