//! t14 — application settings: a small typed document mirrored + persisted exactly
//! like the scene workspace (t17), so settings sync across Desktop ↔ Web and survive
//! restart. The store is the single source of truth; clients hydrate on mount and
//! receive `settings:changed` on every update.
//!
//! Honesty rule: every field here maps to a real effect. Audio-format knobs are
//! intentionally absent — the engine is fixed at 48 kHz/2ch/32f in shared mode, so
//! tuning that needs t10 (resampler / exclusive mode) and would be a dead control today.

use std::path::PathBuf;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::server::{EventBus, ServerEvent};

/// All persisted application settings. `#[serde(default)]` so older files / partial
/// payloads fill missing fields from defaults rather than failing to load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ── Performance (applied live) ──────────────────────────────────────────
    /// Emit VU meter levels at all. Off = no meters, lowest CPU on weak GPUs.
    pub vu_enabled: bool,
    /// VU meter refresh rate in frames per second (clamped 1..=60).
    pub vu_fps: u32,
    /// How often to scan for audio processes, seconds (clamped 1..=30). Applied at
    /// launch; the detector picks it up when (re)started.
    pub process_scan_secs: u32,

    // ── Server / browser access (applied at next launch) ───────────────────
    /// TCP port the embedded daemon listens on.
    pub server_port: u16,
    /// Bind on the LAN (0.0.0.0) instead of loopback. Forces a token on.
    pub server_lan: bool,

    // ── App behavior ───────────────────────────────────────────────────────
    /// Start the routing engine automatically when the app launches (frontend).
    pub start_engine_on_launch: bool,
    /// Close (×) hides to the tray instead of quitting (read live on close).
    pub close_to_tray: bool,
    /// Launch Nodus when Windows starts (HKCU Run; applied on change).
    pub start_with_windows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            vu_enabled: true,
            vu_fps: 15,
            process_scan_secs: 2,
            server_port: 7878,
            server_lan: false,
            start_engine_on_launch: false,
            close_to_tray: true,
            start_with_windows: false,
        }
    }
}

impl Settings {
    /// VU refresh interval, derived from `vu_fps` (clamped to a sane range).
    pub fn vu_interval(&self) -> Duration {
        let fps = self.vu_fps.clamp(1, 60);
        Duration::from_millis(1000 / fps as u64)
    }
    /// Process-scan interval (clamped).
    pub fn scan_interval(&self) -> Duration {
        Duration::from_secs(self.process_scan_secs.clamp(1, 30) as u64)
    }
}

pub struct SettingsStore {
    inner: RwLock<Settings>,
    path: Option<PathBuf>,
    bus: EventBus,
}

impl SettingsStore {
    /// Load persisted settings if present, else defaults.
    pub fn new(path: Option<PathBuf>, bus: EventBus) -> Self {
        let settings = path
            .as_ref()
            .and_then(|p| load(p))
            .unwrap_or_default();
        Self {
            inner: RwLock::new(settings),
            path,
            bus,
        }
    }

    pub fn get(&self) -> Settings {
        self.inner.read().clone()
    }

    /// Replace settings, persist, broadcast `settings:changed`. Returns the new
    /// settings (normalised). Side effects that must happen regardless of which
    /// client changed them (e.g. Windows autostart) are applied here.
    pub fn set(&self, next: Settings) -> Settings {
        let prev = self.get();
        {
            let mut g = self.inner.write();
            *g = next.clone();
        }
        if prev.start_with_windows != next.start_with_windows {
            apply_autostart(next.start_with_windows);
        }
        if let Some(p) = &self.path {
            if let Err(e) = persist(p, &next) {
                error!("settings persist failed: {e}");
            }
        }
        if let Ok(payload) = serde_json::to_value(&next) {
            let _ = self.bus.send(ServerEvent {
                event: "settings:changed".into(),
                payload,
            });
        }
        next
    }
}

fn load(path: &PathBuf) -> Option<Settings> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Atomic write: serialize → temp file → rename over the target.
fn persist(path: &PathBuf, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Add/remove the HKCU Run entry so Nodus launches with Windows. Uses reg.exe so no
/// extra crate + no elevation (HKCU needs none). No-op off Windows.
#[cfg(target_os = "windows")]
fn apply_autostart(enable: bool) {
    use std::process::Command;
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Nodus";
    let result = if enable {
        match std::env::current_exe() {
            Ok(exe) => Command::new("reg")
                .args(["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &exe.to_string_lossy(), "/f"])
                .output(),
            Err(e) => {
                error!("autostart: cannot resolve current exe: {e}");
                return;
            }
        }
    } else {
        Command::new("reg")
            .args(["delete", KEY, "/v", NAME, "/f"])
            .output()
    };
    if let Err(e) = result {
        error!("autostart reg update failed: {e}");
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_autostart(_enable: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(path: Option<PathBuf>) -> SettingsStore {
        let (bus, _rx) = tokio::sync::broadcast::channel(8);
        SettingsStore::new(path, bus)
    }

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert!(s.vu_enabled);
        assert_eq!(s.vu_interval(), Duration::from_millis(1000 / 15));
        assert_eq!(s.scan_interval(), Duration::from_secs(2));
        assert!(!s.server_lan);
        assert!(s.close_to_tray);
    }

    #[test]
    fn vu_interval_clamps() {
        let mut s = Settings::default();
        s.vu_fps = 0;
        assert_eq!(s.vu_interval(), Duration::from_millis(1000)); // clamped to 1 fps
        s.vu_fps = 1000;
        assert_eq!(s.vu_interval(), Duration::from_millis(1000 / 60)); // clamped to 60
    }

    #[test]
    fn set_broadcasts_and_keeps() {
        let (bus, mut rx) = tokio::sync::broadcast::channel(8);
        let s = SettingsStore::new(None, bus);
        let mut next = Settings::default();
        next.vu_fps = 30;
        next.server_lan = true;
        s.set(next.clone());
        assert_eq!(s.get().vu_fps, 30);
        let ev = rx.try_recv().expect("settings:changed broadcast");
        assert_eq!(ev.event, "settings:changed");
        assert_eq!(ev.payload["vu_fps"], 30);
        assert_eq!(ev.payload["server_lan"], true);
    }

    #[test]
    fn persists_and_reloads() {
        let dir = std::env::temp_dir().join(format!("nodus-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let s = store(Some(path.clone()));
            let mut next = Settings::default();
            next.process_scan_secs = 5;
            s.set(next);
        }
        let s2 = store(Some(path.clone()));
        assert_eq!(s2.get().process_scan_secs, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_json_fills_defaults() {
        // A file written by an older build (missing fields) must still load.
        let s: Settings = serde_json::from_str(r#"{ "vu_fps": 20 }"#).unwrap();
        assert_eq!(s.vu_fps, 20);
        assert!(s.vu_enabled); // default
        assert_eq!(s.server_port, 7878); // default
    }
}
