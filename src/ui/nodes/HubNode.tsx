import './HubNode.css';
import type { HubModel } from './types';

/**
 * HubNode — the "Stream Mix" routing hub (R5), matched 1:1 to Node-design.png.
 *
 * Two layers: an outer blue frosted frame and an inner dark card holding the
 * header, the labeled inputs (mic/music/game) and the "mix" output. The setting
 * rows (sample rate / limiter / mode) and the output meter sit on the frame
 * BELOW the inner card. Connection ports are small nubs on the frame edges,
 * aligned to the input rows and the mix row (data-node/data-side/data-port).
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
    <div className={classes} style={{ left: hub.x, top: hub.y }}>
      <div className="node-label">
        <span className="node-label-dot" />
        mixer
      </div>

      <div className="hub-frame">
        <div className="hub-glow" aria-hidden />

        <div className="hub-inner">
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
            {hub.inputs.map((inp) => (
              <div className="hub-in-row" key={inp.id}>
                <span
                  className="node-port hub-port-in"
                  data-node={hub.id}
                  data-side="in"
                  data-port={inp.id}
                />
                <span className="hub-in-dot" />
                {inp.label}
              </div>
            ))}

            <div className="hub-mix-row">
              mix
              <span className="hub-mix-dot" />
              <span
                className="node-port hub-port-out"
                data-node={hub.id}
                data-side="out"
                data-port=""
              />
            </div>
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
      <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
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
