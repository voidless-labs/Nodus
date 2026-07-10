use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use super::wasapi::WasapiError;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("WASAPI error: {0}")]
    Wasapi(#[from] WasapiError),
    #[error("enumeration failed: {0}")]
    Enumeration(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Input,
    Output,
    Virtual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub device_type: DeviceType,
    pub is_default: bool,
    /// Real system name before Nodus branding (e.g. "CABLE Output (VB-Audio Virtual Cable)").
    /// Populated only when `name` has been rebranded via apply_nodus_labels.
    #[serde(default)]
    pub original_name: Option<String>,
    /// True for software/virtual audio devices (VB-Cable, VoiceMeeter, MIXLINE,
    /// Nodus, …) as opposed to real hardware. `device_type` still carries the
    /// data flow (Input = capture, Output = render) — that drives port direction;
    /// `is_virtual` only affects classification/badging in the UI. (t9)
    #[serde(default)]
    pub is_virtual: bool,
}

/// Heuristic: does this audio endpoint belong to a software/virtual driver?
/// Name-list of common virtual-audio vendors (cheap and reliable for the ones
/// users actually have). Enumerator/form-factor signals are a future refinement.
pub fn is_virtual_device(name: &str) -> bool {
    let n = name.to_lowercase();
    const MARKERS: &[&str] = &[
        "virtual",
        "vb-audio",
        "cable",
        "voicemeeter",
        "vac",
        "mixline",
        "nodus",
        "steam streaming",
        "nvidia broadcast",
        "rtx voice",
        "obs",
    ];
    MARKERS.iter().any(|m| n.contains(m))
}

// ── Windows implementation ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::{
        Win32::{
            Devices::Properties::DEVPKEY_Device_FriendlyName,
            Media::Audio::{
                eCapture, eConsole, eRender, IMMDevice, IMMDeviceEnumerator,
                MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
            },
            System::Com::{
                CoCreateInstance, StructuredStorage::PropVariantClear, CLSCTX_ALL, STGM,
            },
            UI::Shell::PropertiesSystem::IPropertyStore,
        },
    };

    // STGM_READ = 0x00000000 (Windows SDK constant, wrapped in STGM newtype)
    const STGM_READ: STGM = STGM(0x00000000);

    fn device_friendly_name(device: &IMMDevice) -> Result<String, DeviceError> {
        unsafe {
            let store: IPropertyStore = device
                .OpenPropertyStore(STGM_READ)
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

            let mut prop = store
                .GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _)
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

            // PROPVARIANT x64 binary layout: vt(u16) + reserved(6 bytes) + value(8+ bytes)
            // VT_LPWSTR = 31 means the 8-byte value is a PWSTR pointer.
            let raw = &prop as *const _ as *const u8;
            let vt = u16::from_le_bytes([*raw, *raw.add(1)]);
            let name = if vt == 31 {
                let pwstr = *(raw.add(8) as *const *const u16);
                if pwstr.is_null() {
                    "Unknown".into()
                } else {
                    let mut len = 0usize;
                    while *pwstr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(pwstr, len);
                    String::from_utf16_lossy(slice).into()
                }
            } else {
                "Unknown".into()
            };

            PropVariantClear(&mut prop).ok();
            Ok(name)
        }
    }

    fn device_id(device: &IMMDevice) -> Result<String, DeviceError> {
        unsafe {
            let raw = device
                .GetId()
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;
            let id = raw.to_string().unwrap_or_default();
            Ok(id)
        }
    }

    fn default_device_id(
        enumerator: &IMMDeviceEnumerator,
        flow: windows::Win32::Media::Audio::EDataFlow,
    ) -> Option<String> {
        unsafe {
            let dev = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
            device_id(&dev).ok()
        }
    }

    fn enumerate_flow(
        enumerator: &IMMDeviceEnumerator,
        flow: windows::Win32::Media::Audio::EDataFlow,
        dev_type: DeviceType,
        default_id: &Option<String>,
    ) -> Result<Vec<AudioDevice>, DeviceError> {
        let mut devices = Vec::new();
        unsafe {
            let collection = enumerator
                .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

            let count = collection
                .GetCount()
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

            for i in 0..count {
                let device = collection
                    .Item(i)
                    .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

                let id = device_id(&device)?;
                let name = device_friendly_name(&device).unwrap_or_else(|_| "Unknown".into());
                let is_default = default_id.as_deref() == Some(&id);

                debug!("found device [{dev_type:?}] {name} ({id})");
                let is_virtual = is_virtual_device(&name);
                devices.push(AudioDevice {
                    id,
                    name,
                    device_type: dev_type.clone(),
                    is_default,
                    original_name: None,
                    is_virtual,
                });
            }
        }
        Ok(devices)
    }

    pub fn enumerate_audio_devices() -> Result<Vec<AudioDevice>, DeviceError> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| DeviceError::Enumeration(e.to_string()))?;

            let default_output = default_device_id(&enumerator, eRender);
            let default_input = default_device_id(&enumerator, eCapture);

            let mut all = Vec::new();
            all.extend(enumerate_flow(
                &enumerator,
                eRender,
                DeviceType::Output,
                &default_output,
            )?);
            all.extend(enumerate_flow(
                &enumerator,
                eCapture,
                DeviceType::Input,
                &default_input,
            )?);

            Ok(all)
        }
    }

    // ── Live device-change notifications (t19) ─────────────────────────────
    // Windows tells us when endpoints are added / removed / change state / the
    // default changes, so the UI device list (and the per-node presence status +
    // the Add panel) stays live instead of frozen at the startup snapshot.

    #[windows::core::implement(windows::Win32::Media::Audio::IMMNotificationClient)]
    struct NotificationClient {
        // Mutex so the COM object is Sync — endpoint callbacks may arrive on any
        // MTA thread. Sending is cheap; the heavy re-enumeration is off-thread.
        tx: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    }

    impl NotificationClient {
        fn ping(&self) {
            if let Ok(tx) = self.tx.lock() {
                let _ = tx.send(());
            }
        }
    }

    #[allow(non_snake_case)]
    impl windows::Win32::Media::Audio::IMMNotificationClient_Impl for NotificationClient_Impl {
        fn OnDeviceStateChanged(
            &self,
            _device_id: &windows::core::PCWSTR,
            _new_state: windows::Win32::Media::Audio::DEVICE_STATE,
        ) -> windows::core::Result<()> {
            self.ping();
            Ok(())
        }
        fn OnDeviceAdded(&self, _device_id: &windows::core::PCWSTR) -> windows::core::Result<()> {
            self.ping();
            Ok(())
        }
        fn OnDeviceRemoved(&self, _device_id: &windows::core::PCWSTR) -> windows::core::Result<()> {
            self.ping();
            Ok(())
        }
        fn OnDefaultDeviceChanged(
            &self,
            _flow: windows::Win32::Media::Audio::EDataFlow,
            _role: windows::Win32::Media::Audio::ERole,
            _default_device_id: &windows::core::PCWSTR,
        ) -> windows::core::Result<()> {
            self.ping();
            Ok(())
        }
        fn OnPropertyValueChanged(
            &self,
            _device_id: &windows::core::PCWSTR,
            _key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
        ) -> windows::core::Result<()> {
            // Property churn (volume, format, …) is noisy and never adds/removes a
            // device — ignore it so we don't re-enumerate on every volume tick.
            Ok(())
        }
    }

    /// Register for endpoint notifications and call `on_change` (debounced) whenever
    /// the set of devices changes. Blocks forever — keeps the COM registration alive,
    /// so run it on a dedicated thread. (t19)
    pub fn watch_device_changes(on_change: Box<dyn Fn() + Send>) -> Result<(), DeviceError> {
        use crate::audio::wasapi::ComGuard;
        use windows::Win32::Media::Audio::IMMNotificationClient;

        let _com = ComGuard::init().map_err(|e| DeviceError::Enumeration(e.to_string()))?;
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let client: IMMNotificationClient =
            NotificationClient { tx: std::sync::Mutex::new(tx) }.into();

        let enumerator: IMMDeviceEnumerator = unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?
        };
        unsafe {
            enumerator
                .RegisterEndpointNotificationCallback(&client)
                .map_err(|e| DeviceError::Enumeration(e.to_string()))?;
        }

        // Coalesce bursts: a single BT reconnect fires added + state + default in a
        // row — wait a short window and drain, then re-enumerate once.
        while rx.recv().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(300));
            while rx.try_recv().is_ok() {}
            on_change();
        }

        unsafe {
            let _ = enumerator.UnregisterEndpointNotificationCallback(&client);
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn enumerate_audio_devices() -> Result<Vec<AudioDevice>, DeviceError> {
        Ok(vec![AudioDevice {
            id: "stub-output".into(),
            name: "Stub Output (non-Windows)".into(),
            device_type: DeviceType::Output,
            is_default: true,
            original_name: None,
            is_virtual: false,
        }])
    }

    pub fn watch_device_changes(_on_change: Box<dyn Fn() + Send>) -> Result<(), DeviceError> {
        Ok(()) // no endpoint notifications off Windows
    }
}

pub use platform::{enumerate_audio_devices, watch_device_changes};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn enumerate_returns_at_least_one_device() {
        // Requires COM to be initialized — done by ComGuard in production.
        // In tests on Windows there should always be at least one audio device.
        use crate::audio::wasapi::ComGuard;
        let _guard = ComGuard::init().expect("COM init");
        let devices = enumerate_audio_devices().expect("enumerate");
        assert!(
            !devices.is_empty(),
            "Expected at least one audio device on Windows"
        );
    }

    #[test]
    fn device_type_serializes() {
        let d = AudioDevice {
            id: "x".into(),
            name: "Test".into(),
            device_type: DeviceType::Output,
            is_default: false,
            original_name: None,
            is_virtual: false,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"output\""));
    }

    #[test]
    fn detects_virtual_devices() {
        for name in [
            "CABLE Output (VB-Audio Virtual Cable)",
            "Микрофон (MIXLINE Stream)",
            "VoiceMeeter Output",
            "Nodus Mic",
        ] {
            assert!(is_virtual_device(name), "expected virtual: {name}");
        }
        for name in ["Headphones (Galaxy Buds)", "Microphone (Blue Yeti)", "Speakers (Realtek)"] {
            assert!(!is_virtual_device(name), "expected physical: {name}");
        }
    }
}
