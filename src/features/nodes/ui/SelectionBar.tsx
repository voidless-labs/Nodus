import './SelectionBar.css';

/**
 * SelectionBar — contextual actions for a multi-selection (R23).
 *
 * Floats at the top-centre of the canvas when 2+ nodes are selected. Distinct
 * from the (future R20) single-node action menu: this only carries group ops —
 * mute all / unmute all / delete. Press Esc or click empty canvas to dismiss.
 */
export function SelectionBar({
  count,
  onMuteAll,
  onUnmuteAll,
  onDelete,
  onClear,
}: {
  count: number;
  onMuteAll: () => void;
  onUnmuteAll: () => void;
  onDelete: () => void;
  onClear: () => void;
}) {
  return (
    <div className="selbar" role="toolbar" aria-label="selection actions">
      <span className="selbar-count">{count} selected</span>
      <span className="selbar-sep" />
      <button className="selbar-btn" onClick={onMuteAll}>
        mute all
      </button>
      <button className="selbar-btn" onClick={onUnmuteAll}>
        unmute all
      </button>
      <button className="selbar-btn selbar-btn--danger" onClick={onDelete}>
        <IconTrash />
        delete
      </button>
      <button className="selbar-x" aria-label="clear selection" onClick={onClear}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
    </div>
  );
}

function IconTrash() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
    </svg>
  );
}
