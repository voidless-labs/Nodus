import './Topbar.css';

/**
 * Topbar — the single persistent chrome bar (R10).
 *
 * Left: compact NODUS mark. Center: scene tabs (pills). Right: a "⋯" menu
 * (Settings / Import / Export) and the accent engine pill (Start ↔ Live),
 * synced with the floating EngineButton via the shared `live` state.
 * No ENGAGE/IDLE wording anywhere.
 */
export function Topbar({ live, onToggleLive }: { live: boolean; onToggleLive: () => void }) {
  return (
    <header className="topbar">
      <div className="topbar-left">
        <span className="brand-mark" aria-hidden>
          <i style={{ background: 'var(--color-type-source)' }} />
          <i style={{ background: 'var(--color-type-output)' }} />
          <i style={{ background: 'var(--color-accent)' }} />
        </span>
        <span className="brand-name">nodus</span>
      </div>

      <nav className="topbar-center" aria-label="scenes">
        <button className="scene-tab is-active">
          <span className="scene-dot" />
          Scene 1
          <span className="scene-close" aria-label="close scene">
            ×
          </span>
        </button>
        <button className="scene-add" aria-label="add scene">
          +
        </button>
      </nav>

      <div className="topbar-right">
        <button className="topbar-menu" aria-label="menu">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <circle cx="5" cy="12" r="1.6" />
            <circle cx="12" cy="12" r="1.6" />
            <circle cx="19" cy="12" r="1.6" />
          </svg>
        </button>
        <button
          className={`engine-pill ${live ? 'is-live' : ''}`}
          onClick={onToggleLive}
          title={live ? 'stop routing' : 'start routing'}
        >
          {live ? (
            <>
              <span className="engine-dot" />
              live
            </>
          ) : (
            <>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor">
                <path d="M8 5v14l11-7z" />
              </svg>
              start
            </>
          )}
        </button>
      </div>
    </header>
  );
}
