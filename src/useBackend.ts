import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getAudioDevices,
  getRunningAudioProcesses,
  isTauri,
  listenToEvent,
  startEngine,
  stopEngine,
  type AudioDevice,
  type AudioProcess,
  type VolumeLevels,
} from './bridge';

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
  { id: 'dev-nodusmic', name: 'Nodus Mic', device_type: 'virtual', is_default: false },
];

const SAMPLE_PROCESSES: AudioProcess[] = [
  { exe_name: 'spotify.exe', pid: 1001, display_name: 'Spotify', source_type: 'music' },
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
  setLive: (on: boolean) => void;
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

    if (!isTauri) {
      // Browser preview: sample data, no live engine.
      setDevices(SAMPLE_DEVICES);
      setProcesses(SAMPLE_PROCESSES);
      setReady(true);
      return;
    }

    (async () => {
      const [devs, procs] = await Promise.all([getAudioDevices(), getRunningAudioProcesses()]);
      if (cancelled) return;
      setDevices(devs);
      setProcesses(procs);
      setReady(true);

      unsubs.current.push(
        await listenToEvent<AudioDevice[]>('audio-devices-changed', (d) => setDevices(d)),
        await listenToEvent<AudioProcess[]>('process-changed', (p) => setProcesses(p)),
        await listenToEvent<VolumeLevels>('volume-levels', (l) => setLevels(l ?? {})),
      );
    })();

    return () => {
      cancelled = true;
      unsubs.current.forEach((u) => u());
      unsubs.current = [];
    };
  }, []);

  const setLive = useCallback((on: boolean) => {
    setLiveState(on);
    if (!isTauri) return;
    void (on ? startEngine() : stopEngine());
    if (!on) setLevels({});
  }, []);

  return { ready, tauri: isTauri, devices, processes, levels, live, setLive };
}
