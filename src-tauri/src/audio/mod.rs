//! Real Windows audio via WASAPI: COM setup, device enumeration, and the
//! capture/render sessions. (Nodus's own virtual devices live in `virtual_audio`.)

pub mod devices;
pub mod dsp;
/// Audio-path health counters — how much audio we actually lost (t30).
pub mod glitch;
/// MMCSS scheduling for the audio threads (t30).
pub mod mmcss;
pub mod session;
pub mod wasapi;
