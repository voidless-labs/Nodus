import './EngineButton.css';

/**
 * EngineButton — the floating engine pill at top-center of the canvas (R11).
 * The only engine control. Paused: dim outline "+ engine paused". Live: a
 * solid amber pill "+ engine live" (owner spec, fill rgba 232,163,61).
 */
export function EngineButton({ live, onToggleLive }: { live: boolean; onToggleLive: () => void }) {
  return (
    <button
      className={`engine-float ${live ? 'is-live' : ''}`}
      onClick={onToggleLive}
      title={live ? 'stop routing' : 'start routing'}
    >
      <span className="engine-float-plus">+</span>
      {live ? 'engine live' : 'engine paused'}
    </button>
  );
}
