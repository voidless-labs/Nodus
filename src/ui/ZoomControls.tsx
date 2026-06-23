import './ZoomControls.css';

/**
 * ZoomControls — zoom column at bottom-right (R14 + R21).
 * Zoom in / out, a percent readout (click to reset to 100%), and fit-to-view.
 */
export function ZoomControls({
  zoom = 1,
  onZoomIn,
  onZoomOut,
  onReset,
  onFit,
}: {
  zoom?: number;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
  onReset?: () => void;
  onFit?: () => void;
}) {
  return (
    <div className="zoom-controls">
      <button className="zoom-btn" aria-label="zoom in" title="zoom in" onClick={onZoomIn}>
        <svg {...IC}>
          <path d="M12 5v14M5 12h14" />
        </svg>
      </button>
      <button
        className="zoom-pct"
        aria-label="reset zoom to 100%"
        title="reset zoom"
        onClick={onReset}
      >
        {Math.round(zoom * 100)}%
      </button>
      <button className="zoom-btn" aria-label="zoom out" title="zoom out" onClick={onZoomOut}>
        <svg {...IC}>
          <path d="M5 12h14" />
        </svg>
      </button>
      <button className="zoom-btn" aria-label="fit to view" title="fit to view" onClick={onFit}>
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
