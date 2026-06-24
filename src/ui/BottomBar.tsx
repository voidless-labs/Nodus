import { useState } from 'react';
import './BottomBar.css';
import type { AudioDevice } from '../bridge';
import { EditableName } from './nodes/EditableName';

/**
 * BottomBar — the floating node library (R12), an accent element.
 *
 * Search field (top) + category tabs (bottom). Clicking a category expands a
 * list of that category's node types IN THE MIDDLE of the bar, between the
 * search and the tabs (the bar grows upward). Typing in the search highlights
 * matching nodes on the canvas via `onSearch` (the "type a name to highlight"
 * behavior) and also filters the open category list.
 */
type Tab = 'routing' | 'virtual' | 'fx' | 'logic';

type NodeType = { id: string; name: string; sub: string };

// The node palette. Real sources/outputs are added via "+ add" (detected
// apps/devices); this palette holds the abstract routing nodes + effects/logic.
// Virtual devices live in their own panel (created, not generic placeholders).
const CATALOG: Record<'routing' | 'fx' | 'logic', NodeType[]> = {
  routing: [
    { id: 'mixer', name: 'Mixer', sub: 'many → one' },
    { id: 'splitter', name: 'Splitter', sub: 'one → many' },
  ],
  fx: [
    { id: 'gate', name: 'Noise Gate', sub: 'cleanup' },
    { id: 'comp', name: 'Compressor', sub: 'dynamics' },
    { id: 'limiter', name: 'Limiter', sub: 'ceiling' },
    { id: 'eq', name: 'EQ', sub: 'equalizer' },
    { id: 'gain', name: 'Gain', sub: 'level / trim' },
  ],
  logic: [
    { id: 'duck', name: 'Ducking', sub: 'lower on trigger' },
    { id: 'trigger', name: 'Push-to-Talk', sub: 'hold a key' },
  ],
};

const TABS: { id: Tab; label: string }[] = [
  { id: 'routing', label: 'routing' },
  { id: 'virtual', label: 'virtual' },
  { id: 'fx', label: 'fx' },
  { id: 'logic', label: 'logic' },
];

export function BottomBar({
  onSearch,
  virtualOwn = [],
  virtualOther = [],
  createdIds,
  editingId,
  onCreateVirtual,
  onRenameVirtual,
  onDeleteVirtual,
  onBeginPlace,
}: {
  onSearch?: (q: string) => void;
  /** Nodus's own virtual devices (created + branded) and third-party ones. */
  virtualOwn?: AudioDevice[];
  virtualOther?: AudioDevice[];
  /** Ids of user-created virtual devices — these are renamable / deletable. */
  createdIds?: Set<string>;
  /** A just-created device id — its name field opens for editing. */
  editingId?: string | null;
  onCreateVirtual?: () => void;
  onRenameVirtual?: (id: string, name: string) => void;
  onDeleteVirtual?: (id: string) => void;
  /** Begin a pointer-based drag of a catalog node / device onto the canvas. */
  onBeginPlace?: (
    e: React.PointerEvent,
    payload: { kind: 'device' | 'type'; id: string },
    label: string,
    onTap: () => void,
  ) => void;
}) {
  const [active, setActive] = useState<Tab | null>(null);
  // Remember the last category so the list keeps its content while it
  // animates closed (the wrapper stays mounted for a smooth height collapse).
  const [lastTab, setLastTab] = useState<Tab>('routing');
  const [q, setQ] = useState('');

  const setQuery = (v: string) => {
    setQ(v);
    onSearch?.(v);
  };

  const shownTab = active ?? lastTab;
  // Show the whole category; while typing, matching items are highlighted (not
  // filtered out) — same idea as the canvas highlight.
  const items = shownTab === 'virtual' ? [] : CATALOG[shownTab];
  const matchOf = (name: string): 'match' | 'dim' | '' => {
    const t = q.trim().toLowerCase();
    if (!t) return '';
    return name.toLowerCase().includes(t) ? 'match' : 'dim';
  };

  const toggle = (t: Tab) => {
    setLastTab(t);
    setActive((cur) => (cur === t ? null : t));
  };

  // Press a catalog card → pointer-drag it onto the canvas (WebView2-safe).
  const startType = (e: React.PointerEvent, n: NodeType) => {
    onBeginPlace?.(e, { kind: 'type', id: n.id }, n.name, () => {});
    setActive(null);
  };
  // Press a virtual device card → pointer-drag an instance node bound to it
  // (each drag = another instance of the SAME device). Ignore presses that land
  // on the card's interactive bits (rename input / delete ×).
  const startDevice = (e: React.PointerEvent, dev: AudioDevice, label: string) => {
    if ((e.target as HTMLElement).closest('input, .bb-card-x')) return;
    onBeginPlace?.(e, { kind: 'device', id: dev.id }, label, () => {});
    setActive(null);
  };

  const deviceCard = (dev: AudioDevice) => {
    const name = dev.name.replace(/\s*\([^)]*\)\s*$/, '').trim() || dev.name;
    const m = matchOf(name);
    const created = createdIds?.has(dev.id) ?? false;
    return (
      <div
        key={dev.id}
        className={`bb-card ${m === 'match' ? 'is-match' : ''} ${m === 'dim' ? 'is-dim' : ''}`}
        role="button"
        tabIndex={active ? 0 : -1}
        onPointerDown={(e) => startDevice(e, dev, name)}
      >
        {created ? (
          <EditableName
            className="bb-card-name"
            value={dev.name}
            autoEdit={editingId === dev.id}
            placeholder="Set a name for the virtual mic"
            onRename={(nm) => onRenameVirtual?.(dev.id, nm)}
          />
        ) : (
          <span className="bb-card-name">{name}</span>
        )}
        <span className="bb-card-sub">virtual device · drag to add</span>
        {created && (
          <button
            className="bb-card-x"
            aria-label="delete device"
            title="delete device"
            onClick={(e) => {
              e.stopPropagation();
              onDeleteVirtual?.(dev.id);
            }}
            onDragStart={(e) => e.preventDefault()}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        )}
      </div>
    );
  };

  return (
    <>
      {active && <div className="bb-overlay" onClick={() => setActive(null)} />}
      <div className={`bottombar ${active ? 'is-open' : ''}`}>
        <div className="bb-bar">
          <label className="bb-search">
            <SearchIcon />
            <input
              value={q}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="search nodes · type a name to highlight…"
            />
          </label>

          <div className={`bb-mid-wrap ${active ? 'is-open' : ''}`} aria-hidden={!active}>
            <div className="bb-mid-inner">
              <div className="bb-mid">
                <div className="bb-mid-h">{shownTab}</div>
                {shownTab === 'virtual' ? (
                  <>
                    <div className="bb-group-h">ours</div>
                    <div className="bb-grid">
                      {virtualOwn.map((dev) => deviceCard(dev))}
                      <button
                        className="bb-card bb-card--add"
                        tabIndex={active ? 0 : -1}
                        onClick={() => onCreateVirtual?.()}
                      >
                        <span className="bb-card-name">+ new device</span>
                        <span className="bb-card-sub">create a virtual mic</span>
                      </button>
                    </div>
                    {virtualOther.length > 0 && (
                      <>
                        <div className="bb-group-h">other</div>
                        <div className="bb-grid">{virtualOther.map((dev) => deviceCard(dev))}</div>
                      </>
                    )}
                  </>
                ) : (
                  <div className="bb-grid">
                    {items.map((n) => {
                      const m = matchOf(n.name);
                      return (
                        // Pointer-based drag (native HTML5 drag doesn't start
                        // reliably in WebView2). Press + move places on canvas.
                        <div
                          key={n.id}
                          className={`bb-card ${m === 'match' ? 'is-match' : ''} ${m === 'dim' ? 'is-dim' : ''}`}
                          role="button"
                          tabIndex={active ? 0 : -1}
                          onPointerDown={(e) => startType(e, n)}
                        >
                          <span className="bb-card-name">{n.name}</span>
                          <span className="bb-card-sub">{n.sub}</span>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          </div>

          <div className="bb-tabs">
            {TABS.map((t) => (
              <BBTab key={t.id} active={active === t.id} onClick={() => toggle(t.id)} label={t.label}>
                <TabIcon id={t.id} />
              </BBTab>
            ))}
          </div>
        </div>
      </div>
    </>
  );
}

function BBTab({
  active,
  onClick,
  label,
  round,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  round?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      className={`bb-tab ${round ? 'bb-tab--round' : ''} ${active ? 'is-active' : ''}`}
      onClick={onClick}
      title={label}
      aria-label={label}
    >
      {children}
    </button>
  );
}

const IC = {
  width: 17,
  height: 17,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
function SearchIcon() {
  return (
    <svg {...IC} width={15} height={15}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}
function TabIcon({ id }: { id: Tab }) {
  switch (id) {
    case 'routing':
      return (
        <svg {...IC}>
          <circle cx="6" cy="6" r="2.5" />
          <circle cx="6" cy="18" r="2.5" />
          <circle cx="18" cy="12" r="2.5" />
          <path d="M8.2 7 15.8 11M8.2 17 15.8 13" />
        </svg>
      );
    case 'virtual':
      return (
        <svg {...IC}>
          <rect x="9" y="2" width="6" height="11" rx="3" />
          <path d="M5 10a7 7 0 0 0 14 0M12 17v4" />
        </svg>
      );
    case 'fx':
      return (
        <svg {...IC}>
          <path d="M4 21V14M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
        </svg>
      );
    case 'logic':
      return (
        <svg {...IC}>
          <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z" />
        </svg>
      );
  }
}
