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

export type NodusEvent = 'audio-devices-changed' | 'process-changed' | 'volume-levels';

// ── Runtime detection + lazy Tauri API ──────────────────────────────────────

export const isTauri =
  typeof window !== 'undefined' && '__TAURI_IPC__' in (window as unknown as Record<string, unknown>);

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
  const fn = await getInvoke();
  if (!fn) return null;
  return (await fn(cmd, args)) as T;
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

/** Listen to any Tauri event by name. Returns an unsubscribe fn (no-op in browser). */
export async function listenAny<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  const listen = await getListen();
  if (!listen) return () => {};
  return listen(event, (e) => handler(e.payload as T));
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

/** Subscribe to a backend event. Returns an unsubscribe fn (no-op in browser). */
export async function listenToEvent<T>(
  event: NodusEvent,
  handler: (payload: T) => void,
): Promise<() => void> {
  const listen = await getListen();
  if (!listen) return () => {};
  return listen(event, (e) => handler(e.payload as T));
}
