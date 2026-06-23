import { useMemo, useState } from 'react';
import './AddPanel.css';
import type { AudioDevice, AudioProcess, SourceType } from '../bridge';

/**
 * AddPanel — the "+ add" launcher at bottom-left (R13), an accent element.
 *
 * Collapsed: a prominent "+ add" pill with the node/route counts to its right.
 * Expanded: a panel growing upward with a search field over two sections —
 * "playing now" (real detected apps) and "devices" (real audio devices), each a
 * colored round type icon + name + subtype. Data comes from useBackend (R3);
 * picking a row will create the node on the canvas in R18.
 */
type Glyph = 'music' | 'game' | 'mic' | 'headphones' | 'speaker';
type TypeColor = 'source' | 'output' | 'virtual';
type AddItem = {
  id: string;
  name: string;
  sub: string;
  glyph: Glyph;
  color: TypeColor;
  icon?: string | null;
  device?: AudioDevice;
  process?: AudioProcess;
};

function processGlyph(t: SourceType): Glyph {
  if (t === 'music') return 'music';
  if (t === 'game') return 'game';
  return 'mic';
}

function processToItem(p: AudioProcess): AddItem {
  return {
    id: `proc:${p.exe_name}`,
    name: p.display_name,
    sub: p.source_type === 'unknown' ? 'app' : p.source_type,
    glyph: processGlyph(p.source_type),
    color: 'source',
    icon: p.icon,
    process: p,
  };
}

function deviceToItem(d: AudioDevice): AddItem {
  const low = d.name.toLowerCase();
  let glyph: Glyph;
  let color: TypeColor;
  if (d.device_type === 'input') {
    glyph = 'mic';
    color = 'source';
  } else if (d.device_type === 'virtual') {
    glyph = low.includes('mic') ? 'mic' : 'speaker';
    color = 'virtual';
  } else {
    glyph = low.includes('headphone') || low.includes('buds') ? 'headphones' : 'speaker';
    color = 'output';
  }
  // Strip the trailing "(System name)" for a cleaner primary label.
  const name = d.name.replace(/\s*\([^)]*\)\s*$/, '').trim() || d.name;
  const sub = d.original_name ?? d.name.match(/\(([^)]*)\)\s*$/)?.[1] ?? d.device_type;
  return { id: `dev:${d.id}`, name, sub, glyph, color, device: d };
}

export function AddPanel({
  nodes,
  routes,
  devices: rawDevices,
  processes,
  onAddDevice,
  onAddProcess,
}: {
  nodes: number;
  routes: number;
  devices: AudioDevice[];
  processes: AudioProcess[];
  onAddDevice?: (d: AudioDevice) => void;
  onAddProcess?: (p: AudioProcess) => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');

  const [apps, devices] = useMemo(() => {
    const t = q.trim().toLowerCase();
    const f = (xs: AddItem[]) => (t ? xs.filter((x) => x.name.toLowerCase().includes(t)) : xs);
    return [f(processes.map(processToItem)), f(rawDevices.map(deviceToItem))];
  }, [q, processes, rawDevices]);

  const pick = (item: AddItem) => {
    if (item.device) onAddDevice?.(item.device);
    else if (item.process) onAddProcess?.(item.process);
    setOpen(false);
    setQ('');
  };

  // Drag a row onto the canvas to place the node exactly where you drop it.
  // Close the panel so the canvas becomes the drop target (the drag image is
  // already captured by the browser, so it survives the unmount).
  const onRowDragStart = (e: React.DragEvent, item: AddItem) => {
    const payload = item.device
      ? { kind: 'device', id: item.device.id }
      : item.process
        ? { kind: 'process', id: item.process.exe_name }
        : null;
    if (!payload) return;
    e.dataTransfer.setData('application/nodus-add', JSON.stringify(payload));
    e.dataTransfer.effectAllowed = 'copy';
    setOpen(false);
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
                    <AddRow
                      key={it.id}
                      item={it}
                      onClick={() => pick(it)}
                      onDragStart={(e) => onRowDragStart(e, it)}
                    />
                  ))}
                </div>
              )}
              {devices.length > 0 && (
                <div className="ap-section">
                  <div className="ap-section-h">devices</div>
                  {devices.map((it) => (
                    <AddRow
                      key={it.id}
                      item={it}
                      onClick={() => pick(it)}
                      onDragStart={(e) => onRowDragStart(e, it)}
                    />
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

function AddRow({
  item,
  onClick,
  onDragStart,
}: {
  item: AddItem;
  onClick: () => void;
  onDragStart: (e: React.DragEvent) => void;
}) {
  return (
    <button
      className="ap-row"
      draggable
      onClick={onClick}
      onDragStart={onDragStart}
      style={{ ['--c' as string]: `var(--color-type-${item.color})` }}
    >
      <span className="ap-row-icon">
        {item.icon ? (
          <img className="ap-row-icon-img" src={item.icon} alt="" draggable={false} />
        ) : (
          <Glyph kind={item.glyph} />
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
