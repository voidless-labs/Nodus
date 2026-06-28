/// Audio session — WASAPI capture (loopback + mic) and render.
///
/// Architecture:
///   AudioCapture::start() spawns a thread that reads from IAudioCaptureClient
///   and sends raw f32 frames through a channel.
///   AudioRender receives those frames and writes to IAudioRenderClient.
///   The routing engine wires captures → renders with volume/mute applied.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use thiserror::Error;
use tracing::{debug, error};

use super::wasapi::{AudioFormat, WasapiError};

// 10ms was too tight for BT devices, 20ms too tight for the kernel-ring source:
// the driver delivers audio in ~15.6ms timer-tick bursts, so the cushion must
// cover at least two ticks of producer jitter. 50ms is still inaudible latency.
pub const BUFFER_DURATION_MS: u64 = 50;
pub const CHANNEL_CAPACITY: usize = 64;

/// Samples as interleaved f32.
pub type AudioFrame = Vec<f32>;

/// RAII guard for 1 ms multimedia timer resolution. Windows' default scheduler
/// tick is ~15.6 ms, which turns every `thread::sleep(1ms)` in the audio paths
/// into a 15 ms stall — enough to starve a 50 ms device buffer under jitter.
/// Standard practice for audio apps; resolution is restored on drop.
#[cfg(target_os = "windows")]
pub struct TimerResolutionGuard;

#[cfg(target_os = "windows")]
impl TimerResolutionGuard {
    pub fn acquire() -> Self {
        unsafe {
            let _ = windows::Win32::Media::timeBeginPeriod(1);
        }
        TimerResolutionGuard
    }
}

#[cfg(target_os = "windows")]
impl Drop for TimerResolutionGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Media::timeEndPeriod(1);
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("WASAPI error: {0}")]
    Wasapi(#[from] WasapiError),
    #[error("device not available: {0}")]
    DeviceUnavailable(String),
    #[error("format mismatch")]
    FormatMismatch,
    #[error("session already running")]
    AlreadyRunning,
    #[error("channel error")]
    ChannelError,
}

// ── Windows implementation ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use tokio::sync::broadcast;
    use windows::{
        core::PCWSTR,
        Win32::Media::Audio::{
            IAudioCaptureClient, IAudioClient, IAudioRenderClient, ISimpleAudioVolume,
            IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
            AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
        },
        Win32::System::Com::{CoCreateInstance, CLSCTX_ALL},
    };

    // Completion handler for ActivateAudioInterfaceAsync (process loopback activation
    // is asynchronous). It just signals a Win32 event the capture thread waits on.
    #[windows::core::implement(windows::Win32::Media::Audio::IActivateAudioInterfaceCompletionHandler)]
    struct ActivationHandler {
        event: windows::Win32::Foundation::HANDLE,
    }

    impl windows::Win32::Media::Audio::IActivateAudioInterfaceCompletionHandler_Impl
        for ActivationHandler_Impl
    {
        fn ActivateCompleted(
            &self,
            _operation: Option<&windows::Win32::Media::Audio::IActivateAudioInterfaceAsyncOperation>,
        ) -> windows::core::Result<()> {
            unsafe { windows::Win32::System::Threading::SetEvent(self.event) }
        }
    }

    unsafe fn get_device_by_id(
        enumerator: &IMMDeviceEnumerator,
        device_id: &str,
    ) -> Result<windows::Win32::Media::Audio::IMMDevice, SessionError> {
        let wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
        enumerator
            .GetDevice(PCWSTR(wide.as_ptr()))
            .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))
    }

    fn make_waveformat(fmt: &AudioFormat) -> WAVEFORMATEX {
        let block_align = fmt.channels * (fmt.bits_per_sample / 8);
        WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: fmt.channels,
            nSamplesPerSec: fmt.sample_rate,
            nAvgBytesPerSec: fmt.sample_rate * block_align as u32,
            nBlockAlign: block_align,
            wBitsPerSample: fmt.bits_per_sample,
            cbSize: 0,
        }
    }

    /// Determine the format a capture on this device will actually produce.
    ///  - Render endpoint (loopback): the device's shared-mode mix format. Loopback
    ///    cannot AUTOCONVERT, so frames arrive in this format (always 32-bit float,
    ///    but channel count / sample rate vary per device). The renderer must be told
    ///    so it can AUTOCONVERT source→output correctly.
    ///  - Capture endpoint (mic/input): we capture with AUTOCONVERTPCM into our
    ///    normalized format, so report the default.
    pub fn get_device_capture_format(device_id: &str) -> Result<AudioFormat, SessionError> {
        use crate::audio::wasapi::ComGuard;
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{eRender, IMMEndpoint};
        use windows::Win32::System::Com::CoTaskMemFree;

        let _com = ComGuard::init()?;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let device = get_device_by_id(&enumerator, device_id)?;

            let endpoint: IMMEndpoint = device
                .cast()
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let flow = endpoint
                .GetDataFlow()
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            if flow != eRender {
                // Input device: capture converts to our normalized format.
                return Ok(AudioFormat::default());
            }

            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            let mix = client
                .GetMixFormat()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            let fmt = AudioFormat {
                sample_rate: (*mix).nSamplesPerSec,
                channels: (*mix).nChannels,
                bits_per_sample: 32, // WASAPI shared-mode mix is always 32-bit float
            };
            CoTaskMemFree(Some(mix as *mut _));
            Ok(fmt)
        }
    }

    /// Loopback capture from a render device (what Windows plays to that device).
    pub struct LoopbackCapture {
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        sender: Option<broadcast::Sender<AudioFrame>>,
        level: Arc<std::sync::atomic::AtomicU32>,
    }

    impl LoopbackCapture {
        pub fn new(device_id: impl Into<String>, format: AudioFormat) -> Self {
            Self {
                device_id: device_id.into(),
                format,
                stop_flag: Arc::new(AtomicBool::new(false)),
                sender: None,
                level: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }
        }

        pub fn current_level(&self) -> f32 {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }

        /// Subscribe to an already-running capture. Returns `None` if not started.
        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }

        /// Start capturing. Returns a receiver that yields audio frames.
        /// If already running, returns a new subscriber instead.
        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(sender) = &self.sender {
                return Ok(sender.subscribe());
            }
            let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx.clone());
            self.stop_flag.store(false, Ordering::SeqCst);

            let device_id = self.device_id.clone();
            let format = self.format;
            let stop_flag = Arc::clone(&self.stop_flag);
            let level = Arc::clone(&self.level);

            std::thread::spawn(move || {
                if let Err(e) = run_loopback_capture(device_id, format, stop_flag, tx, level) {
                    error!("loopback capture error: {e}");
                }
            });

            Ok(rx)
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
            self.level.store(0u32, Ordering::Relaxed);
        }
    }

    fn run_loopback_capture(
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        sender: tokio::sync::broadcast::Sender<AudioFrame>,
        level: Arc<std::sync::atomic::AtomicU32>,
    ) -> Result<(), SessionError> {
        use crate::audio::wasapi::ComGuard;
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{eRender, IMMEndpoint};
        use windows::Win32::System::Com::CoTaskMemFree;
        let _com = ComGuard::init()?;

        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;

            let device = get_device_by_id(&enumerator, &device_id)?;

            // Determine if this is an output (loopback) or input (direct capture) device
            let endpoint: IMMEndpoint = device
                .cast()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            let data_flow = endpoint
                .GetDataFlow()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            let is_output = data_flow == eRender;

            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let buf_duration = (BUFFER_DURATION_MS * 10_000) as i64; // 100-ns units

            // For output loopback: must use GetMixFormat() — AUTOCONVERTPCM is not supported
            //   with LOOPBACK flag.
            // For input (mic): use AUTOCONVERTPCM with our normalised format.
            let n_channels: usize;
            if is_output {
                let mix_fmt = client
                    .GetMixFormat()
                    .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
                n_channels = (*mix_fmt).nChannels as usize;
                let init_result = client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buf_duration,
                    0,
                    mix_fmt,
                    None,
                );
                CoTaskMemFree(Some(mix_fmt as *mut _));
                init_result
                    .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            } else {
                // Input device: direct capture with format auto-conversion
                let wfx = make_waveformat(&format);
                n_channels = format.channels as usize;
                client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                        buf_duration,
                        0,
                        &wfx,
                        None,
                    )
                    .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            }

            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            client
                .Start()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let mode = if is_output { "loopback" } else { "input" };
            debug!("capture ({mode}) started on device {device_id}");

            while !stop_flag.load(Ordering::SeqCst) {
                let mut data_ptr = std::ptr::null_mut();
                let mut frames_available = 0u32;
                let mut flags = 0u32;

                match capture.GetBuffer(
                    &mut data_ptr,
                    &mut frames_available,
                    &mut flags,
                    None,
                    None,
                ) {
                    Ok(()) if frames_available > 0 => {
                        let sample_count = frames_available as usize * n_channels;
                        let slice = std::slice::from_raw_parts(
                            data_ptr as *const f32,
                            sample_count,
                        );
                        let frame = slice.to_vec();
                        capture
                            .ReleaseBuffer(frames_available)
                            .map_err(|e| {
                                SessionError::Wasapi(WasapiError::Buffer(e.to_string()))
                            })?;
                        // Raw RMS level for VU meter, dBFS scale [-60, 0] → [0.0, 1.0].
                        // No gating or smoothing — show what the device actually captures.
                        let sum_sq: f32 = frame.iter().map(|s| s * s).sum();
                        let rms = (sum_sq / frame.len() as f32).sqrt();
                        let db = 20.0 * rms.max(1e-7_f32).log10();
                        let scaled = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                        level.store(scaled.to_bits(), Ordering::Relaxed);
                        // Ignore send error — receiver may have been dropped
                        let _ = sender.send(frame);
                    }
                    Ok(()) => {
                        // No audio frames — decay level towards zero
                        let prev = f32::from_bits(level.load(Ordering::Relaxed));
                        if prev > 0.001 {
                            level.store((prev * 0.8).to_bits(), Ordering::Relaxed);
                        } else {
                            level.store(0u32, Ordering::Relaxed);
                        }
                        std::thread::sleep(Duration::from_millis(BUFFER_DURATION_MS / 2));
                    }
                    Err(_) => {
                        level.store(0u32, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_millis(BUFFER_DURATION_MS));
                    }
                }
            }

            client.Stop().ok();
        }

        debug!("loopback capture stopped on device {device_id}");
        Ok(())
    }

    /// Per-app Windows audio session controller (mute/volume).
    /// The audio session COM object implements both IAudioSessionControl and ISimpleAudioVolume,
    /// so we can QI directly from the session control.
    #[derive(Clone)]
    pub struct AppSessionControl(ISimpleAudioVolume);

    impl AppSessionControl {
        pub fn set_mute(&self, muted: bool) {
            use windows::core::GUID;
            unsafe { let _ = self.0.SetMute(muted, std::ptr::null::<GUID>()); }
        }
        pub fn set_volume(&self, vol: f32) {
            use windows::core::GUID;
            unsafe { let _ = self.0.SetMasterVolume(vol.clamp(0.0, 1.0), std::ptr::null::<GUID>()); }
        }
    }

    // ISimpleAudioVolume (IUnknown) is Send+Sync in windows-rs.
    unsafe impl Send for AppSessionControl {}
    unsafe impl Sync for AppSessionControl {}

    /// Find the WASAPI output device ID that a given exe is currently playing audio to.
    /// Collects ALL PIDs for the exe (handles multi-process apps like Spotify).
    /// Returns (device_id, AppSessionControl) for mute/volume control.
    pub fn find_device_for_exe(exe_name: &str) -> Result<(String, AppSessionControl), SessionError> {
        use crate::audio::wasapi::ComGuard;
        use std::collections::HashSet;
        use windows::core::Interface;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Media::Audio::{
            eRender, IAudioSessionControl2, IAudioSessionManager2, ISimpleAudioVolume,
            DEVICE_STATE_ACTIVE,
        };
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let _com = ComGuard::init()?;
        let exe_lower = exe_name.to_lowercase();

        // Collect ALL PIDs for the target exe — multi-process apps (Spotify, Chrome)
        // may have the audio session on a different instance than the first one found.
        let pids: HashSet<u32> = unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut set = HashSet::new();
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(
                        entry.szExeFile.split(|&c| c == 0).next().unwrap_or(&[]),
                    );
                    if name.to_lowercase() == exe_lower {
                        set.insert(entry.th32ProcessID);
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            set
        };

        if pids.is_empty() {
            return Err(SessionError::DeviceUnavailable(format!("{exe_name} not running")));
        }

        // Find which output device any of those PIDs has an active audio session on.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;

            let collection = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;

            let count = collection.GetCount().unwrap_or(0);

            for i in 0..count {
                let device = match collection.Item(i) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                let session_mgr: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let sess_enum = match session_mgr.GetSessionEnumerator() {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let sess_count = sess_enum.GetCount().unwrap_or(0);

                for j in 0..sess_count {
                    let ctrl = match sess_enum.GetSession(j) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if pids.contains(&ctrl2.GetProcessId().unwrap_or(0)) {
                        let vol: ISimpleAudioVolume = match ctrl.cast() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let id_str = device.GetId()
                            .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?
                            .to_string()
                            .unwrap_or_default();
                        return Ok((id_str, AppSessionControl(vol)));
                    }
                }
            }

            Err(SessionError::DeviceUnavailable(format!(
                "no active audio session for {exe_name} (checked {} pids)", pids.len()
            )))
        }
    }

    /// Find the PID of a running process (by exe name) that currently has an active
    /// render audio session — the one actually producing sound. Process loopback then
    /// targets that PID's process tree, so we capture only this app's audio.
    pub fn find_audio_pid_for_exe(exe_name: &str) -> Result<u32, SessionError> {
        use crate::audio::wasapi::ComGuard;
        use std::collections::HashSet;
        use windows::core::Interface;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Media::Audio::{
            eRender, IAudioSessionControl2, IAudioSessionManager2, DEVICE_STATE_ACTIVE,
        };
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let _com = ComGuard::init()?;
        let exe_lower = exe_name.to_lowercase();

        let pids: HashSet<u32> = unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut set = HashSet::new();
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(
                        entry.szExeFile.split(|&c| c == 0).next().unwrap_or(&[]),
                    );
                    if name.to_lowercase() == exe_lower {
                        set.insert(entry.th32ProcessID);
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            set
        };

        if pids.is_empty() {
            return Err(SessionError::DeviceUnavailable(format!("{exe_name} not running")));
        }

        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let collection = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let count = collection.GetCount().unwrap_or(0);
            for i in 0..count {
                let device = match collection.Item(i) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let mgr: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let sess = match mgr.GetSessionEnumerator() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let sc = sess.GetCount().unwrap_or(0);
                for j in 0..sc {
                    let ctrl = match sess.GetSession(j) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let spid = ctrl2.GetProcessId().unwrap_or(0);
                    if pids.contains(&spid) {
                        return Ok(spid);
                    }
                }
            }
            // No active session yet (app not currently playing) — target any PID;
            // INCLUDE_TARGET_PROCESS_TREE picks up audio once it starts.
            match pids.iter().next() {
                Some(&p) => Ok(p),
                None => Err(SessionError::DeviceUnavailable(format!("no pid for {exe_name}"))),
            }
        }
    }

    /// Isolated per-application capture via WASAPI process loopback (Win10 20348+).
    /// Captures only the target PID's process tree — not the whole device mix —
    /// so per-route mute/volume act on this app's copy without touching its
    /// normal playback or other apps on the same device.
    pub struct ProcessLoopbackCapture {
        pid: u32,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        sender: Option<broadcast::Sender<AudioFrame>>,
        level: Arc<std::sync::atomic::AtomicU32>,
    }

    impl ProcessLoopbackCapture {
        pub fn new(pid: u32, format: AudioFormat) -> Self {
            Self {
                pid,
                format,
                stop_flag: Arc::new(AtomicBool::new(false)),
                sender: None,
                level: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }
        }

        pub fn current_level(&self) -> f32 {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }

        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }

        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(sender) = &self.sender {
                return Ok(sender.subscribe());
            }
            let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx.clone());
            self.stop_flag.store(false, Ordering::SeqCst);

            let pid = self.pid;
            let format = self.format;
            let stop_flag = Arc::clone(&self.stop_flag);
            let level = Arc::clone(&self.level);

            std::thread::spawn(move || {
                if let Err(e) = run_process_loopback(pid, format, stop_flag, tx, level) {
                    error!("process loopback capture error (pid {pid}): {e}");
                }
            });

            Ok(rx)
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
            self.level.store(0, Ordering::Relaxed);
        }
    }

    fn run_process_loopback(
        pid: u32,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        sender: broadcast::Sender<AudioFrame>,
        level: Arc<std::sync::atomic::AtomicU32>,
    ) -> Result<(), SessionError> {
        use crate::audio::wasapi::ComGuard;
        use windows::core::Interface;
        use windows::Win32::Foundation::{CloseHandle, FALSE, TRUE};
        use windows::Win32::Media::Audio::{
            ActivateAudioInterfaceAsync, IActivateAudioInterfaceCompletionHandler,
            AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
            AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        };
        use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

        let _com = ComGuard::init()?;

        unsafe {
            // Capture only this PID's process tree.
            let activation = AUDIOCLIENT_ACTIVATION_PARAMS {
                ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
                Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                    ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                        TargetProcessId: pid,
                        ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                    },
                },
            };

            // windows-rs has no safe VT_BLOB PROPVARIANT builder; lay out the raw bytes
            // (x64 PROPVARIANT) and pass a pointer. No owned PROPVARIANT is created, so
            // nothing tries to free the borrowed blob on drop.
            #[repr(C)]
            struct PropVariantBlob {
                vt: u16,
                r1: u16,
                r2: u16,
                r3: u16,
                cb_size: u32,
                _pad: u32,
                p_blob_data: *mut core::ffi::c_void,
            }
            const VT_BLOB: u16 = 65;
            let pv = PropVariantBlob {
                vt: VT_BLOB,
                r1: 0,
                r2: 0,
                r3: 0,
                cb_size: core::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                _pad: 0,
                p_blob_data: &activation as *const _ as *mut core::ffi::c_void,
            };
            let pv_ptr = &pv as *const PropVariantBlob as *const windows::core::PROPVARIANT;

            let event = CreateEventW(None, TRUE, FALSE, None)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            let handler: IActivateAudioInterfaceCompletionHandler =
                ActivationHandler { event }.into();

            let op = ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(pv_ptr),
                &handler,
            )
            .map_err(|e| {
                SessionError::DeviceUnavailable(format!("ActivateAudioInterfaceAsync: {e}"))
            })?;

            WaitForSingleObject(event, INFINITE);
            let _ = CloseHandle(event);

            let mut hr = windows::core::HRESULT(0);
            let mut unknown: Option<windows::core::IUnknown> = None;
            op.GetActivateResult(&mut hr, &mut unknown)
                .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
            hr.ok().map_err(|e| {
                SessionError::DeviceUnavailable(format!("process loopback activation: {e}"))
            })?;
            let client: IAudioClient = unknown
                .ok_or_else(|| {
                    SessionError::DeviceUnavailable("no audio client from process loopback".into())
                })?
                .cast()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            // Initialise loopback capture in our normalised format (Windows resamples
            // the app's audio to it — this also sidesteps device mix-format mismatch).
            let wfx = make_waveformat(&format);
            let n_channels = format.channels as usize;
            let buf_duration = (BUFFER_DURATION_MS * 10_000) as i64;
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buf_duration,
                    0,
                    &wfx,
                    None,
                )
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
            client
                .Start()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            debug!("process loopback capture started (pid {pid})");

            while !stop_flag.load(Ordering::SeqCst) {
                let mut data_ptr = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                match capture.GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None) {
                    Ok(()) if frames > 0 => {
                        let sample_count = frames as usize * n_channels;
                        let slice =
                            std::slice::from_raw_parts(data_ptr as *const f32, sample_count);
                        let frame = slice.to_vec();
                        capture
                            .ReleaseBuffer(frames)
                            .map_err(|e| SessionError::Wasapi(WasapiError::Buffer(e.to_string())))?;
                        let sum_sq: f32 = frame.iter().map(|s| s * s).sum();
                        let rms = (sum_sq / frame.len() as f32).sqrt();
                        let db = 20.0 * rms.max(1e-7_f32).log10();
                        let scaled = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                        level.store(scaled.to_bits(), Ordering::Relaxed);
                        let _ = sender.send(frame);
                    }
                    Ok(()) => {
                        let prev = f32::from_bits(level.load(Ordering::Relaxed));
                        if prev > 0.001 {
                            level.store((prev * 0.8).to_bits(), Ordering::Relaxed);
                        } else {
                            level.store(0, Ordering::Relaxed);
                        }
                        std::thread::sleep(Duration::from_millis(BUFFER_DURATION_MS / 2));
                    }
                    Err(_) => {
                        level.store(0, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_millis(BUFFER_DURATION_MS));
                    }
                }
            }

            client.Stop().ok();
        }

        debug!("process loopback capture stopped (pid {pid})");
        Ok(())
    }

    /// Render audio to an output device.
    pub struct AudioRenderer {
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        /// Post-volume RMS of the rendered stream, dBFS-scaled [0,1] (output VU, t16).
        level: Arc<std::sync::atomic::AtomicU32>,
    }

    impl AudioRenderer {
        pub fn new(device_id: impl Into<String>, format: AudioFormat) -> Self {
            Self {
                device_id: device_id.into(),
                format,
                stop_flag: Arc::new(AtomicBool::new(false)),
                level: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }
        }

        /// Current post-volume output level [0,1] for the VU meter on this route's
        /// destination (the engine aggregates per output device).
        pub fn current_level(&self) -> f32 {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }

        /// Start rendering frames from `source`. `volume` and `muted` are applied per-sample.
        pub fn start(
            &self,
            mut source: tokio::sync::broadcast::Receiver<AudioFrame>,
            volume: Arc<std::sync::atomic::AtomicU32>,
            muted: Arc<AtomicBool>,
            pan: Arc<std::sync::atomic::AtomicU32>,
        ) {
            let device_id = self.device_id.clone();
            let format = self.format;
            let stop_flag = Arc::clone(&self.stop_flag);
            let level = Arc::clone(&self.level);

            std::thread::spawn(move || {
                if let Err(e) =
                    run_render(device_id, format, stop_flag, &mut source, volume, muted, pan, level)
                {
                    error!("audio render error: {e}");
                }
            });
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_render(
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        source: &mut tokio::sync::broadcast::Receiver<AudioFrame>,
        volume_atomic: Arc<std::sync::atomic::AtomicU32>,
        muted: Arc<AtomicBool>,
        pan_atomic: Arc<std::sync::atomic::AtomicU32>,
        level: Arc<std::sync::atomic::AtomicU32>,
    ) -> Result<(), SessionError> {
        use crate::audio::wasapi::ComGuard;
        let _com = ComGuard::init()?;
        // 1 ms scheduler resolution: thread::sleep(1) is otherwise ~15.6 ms on
        // Windows, which starves the device buffer between wakeups.
        let _timer = TimerResolutionGuard::acquire();

        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;

            let device = get_device_by_id(&enumerator, &device_id)?;

            // Guard: verify this is a render endpoint — Initialize() on a capture endpoint
            // returns AUDCLNT_E_WRONG_ENDPOINT_TYPE (0x88890003).
            {
                use windows::core::Interface;
                use windows::Win32::Media::Audio::{eRender, IMMEndpoint};
                let endpoint: IMMEndpoint = device
                    .cast()
                    .map_err(|e| SessionError::DeviceUnavailable(e.to_string()))?;
                let flow = endpoint
                    .GetDataFlow()
                    .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;
                if flow != eRender {
                    return Err(SessionError::DeviceUnavailable(format!(
                        "device {device_id} is a capture endpoint — cannot render to it; \
                         use it as a Source node instead"
                    )));
                }
            }

            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let wfx = make_waveformat(&format);
            let buf_duration = (BUFFER_DURATION_MS * 10_000) as i64;

            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    buf_duration,
                    0,
                    &wfx,
                    None,
                )
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let render: IAudioRenderClient = client
                .GetService()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            let buf_frames = client
                .GetBufferSize()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            client
                .Start()
                .map_err(|e| SessionError::Wasapi(WasapiError::AudioClient(e.to_string())))?;

            debug!("render started on device {device_id}");

            // When the last frame arrived — the VU decays only after a real silence
            // gap, not between bursty chunks (which would make it sawtooth).
            let mut last_audio = std::time::Instant::now();

            while !stop_flag.load(Ordering::SeqCst) {
                match source.try_recv() {
                    Ok(mut frame) => {
                        let is_muted = muted.load(Ordering::Relaxed);
                        let vol = f32::from_bits(volume_atomic.load(Ordering::Relaxed));
                        let pan = f32::from_bits(pan_atomic.load(Ordering::Relaxed));

                        if is_muted {
                            for sample in &mut frame {
                                *sample = 0.0;
                            }
                        } else if format.channels == 2 && pan != 0.0 {
                            // Stereo balance: pan<0 attenuates right, pan>0 attenuates left.
                            let lg = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
                            let rg = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
                            for pair in frame.chunks_exact_mut(2) {
                                pair[0] *= vol * lg;
                                pair[1] *= vol * rg;
                            }
                        } else {
                            for sample in &mut frame {
                                *sample *= vol;
                            }
                        }

                        // Post-volume RMS → dBFS [-60,0] → [0,1], smoothed (fast
                        // attack / slow release) so the output VU isn't jerky.
                        if !frame.is_empty() {
                            let sum_sq: f32 = frame.iter().map(|s| s * s).sum();
                            let rms = (sum_sq / frame.len() as f32).sqrt();
                            let db = 20.0 * rms.max(1e-7_f32).log10();
                            let raw = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                            let prev = f32::from_bits(level.load(Ordering::Relaxed));
                            let smoothed = if raw >= prev { raw } else { prev * 0.82 + raw * 0.18 };
                            level.store(smoothed.to_bits(), Ordering::Relaxed);
                            last_audio = std::time::Instant::now();
                        }

                        // Write the WHOLE frame, in parts if the WASAPI buffer is
                        // momentarily full. Dropping the remainder (the old behavior)
                        // audibly crackles with bursty sources — the kernel ring
                        // delivers 2-3 chunks per ~15.6 ms timer tick and the tails
                        // didn't fit into the 20 ms device buffer.
                        let ch = format.channels as usize;
                        let total_frames = frame.len() / ch;
                        let mut written = 0usize;
                        while written < total_frames && !stop_flag.load(Ordering::SeqCst) {
                            let padding = client.GetCurrentPadding().unwrap_or(0);
                            let available = buf_frames.saturating_sub(padding) as usize;
                            if available == 0 {
                                std::thread::sleep(Duration::from_millis(1));
                                continue;
                            }
                            let n = (total_frames - written).min(available);

                            let buf_ptr = render
                                .GetBuffer(n as u32)
                                .map_err(|e| SessionError::Wasapi(WasapiError::Buffer(e.to_string())))?;

                            let dst = std::slice::from_raw_parts_mut(buf_ptr as *mut f32, n * ch);
                            dst.copy_from_slice(&frame[written * ch..(written + n) * ch]);

                            render
                                .ReleaseBuffer(n as u32, 0)
                                .map_err(|e| SessionError::Wasapi(WasapiError::Buffer(e.to_string())))?;
                            written += n;
                        }
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                        // No frame yet — sleep briefly. WASAPI shared mode handles
                        // underruns gracefully (inserts silence itself); don't pre-fill
                        // with zeros or the buffer fills up and drops incoming frames.
                        // Decay the VU only after a real silence gap (>40 ms), not
                        // between bursty chunks — otherwise the meter sawtooths.
                        if last_audio.elapsed() > Duration::from_millis(40) {
                            let prev = f32::from_bits(level.load(Ordering::Relaxed));
                            level.store(if prev > 0.001 { (prev * 0.9).to_bits() } else { 0 }, Ordering::Relaxed);
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                        // Fell behind the producer — e.g. the ~700 ms WASAPI init
                        // let the 64-frame channel overflow while music was already
                        // playing. Skip the dropped frames and keep rendering;
                        // tearing the render down here was a real "no audio" bug.
                        debug!("render lagged {n} frames on {device_id}, continuing");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break, // sender dropped
                }
            }

            client.Stop().ok();
        }

        debug!("render stopped on device {device_id}");
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub mod platform {
    use super::*;

    #[derive(Clone)]
    pub struct AppSessionControl;
    impl AppSessionControl {
        pub fn set_mute(&self, _: bool) {}
        pub fn set_volume(&self, _: f32) {}
    }

    pub fn find_device_for_exe(_exe_name: &str) -> Result<(String, AppSessionControl), SessionError> {
        Err(SessionError::DeviceUnavailable("process loopback not supported on non-Windows".into()))
    }

    pub fn find_audio_pid_for_exe(_exe_name: &str) -> Result<u32, SessionError> {
        Err(SessionError::DeviceUnavailable("process loopback not supported on non-Windows".into()))
    }

    pub fn get_device_capture_format(_device_id: &str) -> Result<AudioFormat, SessionError> {
        Ok(AudioFormat::default())
    }

    pub struct ProcessLoopbackCapture {
        sender: Option<tokio::sync::broadcast::Sender<AudioFrame>>,
    }

    impl ProcessLoopbackCapture {
        pub fn new(_pid: u32, _format: AudioFormat) -> Self {
            Self { sender: None }
        }
        pub fn current_level(&self) -> f32 { 0.0 }
        pub fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }
        pub fn start(
            &mut self,
        ) -> Result<tokio::sync::broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(sender) = &self.sender {
                return Ok(sender.subscribe());
            }
            let (tx, rx) = tokio::sync::broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx);
            Ok(rx)
        }
        pub fn stop(&self) {}
    }

    pub struct LoopbackCapture {
        pub device_id: String,
        sender: Option<tokio::sync::broadcast::Sender<AudioFrame>>,
    }

    impl LoopbackCapture {
        pub fn new(device_id: impl Into<String>, _format: AudioFormat) -> Self {
            Self { device_id: device_id.into(), sender: None }
        }

        pub fn current_level(&self) -> f32 { 0.0 }

        pub fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }

        pub fn start(
            &mut self,
        ) -> Result<tokio::sync::broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(sender) = &self.sender {
                return Ok(sender.subscribe());
            }
            let (tx, rx) = tokio::sync::broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx);
            Ok(rx)
        }

        pub fn stop(&self) {}
    }

    pub struct AudioRenderer {
        pub device_id: String,
    }

    impl AudioRenderer {
        pub fn new(device_id: impl Into<String>, _format: AudioFormat) -> Self {
            Self { device_id: device_id.into() }
        }

        pub fn current_level(&self) -> f32 { 0.0 }

        pub fn start(
            &self,
            _source: tokio::sync::broadcast::Receiver<AudioFrame>,
            _volume: Arc<std::sync::atomic::AtomicU32>,
            _muted: Arc<AtomicBool>,
            _pan: Arc<std::sync::atomic::AtomicU32>,
        ) {
        }

        pub fn stop(&self) {}
    }
}

pub use platform::{
    find_audio_pid_for_exe, find_device_for_exe, get_device_capture_format, AppSessionControl,
    AudioRenderer, LoopbackCapture, ProcessLoopbackCapture,
};

/// Convert f32 volume [0.0 .. 1.0] to AtomicU32 for lock-free sharing.
pub fn volume_to_atomic(v: f32) -> u32 {
    v.to_bits()
}

/// Clamp volume to [0.0, 1.0].
pub fn clamp_volume(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_round_trips_through_bits() {
        let v = 0.75f32;
        let bits = volume_to_atomic(v);
        let back = f32::from_bits(bits);
        assert!((back - v).abs() < f32::EPSILON);
    }

    #[test]
    fn clamp_volume_bounds() {
        assert_eq!(clamp_volume(-0.5), 0.0);
        assert_eq!(clamp_volume(1.5), 1.0);
        assert!((clamp_volume(0.5) - 0.5).abs() < f32::EPSILON);
    }
}
