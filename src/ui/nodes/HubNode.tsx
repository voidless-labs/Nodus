import './HubNode.css';
import type { HubModel } from './types';

/**
 * HubNode — the "Stream Mix" routing hub (R5), matched to Node-design.png.
 *
 * Many labeled inputs (mic / music / game) flow into one mixed output. Setting
 * rows (sample rate, limiter, mode) render as dropdown-style chips. Each input
 * is a real port (data-node/data-side="in"/data-port=<id>) so edges land on the
 * right row; the output is data-side="out". Steel-neutral type color.
 */
export function HubNode({ hub }: { hub: HubModel }) {
  const classes = [
    'node',
    'node--hub',
    hub.active ? 'is-active' : '',
    hub.selected ? 'is-selected' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      className={classes}
      style={{ left: hub.x, top: hub.y, ['--type-color' as string]: 'var(--color-type-hub)' }}
    >
      <div className="node-label">
        <span className="node-label-dot" />
        mixer
      </div>

      <div className="node-card hub-card">
        <div className="node-glow" aria-hidden />

        <div className="node-head">
          <div className="node-icon">
            <HubGlyph />
          </div>
          <div className="node-titles">
            <div className="node-name">{hub.name}</div>
            <div className="node-sub">{hub.subtitle}</div>
          </div>
        </div>

        <div className="hub-io">
          <div className="hub-inputs">
            {hub.inputs.map((inp) => (
              <div className="hub-in-row" key={inp.id}>
                <span
                  className="node-port node-port--in hub-port"
                  data-node={hub.id}
                  data-side="in"
                  data-port={inp.id}
                />
                <span className="hub-in-dot" />
                {inp.label}
              </div>
            ))}
          </div>
          <div className="hub-out-row">
            mix
            <span className="hub-out-dot" />
            <span
              className="node-port node-port--out hub-port-out"
              data-node={hub.id}
              data-side="out"
              data-port=""
            />
          </div>
        </div>

        <div className="hub-settings">
          {hub.settings.map((s) => (
            <div className="hub-set-row" key={s.label}>
              <span className="hub-set-label">{s.label}</span>
              <span className="hub-set-chip">
                {s.value}
                <ChevronDown />
              </span>
            </div>
          ))}
        </div>

        <div className="hub-meter" aria-hidden>
          <div className="hub-meter-fill" style={{ width: `${hub.level * 100}%` }} />
        </div>
      </div>
    </div>
  );
}

function HubGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6h6M14 6h6M4 12h10M18 12h2M4 18h4M12 18h8" />
      <circle cx="12" cy="6" r="2" />
      <circle cx="16" cy="12" r="2" />
      <circle cx="10" cy="18" r="2" />
    </svg>
  );
}
function ChevronDown() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}
