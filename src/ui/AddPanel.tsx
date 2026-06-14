import { useMemo, useState } from 'react';
import './AddPanel.css';

/**
 * AddPanel — the "+ add" launcher at bottom-left (R13).
 *
 * Collapsed: a quiet "+ add" pill. Expanded: a panel growing upward with a
 * search field over two sections — "playing now" (auto-detected apps) and
 * "devices" (system audio devices). Picking a row drops a node on the canvas.
 *
 * Visual only for now: the lists are sample data and onPick just closes the
 * panel; real process/device data and node creation are wired in R3/R18.
 */
type AddItem = { id: string; name: string; sub: string; kind: 'app' | 'device'; avatar?: string };

const SAMPLE_APPS: AddItem[] = [
  { id: 'spotify', name: 'Spotify', sub: 'music app', kind: 'app', avatar: 'S' },
  { id: 'discord', name: 'Discord', sub: 'voice app', kind: 'app', avatar: 'D' },
  { id: 'chrome', name: 'Chrome', sub: 'browser', kind: 'app', avatar: 'C' },
];

const SAMPLE_DEVICES: AddItem[] = [
  { id: 'headphones', name: 'Headphones', sub: 'Galaxy Buds', kind: 'device' },
  { id: 'speakers', name: 'Speakers', sub: 'Realtek', kind: 'device' },
  { id: 'mic', name: 'Microphone', sub: 'Shure MV7', kind: 'device' },
  { id: 'nodusmic', name: 'Nodus Mic', sub: 'virtual · to Discord', kind: 'device' },
];

export function AddPanel() {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');

  const [apps, devices] = useMemo(() => {
    const f = (xs: AddItem[]) =>
      q.trim() ? xs.filter((x) => x.name.toLowerCase().includes(q.trim().toLowerCase())) : xs;
    return [f(SAMPLE_APPS), f(SAMPLE_DEVICES)];
  }, [q]);

  const pick = () => {
    // R3/R18: create the node on the canvas. For now, just close.
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
                placeholder="add to canvas · search apps & devices"
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

        <button className="ap-toggle" onClick={() => setOpen((v) => !v)}>
          <span className="ap-toggle-plus">+</span>
          add
        </button>
      </div>
    </>
  );
}

function AddRow({ item, onClick }: { item: AddItem; onClick: () => void }) {
  return (
    <button className="ap-row" onClick={onClick}>
      <span className="ap-row-icon">
        {item.kind === 'app' ? (
          <span className="ap-row-letter">{item.avatar ?? '?'}</span>
        ) : (
          <DeviceGlyph />
        )}
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
  width: 15,
  height: 15,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
function SearchIcon() {
  return (
    <svg {...IC}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}
function DeviceGlyph() {
  return (
    <svg {...IC} width={14} height={14}>
      <rect x="4" y="3" width="16" height="13" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </svg>
  );
}
