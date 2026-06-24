import { useEffect, useState } from 'react';
import { emitEvent, listenAny, setFlyoutPinned, showMainWindow } from './bridge';
import { Topbar } from './ui/Topbar';
import { EngineButton } from './ui/EngineButton';
import { QuickList, type QuickItem } from './ui/QuickPanel';
import './FlyoutApp.css';

/**
 * FlyoutApp — the tray flyout window's content (t13, Phase B2, variant A).
 *
 * Runs in a SEPARATE Tauri window (`?w=quick`) with no access to the main
 * window's store, so it mirrors the main window over Tauri events:
 * - listens for `quick:snapshot` (pinned items, scenes, engine live state);
 * - on mount asks the main window for a fresh snapshot (`quick:request`);
 * - every action is emitted as `quick:cmd` for the main window to apply,
 *   keeping the canvas + engine the single source of truth.
 *
 * Visually it is a slice of the real app: the shared Topbar (brand + scene
 * switcher, no window buttons), the floating EngineButton, the pinned list, and
 * a Dashboard button that reveals the full app window.
 */
interface Snapshot {
  live: boolean;
  items: QuickItem[];
  scenes: { id: string; name: string }[];
  activeId: string;
  liveSince: number | null;
}

function formatUptime(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${m}:${pad(sec)}`;
}

export default function FlyoutApp() {
  const [snap, setSnap] = useState<Snapshot>({
    live: false,
    items: [],
    scenes: [],
    activeId: '',
    liveSince: null,
  });
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    let unlisten = () => {};
    let alive = true;
    listenAny<Snapshot>('quick:snapshot', (s) => alive && setSnap(s)).then((fn) => {
      if (alive) unlisten = fn;
      else fn();
    });
    void emitEvent('quick:request');
    return () => {
      alive = false;
      unlisten();
    };
  }, []);

  // Tick the uptime clock once a second, only while the engine is live.
  useEffect(() => {
    if (snap.liveSince == null) return;
    const iv = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(iv);
  }, [snap.liveSince]);

  // Pin: keep the flyout open on focus loss (and draggable by its topbar).
  const [pinnedOpen, setPinnedOpen] = useState(false);
  const togglePin = () => {
    const next = !pinnedOpen;
    setPinnedOpen(next);
    void setFlyoutPinned(next);
  };

  const cmd = (payload: Record<string, unknown>) => void emitEvent('quick:cmd', payload);
  const uptime = snap.liveSince != null ? formatUptime(now - snap.liveSince) : null;

  return (
    <div className="flyout">
      <button
        className={`flyout-pin ${pinnedOpen ? 'is-on' : ''}`}
        title={pinnedOpen ? 'unpin (allow auto-hide)' : 'pin open (drag by the topbar)'}
        aria-label="pin open"
        aria-pressed={pinnedOpen}
        onClick={togglePin}
      >
        <PinIcon />
      </button>
      <Topbar
        scenes={snap.scenes.length ? snap.scenes : [{ id: 'scene-1', name: 'Scene 1' }]}
        activeId={snap.activeId}
        onSwitch={(id) => cmd({ type: 'sceneSwitch', id })}
        onAdd={() => cmd({ type: 'sceneAdd' })}
        onClose={(id) => cmd({ type: 'sceneClose', id })}
        onRename={(id, name) => cmd({ type: 'sceneRename', id, name })}
        windowControls={false}
        sceneEditing={false}
      />

      <div className="flyout-body">
        <EngineButton live={snap.live} onToggleLive={() => cmd({ type: 'toggleLive' })} />
        <div className="flyout-scroll">
          <QuickList
            items={snap.items}
            onNodeVolume={(id, value) => cmd({ type: 'nodeVolume', id, value })}
            onNodeMute={(id) => cmd({ type: 'nodeMute', id })}
            onNodeSolo={(id) => cmd({ type: 'nodeSolo', id })}
            onHubInputVolume={(id, inputId, value) =>
              cmd({ type: 'hubInputVolume', id, inputId, value })
            }
            onUnpin={(id) => cmd({ type: 'unpin', id })}
          />
        </div>
      </div>

      <div className="flyout-foot">
        <button
          className="flyout-dash"
          title="open the full Nodus window"
          onClick={() => void showMainWindow()}
        >
          <DashboardIcon />
        </button>
        <span className={`flyout-uptime ${uptime ? 'is-live' : ''}`}>
          {uptime ? (
            <>
              <Dot /> {uptime}
            </>
          ) : (
            'engine off'
          )}
        </span>
      </div>
    </div>
  );
}

function Dot() {
  return <span className="flyout-uptime-dot" aria-hidden />;
}

function PinIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 17v5M9 3h6l-1 6 3 3v1H7v-1l3-3-1-6Z" />
    </svg>
  );
}
function DashboardIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="18" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </svg>
  );
}
