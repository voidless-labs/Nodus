import { useEffect, useState } from 'react';
import { getServerInfo, type Settings, type ServerInfo } from '../bridge';
import './SettingsModal.css';

/**
 * SettingsModal — application settings (t14).
 *
 * Grouped into Performance, Server / browser access, App behavior, and an
 * (informational) Engine & audio section. Every control maps to a real effect:
 * performance + behavior apply live; server options apply on the next launch
 * (clearly labelled). Settings live in the daemon (mirrored + persisted), so a
 * change here shows in the other window too.
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
      <div
        className="settings"
        role="dialog"
        aria-label="Settings"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="settings-head">
          <h2 className="settings-title">Settings</h2>
          <button className="settings-x" aria-label="close" onClick={onClose}>
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
              <path d="M4 4l8 8M12 4l-8 8" />
            </svg>
          </button>
        </header>

        <div className="settings-body">
          {/* ── Performance ─────────────────────────────────────────── */}
          <section className="settings-group">
            <h3 className="settings-group-title">Performance</h3>
            <Row label="Level meters" hint="Live VU meters. Off lowers CPU on weak machines.">
              <Toggle
                checked={settings.vu_enabled}
                onChange={(v) => onUpdate({ vu_enabled: v })}
              />
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
          </section>

          {/* ── Server / browser access ─────────────────────────────── */}
          <section className="settings-group">
            <h3 className="settings-group-title">Server &amp; browser access</h3>
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
              <Toggle
                checked={settings.server_lan}
                onChange={(v) => onUpdate({ server_lan: v })}
              />
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
          </section>

          {/* ── App behavior ────────────────────────────────────────── */}
          <section className="settings-group">
            <h3 className="settings-group-title">App behavior</h3>
            <Row label="Start engine on launch" hint="Turn routing on automatically when Nodus opens.">
              <Toggle
                checked={settings.start_engine_on_launch}
                onChange={(v) => onUpdate({ start_engine_on_launch: v })}
              />
            </Row>
            <Row label="Close to tray" hint="Closing the window hides it instead of quitting.">
              <Toggle
                checked={settings.close_to_tray}
                onChange={(v) => onUpdate({ close_to_tray: v })}
              />
            </Row>
            <Row label="Start with Windows" hint="Launch Nodus when you sign in.">
              <Toggle
                checked={settings.start_with_windows}
                onChange={(v) => onUpdate({ start_with_windows: v })}
              />
            </Row>
          </section>

          {/* ── Engine & audio (informational for now) ──────────────── */}
          <section className="settings-group">
            <h3 className="settings-group-title">Engine &amp; audio</h3>
            <p className="settings-note">
              The engine runs at 48&nbsp;kHz · stereo · 32-bit float (shared mode). Quality and
              latency tuning (resampler, exclusive mode) arrive with the audio-quality work.
            </p>
          </section>
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
