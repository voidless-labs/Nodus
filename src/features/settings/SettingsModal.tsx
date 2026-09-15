import { useEffect, useState } from 'react';
import {
  getServerInfo,
  openExternal,
  openLogFolder,
  type LogRetention,
  type NodeStyle,
  type ServerInfo,
  type Settings,
} from '@/shared/bridge';
import pkg from '../../../package.json';
import './SettingsModal.css';

/** Accent presets — the default amber plus a few on-brand alternatives. */
const ACCENTS: { id: string; hex: string; name: string }[] = [
  { id: 'amber', hex: '#F5C542', name: 'Amber' },
  { id: 'green', hex: '#4ADE80', name: 'Green' },
  { id: 'blue', hex: '#38BDF8', name: 'Blue' },
  { id: 'purple', hex: '#A78BFA', name: 'Purple' },
  { id: 'pink', hex: '#F472B6', name: 'Pink' },
  { id: 'red', hex: '#FB7185', name: 'Red' },
  { id: 'teal', hex: '#2DD4BF', name: 'Teal' },
];

/** Node card styles — the card surface applied to every node on the canvas. */
const NODE_STYLES: { id: NodeStyle; name: string; hint: string }[] = [
  { id: 'primary', name: 'Primary', hint: 'Flat, strict — the default.' },
  { id: 'glass', name: 'Glass', hint: 'Translucent, blurred, accent bloom.' },
  { id: 'legacy', name: 'Legacy', hint: 'The pre-redesign solid card.' },
];

/** Log pruning modes (t35). Either way, the file being written is kept. */
const LOG_RETENTION: { id: LogRetention; name: string; hint: string }[] = [
  { id: 'files', name: 'By count', hint: 'Delete the oldest files beyond a number of files.' },
  { id: 'size', name: 'By size', hint: 'Delete the oldest files once the folder passes a limit.' },
];

const REPO_URL = 'https://github.com/voidless-labs/Nodus';

type TabId = 'appearance' | 'performance' | 'server' | 'behavior' | 'engine' | 'logs' | 'about';
const TABS: { id: TabId; label: string; icon: React.ReactNode }[] = [
  { id: 'appearance', label: 'Appearance', icon: <IconPalette /> },
  { id: 'performance', label: 'Performance', icon: <IconGauge /> },
  { id: 'server', label: 'Server', icon: <IconGlobe /> },
  { id: 'behavior', label: 'Behavior', icon: <IconSliders /> },
  { id: 'engine', label: 'Engine', icon: <IconWave /> },
  { id: 'logs', label: 'Logs', icon: <IconLog /> },
  { id: 'about', label: 'About', icon: <IconInfo /> },
];

/**
 * SettingsModal — application settings (t14), sidebar layout: categories on the
 * left, the selected category's controls on the right. Every control maps to a
 * real effect; settings live in the daemon (mirrored + persisted).
 */
export function SettingsModal({
  settings,
  onUpdate,
  onClose,
}: {
  settings: Settings;
  onUpdate: (patch: Partial<Settings>) => void;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<TabId>('appearance');
  const [server, setServer] = useState<ServerInfo | null>(null);
  useEffect(() => {
    let alive = true;
    getServerInfo().then((s) => alive && setServer(s));
    return () => {
      alive = false;
    };
  }, []);

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings" role="dialog" aria-label="Settings" onClick={(e) => e.stopPropagation()}>
        <nav className="settings-nav" aria-label="Settings sections">
          <div className="settings-nav-title">Settings</div>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={`settings-nav-item${tab === t.id ? ' is-active' : ''}`}
              aria-current={tab === t.id}
              onClick={() => setTab(t.id)}
            >
              <span className="settings-nav-icon">{t.icon}</span>
              {t.label}
            </button>
          ))}
        </nav>

        <div className="settings-content">
          <header className="settings-head">
            <h2 className="settings-title">{TABS.find((t) => t.id === tab)?.label}</h2>
            <button className="settings-x" aria-label="close" onClick={onClose}>
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
                <path d="M4 4l8 8M12 4l-8 8" />
              </svg>
            </button>
          </header>

          <div className="settings-pane">
            {tab === 'appearance' && (
              <>
                <Row label="Accent color" hint="Tints buttons, selection and meters.">
                  <div className="settings-swatches">
                    {ACCENTS.map((a) => (
                      <button
                        key={a.id}
                        type="button"
                        className={`settings-swatch${settings.accent === a.hex ? ' is-on' : ''}`}
                        style={{ background: a.hex }}
                        aria-label={a.name}
                        title={a.name}
                        onClick={() => onUpdate({ accent: a.hex })}
                      />
                    ))}
                  </div>
                </Row>
                <Row
                  label="Node style"
                  hint={NODE_STYLES.find((s) => s.id === settings.node_style)?.hint}
                >
                  <div className="settings-seg" role="group" aria-label="Node style">
                    {NODE_STYLES.map((s) => (
                      <button
                        key={s.id}
                        type="button"
                        className={`settings-seg-btn${settings.node_style === s.id ? ' is-on' : ''}`}
                        aria-pressed={settings.node_style === s.id}
                        onClick={() => onUpdate({ node_style: s.id })}
                      >
                        {s.name}
                      </button>
                    ))}
                  </div>
                </Row>
              </>
            )}

            {tab === 'performance' && (
              <>
                <Row label="Level meters" hint="Live VU meters. Off lowers CPU on weak machines.">
                  <Toggle checked={settings.vu_enabled} onChange={(v) => onUpdate({ vu_enabled: v })} />
                </Row>
                <Row label="Meter refresh" hint={`${settings.vu_fps} fps`}>
                  <input
                    type="range"
                    min={1}
                    max={60}
                    value={settings.vu_fps}
                    disabled={!settings.vu_enabled}
                    onChange={(e) => onUpdate({ vu_fps: Number(e.target.value) })}
                  />
                </Row>
                <Row label="App scan interval" hint={`every ${settings.process_scan_secs}s · applies on restart`}>
                  <input
                    type="range"
                    min={1}
                    max={30}
                    value={settings.process_scan_secs}
                    onChange={(e) => onUpdate({ process_scan_secs: Number(e.target.value) })}
                  />
                </Row>
              </>
            )}

            {tab === 'server' && (
              <>
                <p className="settings-note">Control Nodus from a browser. Changes apply after restart.</p>
                <Row label="Port" hint="Local daemon port">
                  <input
                    type="number"
                    className="settings-num"
                    min={1024}
                    max={65535}
                    value={settings.server_port}
                    onChange={(e) => onUpdate({ server_port: Number(e.target.value) })}
                  />
                </Row>
                <Row
                  label="Allow LAN access"
                  hint={settings.server_lan ? 'Reachable on your network — a token is required.' : 'Loopback only (this PC).'}
                >
                  <Toggle checked={settings.server_lan} onChange={(v) => onUpdate({ server_lan: v })} />
                </Row>
                {server && (
                  <div className="settings-server-info">
                    <div className="settings-kv">
                      <span>URL</span>
                      <code>{server.url}</code>
                    </div>
                    <div className="settings-kv">
                      <span>Token</span>
                      <code>{server.token ? server.token : 'none (dev)'}</code>
                    </div>
                  </div>
                )}
              </>
            )}

            {tab === 'behavior' && (
              <>
                <Row label="Start engine on launch" hint="Turn routing on automatically when Nodus opens.">
                  <Toggle checked={settings.start_engine_on_launch} onChange={(v) => onUpdate({ start_engine_on_launch: v })} />
                </Row>
                <Row label="Close to tray" hint="Closing the window hides it instead of quitting.">
                  <Toggle checked={settings.close_to_tray} onChange={(v) => onUpdate({ close_to_tray: v })} />
                </Row>
                <Row label="Start with Windows" hint="Launch Nodus when you sign in.">
                  <Toggle checked={settings.start_with_windows} onChange={(v) => onUpdate({ start_with_windows: v })} />
                </Row>
              </>
            )}

            {tab === 'engine' && (
              <p className="settings-note">
                The engine runs at 48&nbsp;kHz · stereo · 32-bit float (shared mode). Quality and
                latency tuning (resampler, exclusive mode) arrive with the audio-quality work.
              </p>
            )}

            {tab === 'logs' && (
              <>
                <p className="settings-note">
                  Diagnostics are written to dated files of up to 10&nbsp;MB each, with a new file
                  every day. The first line of each file shows the time span it covers. The file
                  being written is never deleted.
                </p>
                <Row
                  label="Keep logs"
                  hint={LOG_RETENTION.find((r) => r.id === settings.log_retention)?.hint}
                >
                  <div className="settings-seg" role="group" aria-label="Keep logs">
                    {LOG_RETENTION.map((r) => (
                      <button
                        key={r.id}
                        type="button"
                        className={`settings-seg-btn${settings.log_retention === r.id ? ' is-on' : ''}`}
                        aria-pressed={settings.log_retention === r.id}
                        onClick={() => onUpdate({ log_retention: r.id })}
                      >
                        {r.name}
                      </button>
                    ))}
                  </div>
                </Row>
                {settings.log_retention === 'size' ? (
                  <Row label="Folder limit" hint="MB · at least 10 · applied when you leave the field">
                    <CommitNumber
                      value={settings.log_max_mb}
                      min={10}
                      max={10000}
                      ariaLabel="Log folder limit in MB"
                      onCommit={(v) => onUpdate({ log_max_mb: v })}
                    />
                  </Row>
                ) : (
                  <Row label="Files to keep" hint="Including the current one · applied when you leave the field">
                    <CommitNumber
                      value={settings.log_max_files}
                      min={1}
                      max={100}
                      ariaLabel="Log files to keep"
                      onCommit={(v) => onUpdate({ log_max_files: v })}
                    />
                  </Row>
                )}
                <button
                  className="settings-link"
                  onClick={() => void openLogFolder().catch((e) => console.error('open_log_folder:', e))}
                >
                  Open log folder →
                </button>
              </>
            )}

            {tab === 'about' && (
              <>
                <Row label="Nodus" hint="Node-based virtual audio router">
                  <span className="settings-version">v{pkg.version}</span>
                </Row>
                <p className="settings-note">Free software — donations welcome, never required.</p>
                <button className="settings-link" onClick={() => openExternal(REPO_URL)}>
                  Open the project on GitHub →
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="settings-row">
      <div className="settings-row-label">
        <span className="settings-row-name">{label}</span>
        {hint && <span className="settings-row-hint">{hint}</span>}
      </div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      className={`settings-toggle${checked ? ' is-on' : ''}`}
      onClick={() => onChange(!checked)}
    >
      <span className="settings-toggle-knob" />
    </button>
  );
}

/**
 * A number field that commits on blur or Enter, not on every keystroke. For settings
 * with an immediate effect that can't be undone: typing "15" into "files to keep"
 * must not pass through "1" and delete four log files on the way. (t35)
 */
function CommitNumber({
  value,
  min,
  max,
  ariaLabel,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  ariaLabel: string;
  onCommit: (v: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const parsed = Math.round(Number(draft));
    const next = draft.trim() === '' || !Number.isFinite(parsed) ? value : Math.min(max, Math.max(min, parsed));
    setDraft(String(next));
    if (next !== value) onCommit(next);
  };
  return (
    <input
      type="number"
      className="settings-num"
      min={min}
      max={max}
      value={draft}
      aria-label={ariaLabel}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
      }}
    />
  );
}

/* ── Tab icons ──────────────────────────────────────────────────────────── */
function IconPalette() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="13.5" cy="6.5" r="1" /><circle cx="17.5" cy="10.5" r="1" /><circle cx="8.5" cy="7.5" r="1" /><circle cx="6.5" cy="12.5" r="1" />
      <path d="M12 2a10 10 0 0 0 0 20c1.1 0 2-.9 2-2 0-.5-.2-1-.5-1.3-.3-.4-.5-.8-.5-1.2 0-1 .8-1.8 1.8-1.8H16a4 4 0 0 0 4-4c0-5-3.6-9-8-9z" />
    </svg>
  );
}
function IconGauge() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 14l4-4" /><path d="M3.3 18a9 9 0 1 1 17.4 0" />
    </svg>
  );
}
function IconGlobe() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18z" />
    </svg>
  );
}
function IconSliders() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M18 18h2" /><circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle cx="16" cy="18" r="2" />
    </svg>
  );
}
function IconWave() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 12h3l2-7 4 18 3-13 2 6h6" />
    </svg>
  );
}
function IconInfo() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" /><path d="M12 11v5M12 8h.01" />
    </svg>
  );
}
function IconLog() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 13h6M9 17h4" />
    </svg>
  );
}
