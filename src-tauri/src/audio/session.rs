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

pub const BUFFER_DURATION_MS: u64 = 20; // 10ms too tight for BT devices → underruns → crackling
pub const CHANNEL_CAPACITY: usize = 64;

/// Samples as interleaved f32.
pub type AudioFrame = Vec<f32>;

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

    /// Render audio to an output device.
    pub struct AudioRenderer {
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
    }

    impl AudioRenderer {
        pub fn new(device_id: impl Into<String>, format: AudioFormat) -> Self {
            Self {
                device_id: device_id.into(),
                format,
                stop_flag: Arc::new(AtomicBool::new(false)),
            }
        }

        /// Start rendering frames from `source`. `volume` and `muted` are applied per-sample.
        pub fn start(
            &self,
            mut source: tokio::sync::broadcast::Receiver<AudioFrame>,
            volume: Arc<std::sync::atomic::AtomicU32>,
            muted: Arc<AtomicBool>,
        ) {
            let device_id = self.device_id.clone();
            let format = self.format;
            let stop_flag = Arc::clone(&self.stop_flag);

            std::thread::spawn(move || {
                if let Err(e) =
                    run_render(device_id, format, stop_flag, &mut source, volume, muted)
                {
                    error!("audio render error: {e}");
                }
            });
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
        }
    }

    fn run_render(
        device_id: String,
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
        source: &mut tokio::sync::broadcast::Receiver<AudioFrame>,
        volume_atomic: Arc<std::sync::atomic::AtomicU32>,
        muted: Arc<AtomicBool>,
    ) -> Result<(), SessionError> {
        use crate::audio::wasapi::ComGuard;
        let _com = ComGuard::init()?;

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

            while !stop_flag.load(Ordering::SeqCst) {
                match source.try_recv() {
                    Ok(mut frame) => {
                        let is_muted = muted.load(Ordering::Relaxed);
                        let vol = f32::from_bits(volume_atomic.load(Ordering::Relaxed));

                        for sample in &mut frame {
                            *sample = if is_muted { 0.0 } else { *sample * vol };
                        }

                        let frames_to_write =
                            (frame.len() / format.channels as usize).min(buf_frames as usize);
                        if frames_to_write == 0 {
                            continue;
                        }

                        let padding = client.GetCurrentPadding().unwrap_or(0);
                        let available = buf_frames.saturating_sub(padding) as usize;
                        let write_count = frames_to_write.min(available);
                        if write_count == 0 {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        }

                        let buf_ptr = render
                            .GetBuffer(write_count as u32)
                            .map_err(|e| SessionError::Wasapi(WasapiError::Buffer(e.to_string())))?;

                        let dst = std::slice::from_raw_parts_mut(
                            buf_ptr as *mut f32,
                            write_count * format.channels as usize,
                        );
                        let src = &frame[..write_count * format.channels as usize];
                        dst.copy_from_slice(src);

                        render
                            .ReleaseBuffer(write_count as u32, 0)
                            .map_err(|e| SessionError::Wasapi(WasapiError::Buffer(e.to_string())))?;
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                        // No frame yet — sleep briefly. WASAPI shared mode handles
                        // underruns gracefully (inserts silence itself); don't pre-fill
                        // with zeros or the buffer fills up and drops incoming audio frames.
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break, // sender dropped
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

        pub fn start(
            &self,
            _source: tokio::sync::broadcast::Receiver<AudioFrame>,
            _volume: Arc<std::sync::atomic::AtomicU32>,
            _muted: Arc<AtomicBool>,
        ) {
        }

        pub fn stop(&self) {}
    }
}

pub use platform::{AppSessionControl, AudioRenderer, LoopbackCapture, find_device_for_exe};

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
