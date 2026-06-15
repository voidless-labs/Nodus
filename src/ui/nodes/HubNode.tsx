import { useRef } from 'react';
import './HubNode.css';
import type { HubModel } from './types';

/**
 * HubNode — the "Stream Mix" routing node (R5).
 *
 * Polished node style (the approved Spotify card), with the real mixer body
 * from the Claude-design code: a per-input volume list (each connected input
 * has its own mini slider + percent) flowing into one "mix" output. Input
 * ports ride the card's left edge at their rows; the output port is on the
 * right. Cursor-reactive border in the hub's blue type color.
 */
export function HubNode({ hub, search }: { hub: HubModel; search?: 'match' | 'dim' }) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);

  const onMove = (e: React.MouseEvent) => {
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
    <div className={classes} style={{ left: hub.x, top: hub.y }}>
      <div className="node-label">
        <span className="node-label-dot" />
        mixer
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onMove}>
        <div className="node-glow" aria-hidden />

        <span className="node-port hub-port-out" data-node={hub.id} data-side="out" data-port="" />

        <div className="node-head">
          <div className="node-icon hub-icon">
            <HubGlyph />
          </div>
          <div className="node-titles">
            <div className="node-name">{hub.name}</div>
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
              <input
                className="node-slider hub-slider"
                type="range"
                min={0}
                max={100}
                defaultValue={Math.round((inp.volume ?? 0) * 100)}
                aria-label={`${inp.label} level`}
              />
              <span className="hub-in-pct">{Math.round((inp.volume ?? 0) * 100)}%</span>
            </div>
          ))}
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
