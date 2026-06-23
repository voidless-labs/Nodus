import { useRef } from 'react';
import './HubNode.css';
import type { HubModel } from './types';
import { VolumeSlider } from './VolumeSlider';
import { NodeToolbar } from './NodeToolbar';
import { EditableName } from './EditableName';

/**
 * HubNode — the "Stream Mix" routing node (R5).
 *
 * Polished node style (the approved Spotify card), with the real mixer body
 * from the Claude-design code: a per-input volume list (each connected input
 * has its own mini slider + percent) flowing into one "mix" output. Input
 * ports ride the card's left edge at their rows; the output port is on the
 * right. Cursor-reactive border in the hub's blue type color.
 */
export function HubNode({
  hub,
  search,
  actions,
  onRemoveInput,
  onDuplicate,
  onDelete,
  onRename,
}: {
  hub: HubModel;
  search?: 'match' | 'dim';
  /** Show the action toolbar (sole selected node) — R20. */
  actions?: boolean;
  /** Remove a mixer input (R24). Adding is via the trailing ghost port. */
  onRemoveInput?: (hubId: string, inputId: string) => void;
  onDuplicate?: (id: string) => void;
  onDelete?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);

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
    hub.active ? 'is-active' : '',
    hub.selected ? 'is-selected' : '',
    search === 'match' ? 'is-search-match' : '',
    search === 'dim' ? 'is-search-dim' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={classes} data-node-id={hub.id} style={{ left: hub.x, top: hub.y }}>
      <div className="node-label">
        <span className="node-label-dot" />
        mixer
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onGlowMove}>
        <div className="node-glow" aria-hidden />

        {actions && (
          <NodeToolbar
            onDuplicate={() => onDuplicate?.(hub.id)}
            onDelete={() => onDelete?.(hub.id)}
          />
        )}

        <span className="node-port hub-port-out" data-node={hub.id} data-side="out" data-port="" />

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

        <div className="hub-inputs">
          <div className="hub-inputs-h">inputs · {hub.inputs.length}</div>
          {hub.inputs.map((inp) => (
            <div className="hub-in-row" key={inp.id}>
              <span
                className="node-port hub-port-in"
                data-node={hub.id}
                data-side="in"
                data-port={inp.id}
              />
              <span className="hub-in-name">{inp.label}</span>
              <VolumeSlider
                className="hub-slider"
                defaultValue={inp.volume ?? 0}
                ariaLabel={`${inp.label} level`}
              />
              <button
                className="hub-in-x"
                aria-label={`remove ${inp.label}`}
                title="remove input"
                onClick={() => onRemoveInput?.(hub.id, inp.id)}
              >
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
                  <path d="M18 6 6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          ))}

          {/* Always-present "ghost" input: drag a source's output here to add one. */}
          <div className="hub-in-row hub-add-row">
            <span className="node-port hub-port-add" data-node={hub.id} data-side="in" data-add="1">
              <svg className="hub-add-plus" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" aria-hidden>
                <path d="M12 6v12M6 12h12" />
              </svg>
            </span>
            <span className="hub-add-hint">drag a source here</span>
          </div>
        </div>

        <div className="hub-out">
          <span className="hub-out-label">mix</span>
          <div className="hub-meter" aria-hidden>
            <div className="hub-meter-fill" style={{ width: `${hub.level * 100}%` }} />
          </div>
        </div>
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
