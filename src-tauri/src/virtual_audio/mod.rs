//! Nodus's own virtual audio devices and the link to the kernel driver:
//! the IOCTL control channel, the shared ring-buffer contract, and the
//! ring readers/writers behind the virtual mic / output. (Split out of `audio`.)

pub mod device_control;
#[cfg(target_os = "windows")]
pub mod endpoint_name;
pub(crate) mod ring_layout;
pub mod virtual_capture;
pub mod virtual_device;
pub mod virtual_render;
