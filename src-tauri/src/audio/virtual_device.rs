/// Virtual device management — adaptive setup.
///
/// Nodus supports two backends for virtual audio endpoints:
///   1. VB-Audio VBCABLE  — free, no Test Mode, auto-downloadable
///   2. Nodus kernel driver — own .sys, requires Test Mode (or EV cert for release)
///
/// On startup Nodus calls `get_virtual_setup()` to discover which backend is present
/// and emits the result to the UI for the onboarding dialog.

use serde::{Deserialize, Serialize};

use super::devices::{AudioDevice, DeviceType};

// ── Name detection ────────────────────────────────────────────────────────────

/// Labels that identify known virtual audio backends by substring match on device name.
const VB_AUDIO_MARKERS: &[&str] = &["CABLE Input", "CABLE Output", "VoiceMeeter"];
const NODUS_MARKER: &str = "Nodus";

fn is_vbaudio(name: &str) -> bool {
    VB_AUDIO_MARKERS
        .iter()
        .any(|m| name.to_lowercase().contains(&m.to_lowercase()))
}

fn is_nodus_driver(name: &str) -> bool {
    name.to_lowercase().contains(&NODUS_MARKER.to_lowercase())
}

/// Public: does this device/node name refer to a Nodus virtual endpoint?
/// The routing engine uses this to decide whether a source should be read
/// from the kernel driver's ring buffer (VirtualCapture) instead of WASAPI
/// loopback. Matches the same "Nodus" branding applied by [`apply_nodus_labels`].
pub fn is_nodus_virtual_name(name: &str) -> bool {
    is_nodus_driver(name)
}

/// Public: does this device/node name refer to the Nodus VIRTUAL MICROPHONE
/// endpoint specifically? Matches names containing "nodus" AND ("mic" or
/// "микрофон"), case-insensitive — covers both the raw endpoint name
/// ("Nodus Virtual Mic") and the Windows-localized form
/// ("Микрофон (Nodus Virtual Mic)"). The routing engine uses this to decide
/// whether a route's DESTINATION should be written into the kernel driver's
/// mic ring (VirtualRender) instead of a WASAPI render endpoint.
pub fn is_nodus_virtual_mic_name(name: &str) -> bool {
    let low = name.to_lowercase();
    low.contains("nodus") && (low.contains("mic") || low.contains("микрофон"))
}

/// Map a raw VB-Audio device name to a Nodus-branded label shown in the UI.
/// Returns None if the device doesn't need renaming.
pub fn nodus_label(device_name: &str) -> Option<&'static str> {
    let low = device_name.to_lowercase();
    if low.contains("cable input") {
        // CABLE Input = render endpoint (apps output HERE)
        Some("Nodus Virtual Speaker")
    } else if low.contains("cable output") {
        // CABLE Output = capture endpoint (Nodus reads FROM here)
        Some("Nodus Virtual Output")
    } else {
        None
    }
}

/// Apply Nodus-branded labels to a mutable device list in-place.
/// CABLE Input → "Nodus Virtual Speaker", CABLE Output → "Nodus Virtual Output", etc.
/// Saves the original system name in `original_name` so the UI can still show it.
pub fn apply_nodus_labels(devices: &mut Vec<AudioDevice>) {
    for d in devices.iter_mut() {
        if let Some(label) = nodus_label(&d.name) {
            d.original_name = Some(d.name.clone());
            d.name = label.to_string();
        }
    }
}

// ── Setup status ──────────────────────────────────────────────────────────────

/// Which backend is currently active (or available to install).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VirtualSetupKind {
    /// Nothing found — show onboarding dialog
    NotFound,
    /// VB-Audio VBCABLE detected
    VbAudio,
    /// Our own kernel driver detected
    NodusDriver,
}

/// A single resolved virtual endpoint pair exposed by Nodus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualEndpoint {
    /// Nodus label shown in UI ("Nodus Virtual Speaker")
    pub label: String,
    /// Actual WASAPI device ID
    pub device_id: String,
    /// Input (capture) or Output (render)
    pub device_type: DeviceType,
    /// Raw system name ("CABLE Input (VB-Audio Virtual Cable)")
    pub underlying_name: String,
}

/// Full virtual device setup state returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualSetupStatus {
    pub kind: VirtualSetupKind,
    pub endpoints: Vec<VirtualEndpoint>,
    /// Human-readable message for the UI
    pub message: String,
}

/// Inspect the full device list and determine setup status + resolve endpoints.
pub fn get_virtual_setup(all_devices: &[AudioDevice]) -> VirtualSetupStatus {
    let mut endpoints: Vec<VirtualEndpoint> = Vec::new();

    for d in all_devices {
        if is_nodus_driver(&d.name) {
            endpoints.push(VirtualEndpoint {
                label: d.name.clone(),
                device_id: d.id.clone(),
                device_type: d.device_type.clone(),
                underlying_name: d.name.clone(),
            });
        } else if is_vbaudio(&d.name) {
            if let Some(label) = nodus_label(&d.name) {
                endpoints.push(VirtualEndpoint {
                    label: label.to_string(),
                    device_id: d.id.clone(),
                    device_type: d.device_type.clone(),
                    underlying_name: d.name.clone(),
                });
            }
        }
    }

    // Determine kind by priority: Nodus driver > VB-Audio > nothing
    let kind = if endpoints.iter().any(|e| is_nodus_driver(&e.underlying_name)) {
        VirtualSetupKind::NodusDriver
    } else if !endpoints.is_empty() {
        VirtualSetupKind::VbAudio
    } else {
        VirtualSetupKind::NotFound
    };

    let message = match kind {
        VirtualSetupKind::NotFound => {
            "Virtual device not found. Install VB-Audio or enable Nodus driver.".to_string()
        }
        VirtualSetupKind::VbAudio => {
            format!("VB-Audio detected — {} endpoints ready.", endpoints.len())
        }
        VirtualSetupKind::NodusDriver => {
            format!("Nodus driver detected — {} endpoints ready.", endpoints.len())
        }
    };

    VirtualSetupStatus { kind, endpoints, message }
}

// Keep old helper for backward compat
pub fn query_virtual_status(all_devices: &[AudioDevice]) -> crate::audio::virtual_device::LegacyStatus {
    let setup = get_virtual_setup(all_devices);
    LegacyStatus {
        available: setup.kind != VirtualSetupKind::NotFound,
        devices: all_devices
            .iter()
            .filter(|d| is_vbaudio(&d.name) || is_nodus_driver(&d.name))
            .map(|d| {
                let mut dev = d.clone();
                dev.device_type = DeviceType::Virtual;
                if let Some(label) = nodus_label(&d.name) {
                    dev.name = label.to_string();
                }
                dev
            })
            .collect(),
        message: setup.message,
    }
}

/// Legacy shape kept for bridge.rs compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyStatus {
    pub available: bool,
    pub devices: Vec<AudioDevice>,
    pub message: String,
}

// ── Install / system helpers (Windows) ───────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod setup {
    use std::path::PathBuf;
    use tracing::info;

    const VBCABLE_URL: &str =
        "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack43.zip";

    /// Download and silently install VB-Audio VBCABLE.
    /// Returns Ok(()) when the installer has been launched (UAC prompt shown to user).
    /// Detecting successful install is done by re-querying audio devices afterward.
    pub async fn install_vbcable() -> Result<(), String> {
        use std::process::Command;

        let tmp = std::env::temp_dir().join("nodus_vbcable");
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

        let zip_path = tmp.join("VBCABLE.zip");
        let exe_path = tmp.join("VBCABLE_Setup_x64.exe");

        // Download
        info!("downloading VB-Audio VBCABLE from {VBCABLE_URL}");
        download_file(VBCABLE_URL, &zip_path).await?;

        // Extract installer
        extract_exe_from_zip(&zip_path, &exe_path)?;

        // Launch with UAC elevation (installer will prompt user)
        info!("launching VBCABLE installer (UAC prompt expected)");
        Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Start-Process -FilePath '{}' -Verb RunAs -Wait",
                    exe_path.display()
                ),
            ])
            .spawn()
            .map_err(|e| format!("failed to launch installer: {e}"))?
            .wait()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn download_file(url: &str, dest: &PathBuf) -> Result<(), String> {
        // Use PowerShell's Invoke-WebRequest for simplicity (no extra deps)
        let status = tokio::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
                    url,
                    dest.display()
                ),
            ])
            .status()
            .await
            .map_err(|e| e.to_string())?;

        if !status.success() {
            return Err("download failed — check your internet connection".to_string());
        }
        Ok(())
    }

    fn extract_exe_from_zip(zip: &PathBuf, exe_dest: &PathBuf) -> Result<(), String> {
        let status = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force; \
                     $exe = Get-ChildItem '{}' -Filter '*x64*.exe' -Recurse | Select-Object -First 1; \
                     Copy-Item $exe.FullName -Destination '{}'",
                    zip.display(),
                    zip.parent().unwrap().display(),
                    zip.parent().unwrap().display(),
                    exe_dest.display()
                ),
            ])
            .status()
            .map_err(|e| e.to_string())?;

        if !status.success() || !exe_dest.exists() {
            return Err("could not extract installer from zip".to_string());
        }
        Ok(())
    }

    /// Check if Windows test signing (unsigned driver loading) is enabled.
    pub fn is_test_signing_enabled() -> bool {
        // bcdedit stores this in BCD registry; easiest is to run bcdedit and parse.
        let out = std::process::Command::new("bcdedit")
            .args(["/enum", "{current}"])
            .output();

        match out {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.to_lowercase().contains("testsigning") && text.to_lowercase().contains("yes")
            }
            Err(_) => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod setup {
    pub async fn install_vbcable() -> Result<(), String> {
        Err("VB-Audio install is Windows-only".to_string())
    }
    pub fn is_test_signing_enabled() -> bool {
        false
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, t: DeviceType) -> AudioDevice {
        AudioDevice { id: format!("id-{name}"), name: name.to_string(), device_type: t, is_default: false, original_name: None }
    }

    #[test]
    fn cable_input_maps_to_nodus_virtual_speaker() {
        assert_eq!(nodus_label("CABLE Input (VB-Audio Virtual Cable)"), Some("Nodus Virtual Speaker"));
    }

    #[test]
    fn cable_output_maps_to_nodus_virtual_output() {
        assert_eq!(nodus_label("CABLE Output (VB-Audio Virtual Cable)"), Some("Nodus Virtual Output"));
    }

    #[test]
    fn real_device_not_renamed() {
        assert_eq!(nodus_label("Realtek HD Audio"), None);
    }

    #[test]
    fn virtual_mic_name_detected() {
        assert!(is_nodus_virtual_mic_name("Nodus Virtual Mic"));
        assert!(is_nodus_virtual_mic_name("Микрофон (Nodus Virtual Mic)"));
    }

    #[test]
    fn virtual_mic_name_rejects_non_mic_endpoints() {
        // Nodus, but not a microphone.
        assert!(!is_nodus_virtual_mic_name("Nodus Virtual Speaker"));
        assert!(!is_nodus_virtual_mic_name("Динамики (Nodus Virtual Audio)"));
        // A microphone, but not Nodus.
        assert!(!is_nodus_virtual_mic_name("Микрофон (Realtek High Definition Audio)"));
    }

    #[test]
    fn setup_detects_vbaudio() {
        let devices = vec![
            dev("CABLE Input (VB-Audio Virtual Cable)", DeviceType::Output),
            dev("CABLE Output (VB-Audio Virtual Cable)", DeviceType::Input),
            dev("Headphones", DeviceType::Output),
        ];
        let status = get_virtual_setup(&devices);
        assert_eq!(status.kind, VirtualSetupKind::VbAudio);
        assert_eq!(status.endpoints.len(), 2);
        assert_eq!(status.endpoints[0].label, "Nodus Virtual Speaker");
    }

    #[test]
    fn setup_not_found_when_no_virtual() {
        let devices = vec![dev("Headphones", DeviceType::Output)];
        let status = get_virtual_setup(&devices);
        assert_eq!(status.kind, VirtualSetupKind::NotFound);
        assert!(status.endpoints.is_empty());
    }

    #[test]
    fn apply_labels_renames_in_place() {
        let mut devices = vec![
            dev("CABLE Input (VB-Audio Virtual Cable)", DeviceType::Output),
            dev("Galaxy Buds FE", DeviceType::Output),
        ];
        apply_nodus_labels(&mut devices);
        assert_eq!(devices[0].name, "Nodus Virtual Speaker");
        assert_eq!(devices[1].name, "Galaxy Buds FE"); // unchanged
    }
}
