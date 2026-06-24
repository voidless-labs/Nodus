/**
 * NodeToolbar — actions for a single selected node (R20, variant B).
 *
 * A compact strip pinned to the card's top edge: Solo (leaf nodes only),
 * Duplicate, Delete. Shown only while exactly one node is selected. Lives inside
 * the card so it scales with the node under zoom, like the mute/slider controls.
 */
export function NodeToolbar({
  onSolo,
  soloActive,
  onPin,
  pinActive,
  onDuplicate,
  onDelete,
}: {
  onSolo?: () => void;
  soloActive?: boolean;
  /** Pin/unpin to the quick-controls popup (t13). */
  onPin?: () => void;
  pinActive?: boolean;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="node-toolbar" role="toolbar" aria-label="node actions">
      {onSolo && (
        <button
          className={`node-tb-btn ${soloActive ? 'is-active' : ''}`}
          title="solo"
          aria-label="solo"
          aria-pressed={soloActive}
          onClick={onSolo}
        >
          <IconSolo />
        </button>
      )}
      {onPin && (
        <button
          className={`node-tb-btn ${pinActive ? 'is-active' : ''}`}
          title={pinActive ? 'unpin from quick controls' : 'pin to quick controls'}
          aria-label="pin"
          aria-pressed={pinActive}
          onClick={onPin}
        >
          <IconPin />
        </button>
      )}
      <button className="node-tb-btn" title="duplicate" aria-label="duplicate" onClick={onDuplicate}>
        <IconCopy />
      </button>
      <button
        className="node-tb-btn node-tb-btn--danger"
        title="delete"
        aria-label="delete"
        onClick={onDelete}
      >
        <IconTrash />
      </button>
    </div>
  );
}

const IC = {
  width: 13,
  height: 13,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

function IconSolo() {
  return (
    <svg {...IC}>
      <path d="M12 3v18M7 6v12M17 6v12M3 10v4M21 10v4" />
    </svg>
  );
}
function IconPin() {
  return (
    <svg {...IC}>
      <path d="M12 17v5M9 3h6l-1 6 3 3v1H7v-1l3-3-1-6Z" />
    </svg>
  );
}
function IconCopy() {
  return (
    <svg {...IC}>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15V5a2 2 0 0 1 2-2h10" />
    </svg>
  );
}
function IconTrash() {
  return (
    <svg {...IC}>
      <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
    </svg>
  );
}
