//! Real Windows audio via WASAPI: COM setup, device enumeration, and the
//! capture/render sessions. (Nodus's own virtual devices live in `virtual_audio`.)

pub mod devices;
pub mod session;
pub mod wasapi;
