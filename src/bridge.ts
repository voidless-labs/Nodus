/**
 * bridge.ts — typed Tauri bridge (redesign R3).
 *
 * Wraps the Rust `invoke` commands and events with the exact types from the
 * backend (commands/bridge.rs, audio/devices.rs, detection/process.rs,
 * routing/node.rs). Falls back gracefully when there is no Tauri runtime
 * (plain Vite preview): commands resolve to null/empty and listeners are no-ops,
 * so the UI runs in the browser with sample data.
 *
 * This file is the single contract between the React UI and the Rust engine —
 * changing a command/event here must match the Rust side.
 */

// ── Backend types (mirror the Rust serde shapes) ────────────────────────────

export type DeviceType = 'input' | 'output' | 'virtual';

export interface AudioDevice {
  id: string;
  name: string;
  /** Data flow: input = capture, output = render (drives port direction). */
  device_type: DeviceType;
  is_default: boolean;
  original_name?: string | null;
  /** Software/virtual device (VB-Cable, VoiceMeeter, MIXLINE, Nodus…) — t9. */
  is_virtual?: boolean;
}

export type SourceType =
  | 'game'
  | 'chat'
  | 'voice'
  | 'music'
  | 'browser'
  | 'recording'
  | 'system'
  | 'unknown';

export interface AudioProcess {
  exe_name: string;
  pid: number;
  display_name: string;
  source_type: SourceType;
  /** App icon extracted from the .exe as a PNG data URL (R7). null if unavailable. */
  icon?: string | null;
}

export type BackendNodeType = 'source' | 'output' | 'splitter' | 'mixer' | 'virtual';

export interface BackendNode {
  id: string;
  node_type: BackendNodeType;
  label: string;
  device_id: string;
  exe_name?: string | null;
}

export interface BackendRoute {
  id: string;
  from_node: string;
  to_node: string;
  volume: number; // 0..1
  muted: boolean;
  pan: number; // -1..1
}

export interface RoutingGraph {
  nodes: BackendNode[];
  routes: BackendRoute[];
}

export interface VirtualSetupStatus {
  kind: 'not_found' | 'vb_audio' | 'nodus_driver';
  endpoints: string[];
  message: string;
}

/** volume-levels payload: keyed by device id OR exe name → level 0..1. */
export type VolumeLevels = Record<string, number>;

export type NodusEvent =
  | 'audio-devices-changed'
  | 'process-changed'
  | 'volume-levels'
  | 'engine-state';

// ── Runtime detection + lazy Tauri API ──────────────────────────────────────

export const isTauri =
  typeof window !== 'undefined' && '__TAURI_IPC__' in (window as unknown as Record<string, unknown>);

// ── Web daemon transport (t17 phase A) ──────────────────────────────────────
// When NOT running inside Tauri, the UI can still drive a real engine by talking
// to the embedded Nodus daemon over HTTP (/rpc) + WebSocket (/ws). The daemon
// address is taken from (in order) the URL `?daemon=` param, the VITE_NODUS_DAEMON
// env var, or localStorage 'nodus.daemon'. The token comes from `?token=`,
// VITE_NODUS_TOKEN, or localStorage 'nodus.token'. Accepts `host:port`,
// `http://host:port`, or `ws://host:port`. No daemon configured → the old browser
// behaviour (commands resolve to null, listeners are no-ops, sample data).

interface DaemonCfg {
  http: string;
  ws: string;
  token: string;
}

function readDaemonCfg(): DaemonCfg | null {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(window.location.search);
  const env = (import.meta as unknown as { env?: Record<string, string> }).env ?? {};
  const ls = (key: string) => {
    try {
      return typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
    } catch {
      return null;
    }
  };
  const raw = params.get('daemon') || env.VITE_NODUS_DAEMON || ls('nodus.daemon') || '';
  if (!raw) return null;
  const secure = /^(https|wss):/i.test(raw);
  const host = raw
    .replace(/^wss?:\/\//i, '')
    .replace(/^https?:\/\//i, '')
    .replace(/\/+$/, '');
  const token = params.get('token') || env.VITE_NODUS_TOKEN || ls('nodus.token') || '';
  return {
    http: `${secure ? 'https' : 'http'}://${host}`,
    ws: `${secure ? 'wss' : 'ws'}://${host}`,
    token,
  };
}

const _daemon: DaemonCfg | null = !isTauri ? readDaemonCfg() : null;
/** True when this browser session is wired to a live Nodus daemon (t17). */
export const isDaemon = _daemon !== null;

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
type ListenFn = (
  event: string,
  handler: (e: { payload: unknown }) => void,
) => Promise<() => void>;

let _invoke: InvokeFn | null = null;
let _listen: ListenFn | null = null;

async function getInvoke(): Promise<InvokeFn | null> {
  if (!isTauri) return null;
  if (!_invoke) {
    const mod = await import('@tauri-apps/api/tauri');
    _invoke = mod.invoke as InvokeFn;
  }
  return _invoke;
}

async function getListen(): Promise<ListenFn | null> {
  if (!isTauri) return null;
  if (!_listen) {
    const mod = await import('@tauri-apps/api/event');
    _listen = mod.listen as unknown as ListenFn;
  }
  return _listen;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (isTauri) {
    const fn = await getInvoke();
    if (!fn) return null;
    return (await fn(cmd, args)) as T;
  }
  if (_daemon) {
    const url = `${_daemon.http}/rpc${_daemon.token ? `?token=${encodeURIComponent(_daemon.token)}` : ''}`;
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ cmd, args: args ?? {} }),
    });
    if (!res.ok) throw new Error(`rpc ${cmd}: HTTP ${res.status}`);
    const data = (await res.json()) as { ok?: boolean; result?: unknown; error?: string };
    if (data.ok === false) throw new Error(data.error || `rpc ${cmd} failed`);
    return (data.result ?? null) as T;
  }
  return null;
}

// ── Daemon WebSocket: a single shared connection fanned out to all listeners ──

type WsHandler = (payload: unknown) => void;
const _wsHandlers = new Map<string, Set<WsHandler>>();
let _ws: WebSocket | null = null;
let _wsRetry: ReturnType<typeof setTimeout> | null = null;

function ensureWs(): void {
  if (!_daemon || _ws) return;
  const url = `${_daemon.ws}/ws${_daemon.token ? `?token=${encodeURIComponent(_daemon.token)}` : ''}`;
  try {
    const ws = new WebSocket(url);
    _ws = ws;
    ws.onmessage = (ev) => {
      try {
        const { event, payload } = JSON.parse(ev.data as string) as {
          event: string;
          payload: unknown;
        };
        _wsHandlers.get(event)?.forEach((h) => h(payload));
      } catch {
        /* ignore malformed frame */
      }
    };
    ws.onclose = () => {
      _ws = null;
      if (_wsHandlers.size) scheduleWsRetry();
    };
    ws.onerror = () => {
      try {
        ws.close();
      } catch {
        /* noop */
      }
    };
  } catch {
    _ws = null;
    scheduleWsRetry();
  }
}

function scheduleWsRetry(): void {
  if (_wsRetry) return;
  _wsRetry = setTimeout(() => {
    _wsRetry = null;
    ensureWs();
  }, 1500);
}

function daemonListen(event: string, handler: WsHandler): () => void {
  let set = _wsHandlers.get(event);
  if (!set) {
    set = new Set();
    _wsHandlers.set(event, set);
  }
  set.add(handler);
  ensureWs();
  return () => {
    const s = _wsHandlers.get(event);
    if (!s) return;
    s.delete(handler);
    if (!s.size) _wsHandlers.delete(event);
  };
}

// ── Generic event channel (used by the quick-controls flyout ↔ main, t13) ────

type EmitFn = (event: string, payload?: unknown) => Promise<void>;
let _emit: EmitFn | null = null;

/** Emit a Tauri event to all windows. No-op in the browser. */
export async function emitEvent(event: string, payload?: unknown): Promise<void> {
  if (!isTauri) return;
  if (!_emit) {
    const mod = await import('@tauri-apps/api/event');
    _emit = mod.emit as unknown as EmitFn;
  }
  await _emit(event, payload);
}

/** Listen to any event by name. Returns an unsubscribe fn (no-op when offline). */
export async function listenAny<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (isTauri) {
    const listen = await getListen();
    if (!listen) return () => {};
    return listen(event, (e) => handler(e.payload as T));
  }
  if (_daemon) return daemonListen(event, (p) => handler(p as T));
  return () => {};
}

/** Which Tauri window this document is (by ?w= query). 'quick' = the flyout. */
export function windowKind(): 'main' | 'quick' {
  if (typeof window === 'undefined') return 'main';
  return new URLSearchParams(window.location.search).get('w') === 'quick' ? 'quick' : 'main';
}

// ── Commands ────────────────────────────────────────────────────────────────

export async function getAudioDevices(): Promise<AudioDevice[]> {
  return (await call<AudioDevice[]>('get_audio_devices')) ?? [];
}

export async function getRunningAudioProcesses(): Promise<AudioProcess[]> {
  return (await call<AudioProcess[]>('get_running_audio_processes')) ?? [];
}

export function applyRoutingGraph(graph: RoutingGraph): Promise<unknown> {
  return call('apply_routing_graph', { graph });
}

// Tauri 1.x maps JS camelCase invoke args to snake_case Rust params, so the key
// MUST be `routeId` (not `route_id`) or the command's `route_id` arg is missing
// and the invoke is silently rejected. We send both keys to be safe. (t16 fix)
export function setRouteMute(routeId: string, muted: boolean): Promise<unknown> {
  return call('set_route_mute', { routeId, route_id: routeId, muted });
}

export function setRouteVolume(routeId: string, volume: number): Promise<unknown> {
  return call('set_route_volume', { routeId, route_id: routeId, volume });
}

export function setRoutePan(routeId: string, pan: number): Promise<unknown> {
  return call('set_route_pan', { routeId, route_id: routeId, pan });
}

export function startEngine(): Promise<unknown> {
  return call('start_engine');
}

export function stopEngine(): Promise<unknown> {
  return call('stop_engine');
}

/** Whether the shared engine is currently running (read on mount; live updates
 *  arrive via the `engine-state` event). False when there is no live backend. */
export async function isEngineRunning(): Promise<boolean> {
  return (await call<boolean>('is_engine_running')) ?? false;
}

export async function getVirtualSetupStatus(): Promise<VirtualSetupStatus> {
  return (
    (await call<VirtualSetupStatus>('get_virtual_setup_status')) ?? {
      kind: 'not_found',
      endpoints: [],
      message: '',
    }
  );
}

export function installVbcable(): Promise<unknown> {
  return call('install_vbcable');
}

export async function isTestSigningEnabled(): Promise<boolean> {
  return (await call<boolean>('is_test_signing_enabled')) ?? false;
}

/** Pin/unpin the tray flyout (when pinned it stays open on focus loss). */
export function setFlyoutPinned(pinned: boolean): Promise<unknown> {
  return call('set_flyout_pinned', { pinned });
}

/** Reveal the main window from Rust (the flyout's "Dashboard") — works even when
 *  the main window is hidden to the tray (its own JS is throttled while hidden). */
export function showMainWindow(): Promise<unknown> {
  return call('show_main_window');
}

/** Daemon URL + token to open the Web-UI / hand to Claude-preview (t17). Tauri only. */
export interface ServerInfo {
  url: string;
  token: string;
}
export async function getServerInfo(): Promise<ServerInfo | null> {
  if (!isTauri) return null;
  return call<ServerInfo>('get_server_info');
}

// ── Scene sync (t17 phase B) ─────────────────────────────────────────────────
// The workspace document `{ tabs, activeId }` lives in the daemon as the single
// source of truth. A client pushes the whole document after a local mutation;
// the daemon persists + broadcasts `scene:snapshot` so the other UI mirrors it.

/** The daemon's workspace document + monotonic revision (+ origin on broadcasts). */
export interface SceneSnapshot {
  doc: unknown;
  rev: number;
  origin?: string | null;
}

/** Pull the current workspace document (null when there is no live backend). */
export async function getScene(): Promise<SceneSnapshot | null> {
  if (!isTauri && !_daemon) return null;
  return call<SceneSnapshot>('get_scene');
}

/** Push the whole workspace document; returns the new revision (or null offline). */
export async function pushScene(doc: unknown, origin: string): Promise<number | null> {
  if (!isTauri && !_daemon) return null;
  const r = await call<{ rev: number } | number>('push_scene', { doc, origin });
  if (r == null) return null;
  return typeof r === 'number' ? r : r.rev;
}

// ── Settings (t14) ────────────────────────────────────────────────────────────
// Mirrored + persisted in the daemon like the scene; field names match the Rust
// serde shape (snake_case). Broadcast as `settings:changed`.

export interface Settings {
  // performance (live)
  vu_enabled: boolean;
  vu_fps: number;
  process_scan_secs: number;
  // server / browser access (applied at next launch)
  server_port: number;
  server_lan: boolean;
  // app behavior
  start_engine_on_launch: boolean;
  close_to_tray: boolean;
  start_with_windows: boolean;
}

/** Defaults mirroring Rust `Settings::default()` — used before hydrate / offline. */
export const DEFAULT_SETTINGS: Settings = {
  vu_enabled: true,
  vu_fps: 15,
  process_scan_secs: 2,
  server_port: 7878,
  server_lan: false,
  start_engine_on_launch: false,
  close_to_tray: true,
  start_with_windows: false,
};

export async function getSettings(): Promise<Settings | null> {
  if (!isTauri && !_daemon) return null;
  return call<Settings>('get_settings');
}

/** Replace settings; persists + broadcasts + applies side effects. Returns normalised. */
export async function setSettings(next: Settings): Promise<Settings | null> {
  if (!isTauri && !_daemon) return null;
  return call<Settings>('set_settings', { next });
}

// ── Window controls (custom title bar) ──────────────────────────────────────
// Lazy import of @tauri-apps/api/window so the plain Vite preview (no Tauri)
// never loads it; every control is a no-op in the browser.

type AppWindow = {
  minimize: () => Promise<void>;
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
  hide: () => Promise<void>;
  show: () => Promise<void>;
  unminimize: () => Promise<void>;
  setFocus: () => Promise<void>;
  isMaximized: () => Promise<boolean>;
  onResized: (cb: () => void) => Promise<() => void>;
};

let _appWindow: AppWindow | null = null;

async function getWindow(): Promise<AppWindow | null> {
  if (!isTauri) return null;
  if (!_appWindow) {
    const mod = await import('@tauri-apps/api/window');
    _appWindow = mod.appWindow as unknown as AppWindow;
  }
  return _appWindow;
}

export async function winMinimize(): Promise<void> {
  await (await getWindow())?.minimize();
}
export async function winToggleMaximize(): Promise<void> {
  await (await getWindow())?.toggleMaximize();
}
export async function winClose(): Promise<void> {
  await (await getWindow())?.close();
}
/** Hide this window (used by the tray flyout's close — never destroys it). */
export async function winHide(): Promise<void> {
  await (await getWindow())?.hide();
}
/** Reveal this window from any state (hidden to tray OR minimized to taskbar)
 *  and bring it to front — the flyout's "Dashboard" must work every time. */
export async function winShow(): Promise<void> {
  const w = await getWindow();
  if (!w) return;
  // Independent + order-safe: show() un-hides from the tray; unminimize()
  // restores from the taskbar; setFocus() raises. Each is guarded so one
  // failing (e.g. unminimize on a non-minimized window) can't abort the rest.
  await w.show().catch(() => {});
  await w.unminimize().catch(() => {});
  await w.setFocus().catch(() => {});
}
export async function winIsMaximized(): Promise<boolean> {
  return (await (await getWindow())?.isMaximized()) ?? false;
}
/** Subscribe to window resize (to refresh the maximize/restore icon). No-op in browser. */
export async function onWinResize(cb: () => void): Promise<() => void> {
  const w = await getWindow();
  if (!w) return () => {};
  return w.onResized(cb);
}

// ── Events ──────────────────────────────────────────────────────────────────

/** Subscribe to a backend event. Returns an unsubscribe fn (no-op when offline). */
export async function listenToEvent<T>(
  event: NodusEvent,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (isTauri) {
    const listen = await getListen();
    if (!listen) return () => {};
    return listen(event, (e) => handler(e.payload as T));
  }
  if (_daemon) return daemonListen(event, (p) => handler(p as T));
  return () => {};
}
