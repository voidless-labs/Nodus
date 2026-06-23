import './EdgePopover.css';
import type { EdgeModel } from './nodes/types';

/**
 * EdgePopover — per-route controls on a wire (R9).
 *
 * Opens when a wire is clicked; replaces the old side inspector. Carries the
 * per-route values the engine actually uses (volume, mute, balance) plus a
 * delete. Positioned at the edge midpoint (graph-local coords), floating above
 * the click point. Pure presentation: all changes go through the store
 * callbacks (setEdgeVolume / setEdgeMute / setEdgePan / removeEdge).
 */
export function EdgePopover({
  edge,
  x,
  y,
  onVolume,
  onMute,
  onPan,
  onRemove,
  onClose,
}: {
  edge: EdgeModel;
  x: number;
  y: number;
  onVolume: (id: string, volume: number) => void;
  onMute: (id: string, muted: boolean) => void;
  onPan: (id: string, pan: number) => void;
  onRemove: (id: string) => void;
  onClose: () => void;
}) {
  const vol = Math.round((edge.volume ?? 1) * 100);
  const pan = Math.round((edge.pan ?? 0) * 100); // -100..100
  const muted = !!edge.muted;
  const balanceLabel = pan === 0 ? 'center' : pan < 0 ? `L ${-pan}` : `R ${pan}`;

  return (
    <div className="edge-popover" style={{ left: x, top: y }} role="dialog" aria-label="route controls">
      <div className="ep-head">
        <span className="ep-title">route</span>
        <button className="ep-close" aria-label="close" onClick={onClose}>
          <IconClose />
        </button>
      </div>

      <div className="ep-row">
        <button
          className={`ep-mute ${muted ? 'is-muted' : ''}`}
          aria-label={muted ? 'unmute' : 'mute'}
          onClick={() => onMute(edge.id, !muted)}
        >
          {muted ? <IconMuted /> : <IconSpeaker />}
        </button>
        <input
          className="ep-slider"
          type="range"
          min={0}
          max={100}
          value={vol}
          onChange={(e) => onVolume(edge.id, Number(e.target.value) / 100)}
          aria-label="volume"
        />
        <span className="ep-val">{vol}%</span>
      </div>

      <div className="ep-row">
        <span className="ep-bal-end">L</span>
        <input
          className="ep-slider ep-slider--bal"
          type="range"
          min={-100}
          max={100}
          value={pan}
          onChange={(e) => onPan(edge.id, Number(e.target.value) / 100)}
          aria-label="balance"
        />
        <span className="ep-bal-end">R</span>
      </div>
      <div className="ep-bal-label">{balanceLabel}</div>

      <button className="ep-delete" onClick={() => onRemove(edge.id)}>
        <IconTrash />
        remove route
      </button>
    </div>
  );
}

const IC = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

function IconClose() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" {...IC}>
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}
function IconSpeaker() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" {...IC}>
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
    </svg>
  );
}
function IconMuted() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" {...IC}>
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <path d="M22 9l-6 6M16 9l6 6" />
    </svg>
  );
}
function IconTrash() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" {...IC}>
      <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
    </svg>
  );
}
