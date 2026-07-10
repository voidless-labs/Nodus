use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;
use tracing::{trace, warn};

/// Lock a mutex, recovering the guard even if a previous holder panicked.
/// Project rule: no `.unwrap()` in production code — a poisoned mutex must not
/// cascade. The guarded state (a bool flag / a pid set) is safe to recover.
fn lock_recover<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("failed to create process snapshot: {0}")]
    SnapshotFailed(String),
    #[error("process iteration failed: {0}")]
    IterationFailed(String),
}

/// Nodus source type mapped from the detected exe.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Game,
    Chat,
    Voice,
    Music,
    Browser,
    Recording,
    System,
    Unknown,
}

/// A detected audio process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioProcess {
    pub exe_name: String,
    pub pid: u32,
    pub display_name: String,
    pub source_type: SourceType,
    /// App icon as a PNG data URL, extracted from the .exe (R7). None if unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Map from known exe names to (display_name, source_type).
fn classify_exe(exe: &str) -> Option<(&'static str, SourceType)> {
    match exe.to_lowercase().as_str() {
        "arma3_x64.exe" | "arma3.exe" => Some(("Arma 3", SourceType::Game)),
        "discord.exe" => Some(("Discord", SourceType::Chat)),
        "ts3client_win64.exe" | "ts3client_win32.exe" => Some(("TeamSpeak 3", SourceType::Voice)),
        "spotify.exe" => Some(("Spotify", SourceType::Music)),
        "chrome.exe" => Some(("Chrome", SourceType::Browser)),
        "firefox.exe" => Some(("Firefox", SourceType::Browser)),
        "msedge.exe" => Some(("Edge", SourceType::Browser)),
        "obs64.exe" | "obs32.exe" | "obs.exe" => Some(("OBS Studio", SourceType::Recording)),
        "steamwebhelper.exe" => Some(("Steam", SourceType::System)),
        "csgo.exe" | "cs2.exe" => Some(("Counter-Strike", SourceType::Game)),
        "hl2.exe" => Some(("Source Engine Game", SourceType::Game)),
        _ => None,
    }
}

/// OS audio plumbing / shell processes that always hold a render session but are
/// not a user "source" — hidden from session-based detection (t23). Our own
/// process is excluded separately by PID.
fn is_system_session_exe(exe_lower: &str) -> bool {
    matches!(
        exe_lower,
        "svchost.exe"
            | "audiodg.exe"
            | "dwm.exe"
            | "csrss.exe"
            | "wininit.exe"
            | "taskhostw.exe"
            | "sihost.exe"
            | "runtimebroker.exe"
            | "ctfmon.exe"
            | "explorer.exe"
            | "searchhost.exe"
            | "shellexperiencehost.exe"
            | "startmenuexperiencehost.exe"
            | "applicationframehost.exe"
    )
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// PIDs that currently hold a render audio session — i.e. apps actually using
    /// audio, whether or not they are in the known-exe list. This is what catches
    /// a game (or any app) that produces sound but isn't hard-coded (t23).
    /// Best-effort: returns empty on any COM failure. pid 0 (system sounds) skipped.
    fn audio_session_pids() -> std::collections::HashSet<u32> {
        use crate::audio::wasapi::ComGuard;
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{
            eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
            MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

        let mut pids = std::collections::HashSet::new();
        let _com = match ComGuard::init() {
            Ok(c) => c,
            Err(_) => return pids,
        };
        unsafe {
            let en: IMMDeviceEnumerator =
                match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    Ok(e) => e,
                    Err(_) => return pids,
                };
            let coll = match en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                Ok(c) => c,
                Err(_) => return pids,
            };
            for i in 0..coll.GetCount().unwrap_or(0) {
                let dev = match coll.Item(i) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let mgr: IAudioSessionManager2 = match dev.Activate(CLSCTX_ALL, None) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let se = match mgr.GetSessionEnumerator() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                for j in 0..se.GetCount().unwrap_or(0) {
                    let ctrl = match se.GetSession(j) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let pid = ctrl2.GetProcessId().unwrap_or(0);
                    if pid != 0 {
                        pids.insert(pid);
                    }
                }
            }
        }
        pids
    }

    /// Strip a trailing ".exe" (case-insensitive) for a display name fallback.
    fn pretty_exe_name(exe: &str) -> String {
        if exe.to_lowercase().ends_with(".exe") {
            exe[..exe.len() - 4].to_string()
        } else {
            exe.to_string()
        }
    }

    /// Snapshot running processes; return those that are either a known audio app
    /// OR currently hold a render audio session (t23). Deduplicated by exe name.
    pub fn detect_audio_processes() -> Result<Vec<AudioProcess>, DetectionError> {
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                DetectionError::SnapshotFailed(format!("{e}"))
            })?
        };

        // Use LinkedHashMap ordering: first-seen PID wins for each exe name.
        let mut seen: std::collections::HashMap<String, AudioProcess> =
            std::collections::HashMap::new();

        // Apps actually using audio right now (catches games / unlisted apps, t23).
        let audio_pids = audio_session_pids();
        let own_pid = std::process::id(); // don't list Nodus itself

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // Process32FirstW returns Result<()> in windows-rs; error means no processes
        if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
            unsafe { let _ = CloseHandle(snapshot); }
            return Ok(Vec::new());
        }

        loop {
            let exe_name = String::from_utf16_lossy(
                entry.szExeFile.split(|&c| c == 0).next().unwrap_or(&[]),
            );
            let key = exe_name.to_lowercase();

            if !seen.contains_key(&key) {
                let known = classify_exe(&exe_name);
                let pid = entry.th32ProcessID;
                // Session-based: a real app using audio, but not us and not OS plumbing.
                let session_ok =
                    audio_pids.contains(&pid) && pid != own_pid && !is_system_session_exe(&key);
                // Show if it's a known audio app OR it currently uses audio (t23).
                if known.is_some() || session_ok {
                    let (display_name, source_type) = match known {
                        Some((d, t)) => (d.to_string(), t),
                        None => (pretty_exe_name(&exe_name), SourceType::Unknown),
                    };
                    // trace, not debug: this fires for every process every scan
                    // (~2s) and drowns the log; enable with RUST_LOG=…nodus=trace.
                    trace!("detected audio process: {exe_name} (pid {pid}, session={})",
                        audio_pids.contains(&pid));
                    seen.insert(
                        key,
                        AudioProcess {
                            exe_name: exe_name.clone(),
                            pid,
                            display_name,
                            source_type,
                            icon: crate::detection::icon::icon_data_url(pid, &exe_name),
                        },
                    );
                }
            }

            // Process32NextW returns Err when no more entries remain
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }

        unsafe { let _ = CloseHandle(snapshot); }

        let mut result: Vec<AudioProcess> = seen.into_values().collect();
        // Stable sort by display name for consistent output
        result.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok(result)
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn detect_audio_processes() -> Result<Vec<AudioProcess>, DetectionError> {
        Ok(Vec::new())
    }
}

pub use platform::detect_audio_processes;

/// Background detector that polls for process changes and notifies via a callback.
pub struct ProcessDetector {
    known: Arc<Mutex<HashSet<u32>>>,
    running: Arc<Mutex<bool>>,
}

impl ProcessDetector {
    pub fn new() -> Self {
        Self {
            known: Arc::new(Mutex::new(HashSet::new())),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Start polling in a background thread.
    /// `on_change` is called with the full process list whenever it changes.
    pub fn start<F>(&self, interval: Duration, on_change: F)
    where
        F: Fn(Vec<AudioProcess>) + Send + 'static,
    {
        let mut running_lock = lock_recover(&self.running);
        if *running_lock {
            return;
        }
        *running_lock = true;
        drop(running_lock);

        let known = Arc::clone(&self.known);
        let running = Arc::clone(&self.running);

        std::thread::spawn(move || {
            while *lock_recover(&running) {
                match detect_audio_processes() {
                    Ok(procs) => {
                        let current_pids: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
                        let mut known_lock = lock_recover(&known);
                        if *known_lock != current_pids {
                            *known_lock = current_pids;
                            drop(known_lock);
                            on_change(procs);
                        }
                    }
                    Err(e) => warn!("process detection error: {e}"),
                }
                std::thread::sleep(interval);
            }
        });
    }

    pub fn stop(&self) {
        *lock_recover(&self.running) = false;
    }
}

impl Default for ProcessDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_exe_classified_correctly() {
        let (name, kind) = classify_exe("arma3_x64.exe").unwrap();
        assert_eq!(name, "Arma 3");
        assert_eq!(kind, SourceType::Game);

        let (name, kind) = classify_exe("discord.exe").unwrap();
        assert_eq!(name, "Discord");
        assert_eq!(kind, SourceType::Chat);

        let (name, kind) = classify_exe("spotify.exe").unwrap();
        assert_eq!(kind, SourceType::Music);
        let _ = name;
    }

    #[test]
    fn unknown_exe_returns_none() {
        assert!(classify_exe("notepad.exe").is_none());
        assert!(classify_exe("explorer.exe").is_none());
    }

    #[test]
    fn exe_classification_case_insensitive() {
        assert!(classify_exe("Discord.exe").is_some());
        assert!(classify_exe("ARMA3_X64.EXE").is_some());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn detect_returns_vec_on_windows() {
        // Just verify it doesn't panic — real content depends on what's running
        let result = detect_audio_processes();
        assert!(result.is_ok());
    }
}
