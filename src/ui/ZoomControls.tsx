import './ZoomControls.css';

/**
 * ZoomControls — quiet zoom column at bottom-right (R14).
 * Visual only for now; pan/zoom transforms come with the interactive canvas.
 */
export function ZoomControls() {
  return (
    <div className="zoom-controls">
      <button className="zoom-btn" aria-label="zoom in" title="zoom in">
        <svg {...IC}>
          <path d="M12 5v14M5 12h14" />
        </svg>
      </button>
      <button className="zoom-btn" aria-label="zoom out" title="zoom out">
        <svg {...IC}>
          <path d="M5 12h14" />
        </svg>
      </button>
      <button className="zoom-btn" aria-label="fit to view" title="fit to view">
        <svg {...IC}>
          <path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3" />
        </svg>
      </button>
    </div>
  );
}

const IC = {
  width: 16,
  height: 16,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
