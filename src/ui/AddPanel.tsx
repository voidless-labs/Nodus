import { useMemo, useState } from 'react';
import './AddPanel.css';

/**
 * AddPanel — the "+ add" launcher at bottom-left (R13), an accent element.
 *
 * Collapsed: a prominent "+ add" pill with the node/route counts to its right.
 * Expanded: a wide panel growing upward with a search field (styled like the
 * BottomBar search) over two sections — "playing now" (auto-detected apps) and
 * "devices" — each row a colored round icon + name + type.
 *
 * Visual only for now: lists are sample data, picking a row just closes the
 * panel; real process/device data and node creation are wired in R3/R18.
 */
type Glyph = 'music' | 'game' | 'mic' | 'headphones' | 'speaker';
type TypeColor = 'source' | 'output' | 'virtual';
type AddItem = { id: string; name: string; sub: string; glyph: Glyph; color: TypeColor };

const SAMPLE_APPS: AddItem[] = [
  { id: 'spotify', name: 'Spotify', sub: 'playing now', glyph: 'music', color: 'source' },
  { id: 'cyberpunk', name: 'Cyberpunk', sub: 'playing now', glyph: 'game', color: 'source' },
  { id: 'discord', name: 'Discord', sub: 'quiet', glyph: 'mic', color: 'source' },
  { id: 'mic', name: 'Microphone', sub: 'Blue Yeti', glyph: 'mic', color: 'source' },
];

const SAMPLE_DEVICES: AddItem[] = [
  { id: 'headphones', name: 'Headphones', sub: 'Galaxy Buds', glyph: 'headphones', color: 'output' },
  { id: 'speakers', name: 'Speakers', sub: 'Realtek', glyph: 'speaker', color: 'output' },
  { id: 'nodusmic', name: 'Nodus Mic', sub: 'virtual · to Discord', glyph: 'mic', color: 'virtual' },
];

export function AddPanel({ nodes, routes }: { nodes: number; routes: number }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');

  const [apps, devices] = useMemo(() => {
    const f = (xs: AddItem[]) =>
      q.trim() ? xs.filter((x) => x.name.toLowerCase().includes(q.trim().toLowerCase())) : xs;
    return [f(SAMPLE_APPS), f(SAMPLE_DEVICES)];
  }, [q]);

  const pick = () => {
    setOpen(false);
    setQ('');
  };

  return (
    <>
      {open && <div className="ap-overlay" onClick={() => setOpen(false)} />}
      <div className={`addpanel ${open ? 'is-open' : ''}`}>
        {open && (
          <div className="ap-pop" role="dialog" aria-label="add to canvas">
            <label className="ap-search">
              <SearchIcon />
              <input
                autoFocus
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder="find an app or device…"
              />
            </label>

            <div className="ap-scroll">
              {apps.length > 0 && (
                <div className="ap-section">
                  <div className="ap-section-h">playing now</div>
                  {apps.map((it) => (
                    <AddRow key={it.id} item={it} onClick={pick} />
                  ))}
                </div>
              )}
              {devices.length > 0 && (
                <div className="ap-section">
                  <div className="ap-section-h">devices</div>
                  {devices.map((it) => (
                    <AddRow key={it.id} item={it} onClick={pick} />
                  ))}
                </div>
              )}
              {apps.length === 0 && devices.length === 0 && (
                <div className="ap-empty">nothing matches “{q}”</div>
              )}
            </div>
          </div>
        )}

        <div className="ap-bar">
          <button className="ap-toggle" onClick={() => setOpen((v) => !v)}>
            <span className="ap-toggle-plus">+</span>
            add
          </button>
          <div className="ap-status" aria-hidden>
            <span>
              <b>nodes</b> {nodes}
            </span>
            <span>
              <b>routes</b> {routes}
            </span>
          </div>
        </div>
      </div>
    </>
  );
}

function AddRow({ item, onClick }: { item: AddItem; onClick: () => void }) {
  return (
    <button
      className="ap-row"
      onClick={onClick}
      style={{ ['--c' as string]: `var(--color-type-${item.color})` }}
    >
      <span className="ap-row-icon">
        <Glyph kind={item.glyph} />
      </span>
      <span className="ap-row-text">
        <span className="ap-row-name">{item.name}</span>
        <span className="ap-row-sub">{item.sub}</span>
      </span>
      <span className="ap-row-add">+</span>
    </button>
  );
}

const IC = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

function SearchIcon() {
  return (
    <svg width={15} height={15} viewBox="0 0 24 24" {...IC}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}

function Glyph({ kind }: { kind: Glyph }) {
  const p = { width: 17, height: 17, viewBox: '0 0 24 24', ...IC };
  switch (kind) {
    case 'music':
      return (
        <svg {...p}>
          <path d="M9 18V5l11-2v13" />
          <circle cx="6" cy="18" r="3" />
          <circle cx="17" cy="16" r="3" />
        </svg>
      );
    case 'game':
      return (
        <svg {...p}>
          <path d="M6 12h4M8 10v4M15 11h.01M18 13h.01" />
          <rect x="2" y="6" width="20" height="12" rx="6" />
        </svg>
      );
    case 'mic':
      return (
        <svg {...p}>
          <rect x="9" y="2" width="6" height="11" rx="3" />
          <path d="M5 10a7 7 0 0 0 14 0M12 17v4" />
        </svg>
      );
    case 'headphones':
      return (
        <svg {...p}>
          <path d="M4 14v-2a8 8 0 0 1 16 0v2" />
          <rect x="2" y="14" width="5" height="7" rx="2" />
          <rect x="17" y="14" width="5" height="7" rx="2" />
        </svg>
      );
    case 'speaker':
      return (
        <svg {...p}>
          <rect x="5" y="2" width="14" height="20" rx="3" />
          <circle cx="12" cy="14" r="4" />
          <circle cx="12" cy="6" r="1" />
        </svg>
      );
  }
}
