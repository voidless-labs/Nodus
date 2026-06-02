/// WASAPI COM initialization and core audio client helpers.
///
/// All WASAPI calls must happen on a thread that has called CoInitialize.
/// This module provides a guard type that manages that lifecycle.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WasapiError {
    #[error("COM initialization failed: {0}")]
    ComInit(String),
    #[error("device enumerator creation failed: {0}")]
    EnumeratorFailed(String),
    #[error("property store error: {0}")]
    PropertyStore(String),
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("audio client error: {0}")]
    AudioClient(String),
    #[error("format not supported")]
    FormatNotSupported,
    #[error("buffer error: {0}")]
    Buffer(String),
}

/// RAII guard that calls CoUninitialize on drop.
/// One per thread that interacts with WASAPI.
#[cfg(target_os = "windows")]
pub struct ComGuard;

#[cfg(target_os = "windows")]
impl ComGuard {
    /// Initialize COM in multithreaded apartment mode.
    pub fn init() -> Result<Self, WasapiError> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            // S_FALSE means already initialized — that's fine.
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| WasapiError::ComInit(e.to_string()))?;
        }
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ComGuard {
    fn drop(&mut self) {
        use windows::Win32::System::Com::CoUninitialize;
        unsafe { CoUninitialize() };
    }
}

#[cfg(not(target_os = "windows"))]
pub struct ComGuard;

#[cfg(not(target_os = "windows"))]
impl ComGuard {
    pub fn init() -> Result<Self, WasapiError> {
        Ok(Self)
    }
}

/// Shared audio format used by the routing engine.
/// We normalise everything to 32-bit float PCM, 2-channel, 48 kHz.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 32,
        }
    }
}

impl AudioFormat {
    pub fn frame_size_bytes(&self) -> usize {
        (self.channels as usize) * (self.bits_per_sample as usize / 8)
    }

    pub fn bytes_per_second(&self) -> usize {
        self.sample_rate as usize * self.frame_size_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_format_frame_size() {
        let fmt = AudioFormat::default();
        // 2 channels × 4 bytes (f32) = 8 bytes per frame
        assert_eq!(fmt.frame_size_bytes(), 8);
    }

    #[test]
    fn default_format_bytes_per_second() {
        let fmt = AudioFormat::default();
        // 48000 frames × 8 bytes = 384000
        assert_eq!(fmt.bytes_per_second(), 384_000);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn com_guard_init_succeeds() {
        let guard = ComGuard::init();
        assert!(guard.is_ok());
    }
}
