import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getAudioDevices,
  getRunningAudioProcesses,
  isDaemon,
  isEngineRunning,
  isTauri,
  listenToEvent,
  startEngine,
  stopEngine,
  type AudioDevice,
  type AudioProcess,
  type VolumeLevels,
} from './bridge';

// A live backend is available either in the Tauri runtime OR over the web daemon
// (t17). Only when neither is present do we fall back to sample data.
const HAS_BACKEND = isTauri || isDaemon;

/**
 * useBackend — the live data layer (R3).
 *
 * In a Tauri runtime it loads real audio devices and processes, subscribes to
 * device/process/level events, and drives the engine. In a plain browser
 * preview it falls back to sample data so the UI still renders. The rest of the
 * app consumes `devices`, `processes`, `levels` and `setLive` without caring
 * which mode it is in.
 */

const SAMPLE_DEVICES: AudioDevice[] = [
  { id: 'dev-hp', name: 'Headphones (Galaxy Buds)', device_type: 'output', is_default: true },
  { id: 'dev-spk', name: 'Speakers (Realtek)', device_type: 'output', is_default: false },
  { id: 'dev-mic', name: 'Microphone (Blue Yeti)', device_type: 'input', is_default: true },
  // device_type = data flow (capture/render); is_virtual = software device (t9).
  // Our Nodus mic: a capture endpoint we FEED → treated as a sink.
  { id: 'dev-nodusmic', name: 'Nodus Mic', device_type: 'input', is_default: false, is_virtual: true },
  // VB-Cable Input: a render endpoint you WRITE to → a sink.
  { id: 'dev-cable', name: 'CABLE Input', device_type: 'output', is_default: false, is_virtual: true, original_name: 'VB-Audio Virtual Cable' },
  // Third-party virtual mics (another router): capture endpoints you READ from → sources.
  // Two of them share the display name "Микрофон" — distinguished by the real name.
  { id: 'dev-mixline-s', name: 'Микрофон (MIXLINE Stream)', device_type: 'input', is_default: false, is_virtual: true },
  { id: 'dev-mixline-r', name: 'Микрофон (MIXLINE Record)', device_type: 'input', is_default: false, is_virtual: true },
];

// A crisp SVG so the icon path is visible in the browser preview (no Tauri).
// Real icons come from the backend as extracted PNGs.
const SAMPLE_ICON =
  "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2024%2024'%3E%3Crect%20width='24'%20height='24'%20rx='6'%20fill='%231DB954'/%3E%3Ccircle%20cx='12'%20cy='12'%20r='4.5'%20fill='%23fff'/%3E%3C/svg%3E";

const SAMPLE_PROCESSES: AudioProcess[] = [
  { exe_name: 'spotify.exe', pid: 1001, display_name: 'Spotify', source_type: 'music', icon: SAMPLE_ICON },
  { exe_name: 'discord.exe', pid: 1002, display_name: 'Discord', source_type: 'voice' },
  { exe_name: 'chrome.exe', pid: 1003, display_name: 'Chrome', source_type: 'browser' },
];

export interface Backend {
  ready: boolean;
  tauri: boolean;
  devices: AudioDevice[];
  processes: AudioProcess[];
  /** Live per-source levels, keyed by device id or exe name (0..1). */
  levels: VolumeLevels;
  live: boolean;
  /** Turn the engine on/off. `onStarted` runs once the engine has started
   *  (used to push the current routing graph). No-op argument in the browser. */
  setLive: (on: boolean, onStarted?: () => void) => void;
}

export function useBackend(): Backend {
  const [ready, setReady] = useState(false);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [processes, setProcesses] = useState<AudioProcess[]>([]);
  const [levels, setLevels] = useState<VolumeLevels>({});
  const [live, setLiveState] = useState(false);
  const unsubs = useRef<Array<() => void>>([]);

  // Initial load + event subscriptions.
  useEffect(() => {
    let cancelled = false;

    if (!HAS_BACKEND) {
      // Plain browser preview (no daemon): sample data, no live engine.
      setDevices(SAMPLE_DEVICES);
      setProcesses(SAMPLE_PROCESSES);
      setReady(true);
      return;
    }

    (async () => {
      const [devs, procs, running] = await Promise.all([
        getAudioDevices(),
        getRunningAudioProcesses(),
        isEngineRunning(),
      ]);
      if (cancelled) return;
      setDevices(devs);
      setProcesses(procs);
      // Init the Engine button from the shared engine, not a local default — a
      // client joining while the engine already runs must show "live" (t17).
      setLiveState(running);
      setReady(true);

      unsubs.current.push(
        await listenToEvent<AudioDevice[]>('audio-devices-changed', (d) => setDevices(d)),
        await listenToEvent<AudioProcess[]>('process-changed', (p) => setProcesses(p)),
        await listenToEvent<VolumeLevels>('volume-levels', (l) => setLevels(l ?? {})),
        // Engine on/off driven by the engine itself → every client stays in sync.
        // Raw setter (no start/stop call) so this can't loop with the broadcaster.
        await listenToEvent<boolean>('engine-state', (on) => setLiveState(!!on)),
      );
    })();

    return () => {
      cancelled = true;
      unsubs.current.forEach((u) => u());
      unsubs.current = [];
    };
  }, []);

  const setLive = useCallback((on: boolean, onStarted?: () => void) => {
    setLiveState(on);
    if (!HAS_BACKEND) return;
    if (on) {
      // Start the engine, then push the current routing graph (proven order).
      startEngine()
        .then(() => onStarted?.())
        .catch((e) => console.error('start_engine:', e));
    } else {
      void stopEngine().catch((e) => console.error('stop_engine:', e));
      setLevels({});
    }
  }, []);

  return { ready, tauri: isTauri, devices, processes, levels, live, setLive };
}
