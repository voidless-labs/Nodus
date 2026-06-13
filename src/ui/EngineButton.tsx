import './EngineButton.css';

/**
 * EngineButton — the floating engine pill at top-center of the canvas (R11).
 * Mirrors the topbar engine pill; both read/write the same `live` state so
 * they stay in sync. Paused: dim "engine paused". Live: amber "● engine live".
 */
export function EngineButton({ live, onToggleLive }: { live: boolean; onToggleLive: () => void }) {
  return (
    <button
      className={`engine-float ${live ? 'is-live' : ''}`}
      onClick={onToggleLive}
      title={live ? 'stop routing' : 'start routing'}
    >
      {live ? <span className="engine-float-dot" /> : <span className="engine-float-plus">+</span>}
      {live ? 'engine live' : 'engine paused'}
    </button>
  );
}
