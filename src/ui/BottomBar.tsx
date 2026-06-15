import { useState } from 'react';
import './BottomBar.css';

/**
 * BottomBar — the floating node library (R12), an accent element.
 *
 * Search field (top) + category tabs (bottom). Clicking a category expands a
 * list of that category's node types IN THE MIDDLE of the bar, between the
 * search and the tabs (the bar grows upward). Typing in the search highlights
 * matching nodes on the canvas via `onSearch` (the "type a name to highlight"
 * behavior) and also filters the open category list.
 */
type Tab = 'recent' | 'all' | 'fx' | 'logic' | 'misc';

type NodeType = { id: string; name: string; sub: string };

const CATALOG: Record<Tab, NodeType[]> = {
  recent: [
    { id: 'spotify', name: 'Spotify', sub: 'source' },
    { id: 'headphones', name: 'Headphones', sub: 'output' },
    { id: 'nodusmic', name: 'Nodus Mic', sub: 'virtual' },
  ],
  all: [
    { id: 'source', name: 'Source', sub: 'app or microphone' },
    { id: 'output', name: 'Output', sub: 'speakers / OBS' },
    { id: 'hub', name: 'Stream Mix', sub: 'routing hub' },
    { id: 'virtual', name: 'Virtual device', sub: 'Nodus speaker / mic' },
    { id: 'fx', name: 'Effect', sub: 'on a route' },
    { id: 'logic', name: 'Logic', sub: 'condition / trigger' },
  ],
  fx: [
    { id: 'eq', name: 'EQ', sub: 'equalizer' },
    { id: 'comp', name: 'Compressor', sub: 'dynamics' },
    { id: 'gain', name: 'Gain', sub: 'level' },
    { id: 'limiter', name: 'Limiter', sub: 'ceiling' },
    { id: 'reverb', name: 'Reverb', sub: 'space' },
    { id: 'gate', name: 'Noise Gate', sub: 'cleanup' },
  ],
  logic: [
    { id: 'ptt', name: 'Push-to-Talk', sub: 'hold a key' },
    { id: 'hotkey', name: 'Hotkey', sub: 'toggle on key' },
    { id: 'toggle', name: 'Toggle', sub: 'on / off' },
    { id: 'timer', name: 'Timer', sub: 'time window' },
  ],
  misc: [
    { id: 'streammix', name: 'Stream Mix', sub: 'preset' },
    { id: 'midi', name: 'MIDI', sub: 'control' },
  ],
};

const TABS: { id: Tab; label: string }[] = [
  { id: 'recent', label: 'recent' },
  { id: 'all', label: 'all' },
  { id: 'fx', label: 'fx' },
  { id: 'logic', label: 'logic' },
];

export function BottomBar({ onSearch }: { onSearch?: (q: string) => void }) {
  const [active, setActive] = useState<Tab | null>(null);
  // Remember the last category so the list keeps its content while it
  // animates closed (the wrapper stays mounted for a smooth height collapse).
  const [lastTab, setLastTab] = useState<Tab>('all');
  const [q, setQ] = useState('');

  const setQuery = (v: string) => {
    setQ(v);
    onSearch?.(v);
  };

  const shownTab = active ?? lastTab;
  // Show the whole category; while typing, matching items are highlighted (not
  // filtered out) — same idea as the canvas highlight.
  const items = CATALOG[shownTab];
  const matchOf = (name: string): 'match' | 'dim' | '' => {
    const t = q.trim().toLowerCase();
    if (!t) return '';
    return name.toLowerCase().includes(t) ? 'match' : 'dim';
  };

  const toggle = (t: Tab) => {
    setLastTab(t);
    setActive((cur) => (cur === t ? null : t));
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
                <div className="bb-grid">
                  {items.map((n) => {
                    const m = matchOf(n.name);
                    return (
                      <button
                        key={n.id}
                        className={`bb-card ${m === 'match' ? 'is-match' : ''} ${m === 'dim' ? 'is-dim' : ''}`}
                        tabIndex={active ? 0 : -1}
                        onClick={() => setActive(null)}
                      >
                        <span className="bb-card-name">{n.name}</span>
                        <span className="bb-card-sub">{n.sub}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>

          <div className="bb-tabs">
            {TABS.map((t) => (
              <BBTab key={t.id} active={active === t.id} onClick={() => toggle(t.id)} label={t.label}>
                <TabIcon id={t.id} />
              </BBTab>
            ))}
            <div className="bb-spacer" />
            <BBTab active={active === 'misc'} onClick={() => toggle('misc')} label="misc" round>
              <TabIcon id="misc" />
            </BBTab>
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
    case 'recent':
      return (
        <svg {...IC}>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 2" />
        </svg>
      );
    case 'all':
      return (
        <svg {...IC}>
          <rect x="3" y="3" width="7" height="7" rx="1.5" />
          <rect x="14" y="3" width="7" height="7" rx="1.5" />
          <rect x="3" y="14" width="7" height="7" rx="1.5" />
          <rect x="14" y="14" width="7" height="7" rx="1.5" />
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
    case 'misc':
      return (
        <svg {...IC}>
          <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M18.4 5.6l-2.8 2.8M8.4 15.6l-2.8 2.8" />
        </svg>
      );
  }
}
