use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;
use tracing::{debug, warn};

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

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// Snapshot all running processes, return those matching known audio apps.
    /// Deduplicated by exe name — multi-process apps (browsers, Discord) show once.
    pub fn detect_audio_processes() -> Result<Vec<AudioProcess>, DetectionError> {
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                DetectionError::SnapshotFailed(format!("{e}"))
            })?
        };

        // Use LinkedHashMap ordering: first-seen PID wins for each exe name.
        let mut seen: std::collections::HashMap<String, AudioProcess> =
            std::collections::HashMap::new();

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
                if let Some((display_name, source_type)) = classify_exe(&exe_name) {
                    debug!("detected audio process: {exe_name} (pid {})", entry.th32ProcessID);
                    let pid = entry.th32ProcessID;
                    seen.insert(
                        key,
                        AudioProcess {
                            exe_name: exe_name.clone(),
                            pid,
                            display_name: display_name.to_string(),
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
