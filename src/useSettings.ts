import { useCallback, useEffect, useState } from 'react';
import {
  DEFAULT_SETTINGS,
  getSettings,
  isDaemon,
  isTauri,
  listenAny,
  setSettings as bridgeSetSettings,
  type Settings,
} from './bridge';

/**
 * useSettings — the application-settings store (t14).
 *
 * The daemon owns settings (mirrored + persisted like the scene), so this hook
 * hydrates from it on mount, follows `settings:changed` broadcasts (so a change in
 * one window/UI shows in the other), and writes through `set_settings`. Without a
 * backend it falls back to in-memory defaults so the modal still works in preview.
 */

const HAS_BACKEND = isTauri || isDaemon;

export interface SettingsStore {
  settings: Settings;
  /** True once the daemon value has been loaded (or there is no backend). */
  ready: boolean;
  /** Patch one or more fields; optimistic locally, persisted via the daemon. */
  update: (patch: Partial<Settings>) => void;
}

export function useSettings(): SettingsStore {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [ready, setReady] = useState(!HAS_BACKEND);

  useEffect(() => {
    if (!HAS_BACKEND) return;
    let cancelled = false;
    let unsub = () => {};
    (async () => {
      const s = await getSettings();
      if (cancelled) return;
      if (s) setSettings(s);
      setReady(true);
      unsub = await listenAny<Settings>('settings:changed', (s2) => {
        if (s2) setSettings(s2);
      });
    })();
    return () => {
      cancelled = true;
      unsub();
    };
  }, []);

  const update = useCallback((patch: Partial<Settings>) => {
    setSettings((cur) => {
      const next = { ...cur, ...patch };
      // Fire-and-forget; the daemon echoes `settings:changed` which keeps every
      // client (including this one) consistent.
      void bridgeSetSettings(next).catch((e) => console.error('set_settings:', e));
      return next;
    });
  }, []);

  return { settings, ready, update };
}
