import './Topbar.css';

/**
 * Topbar — the single persistent chrome bar (R10).
 *
 * Left: compact NODUS mark + wordmark. Center: scene navigator
 * (‹ Scene name × ›) as in the Claude-design reference. Right: a "⋯" menu
 * (Settings / Import / Export).
 *
 * The engine control lives ONLY on the floating center pill (EngineButton) —
 * no Start/Live in the topbar.
 */
export function Topbar() {
  return (
    <header className="topbar">
      <div className="topbar-left">
        <span className="brand-mark" aria-hidden />
        <span className="brand-name">nodus</span>
      </div>

      <nav className="scene-nav" aria-label="scenes">
        <button className="scene-arrow" aria-label="previous scene">
          <ChevronLeft />
        </button>
        <div className="scene-pill">
          <span className="scene-dot" />
          <span className="scene-name">Scene 1</span>
          <button className="scene-close" aria-label="close scene">
            <Cross />
          </button>
        </div>
        <button className="scene-arrow" aria-label="next scene">
          <ChevronRight />
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
      </div>
    </header>
  );
}

function ChevronLeft() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}
function ChevronRight() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m9 18 6-6-6-6" />
    </svg>
  );
}
function Cross() {
  return (
    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}
