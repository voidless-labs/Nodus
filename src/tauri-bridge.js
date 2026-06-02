// Tauri bridge — wraps invoke/listen, falls back gracefully in browser-only mode.
const isTauri = typeof window !== 'undefined' && !!window.__TAURI_IPC__;

let _invoke = null;
let _listen = null;

async function _getInvoke() {
  if (!isTauri) return null;
  if (!_invoke) {
    const mod = await import('@tauri-apps/api/tauri');
    _invoke = mod.invoke;
  }
  return _invoke;
}

async function _getListen() {
  if (!isTauri) return null;
  if (!_listen) {
    const mod = await import('@tauri-apps/api/event');
    _listen = mod.listen;
  }
  return _listen;
}

async function _invoke_cmd(cmd, args) {
  const fn = await _getInvoke();
  if (!fn) return null;
  return fn(cmd, args);
}

export async function getAudioDevices() {
  const result = await _invoke_cmd('get_audio_devices');
  return result ?? [];
}

export async function getRunningAudioProcesses() {
  const result = await _invoke_cmd('get_running_audio_processes');
  return result ?? [];
}

export async function applyRoutingGraph(graph) {
  return _invoke_cmd('apply_routing_graph', { graph });
}

export async function setRouteMute(routeId, muted) {
  return _invoke_cmd('set_route_mute', { route_id: routeId, muted });
}

export async function setRouteVolume(routeId, volume) {
  return _invoke_cmd('set_route_volume', { route_id: routeId, volume });
}

export async function startEngine() {
  return _invoke_cmd('start_engine');
}

export async function stopEngine() {
  return _invoke_cmd('stop_engine');
}

export async function listenToEvent(event, handler) {
  const listen = await _getListen();
  if (!listen) return () => {};
  return listen(event, (e) => handler(e.payload));
}

// ── Virtual device setup ─────────────────────────────────────────────────────

/**
 * Returns { kind: 'not_found' | 'vb_audio' | 'nodus_driver', endpoints: [...], message: '' }
 * UI uses this on startup to decide whether to show the setup dialog.
 */
export async function getVirtualSetupStatus() {
  const result = await _invoke_cmd('get_virtual_setup_status');
  return result ?? { kind: 'not_found', endpoints: [], message: '' };
}

/**
 * Download + install VB-Audio VBCABLE (Windows UAC prompt will appear).
 * Poll getVirtualSetupStatus() + getAudioDevices() afterward to confirm.
 */
export async function installVbcable() {
  return _invoke_cmd('install_vbcable');
}

/**
 * Returns true if Windows test signing mode is currently enabled.
 * Used by the setup dialog to show the Test Mode option state.
 */
export async function isTestSigningEnabled() {
  const result = await _invoke_cmd('is_test_signing_enabled');
  return result ?? false;
}
