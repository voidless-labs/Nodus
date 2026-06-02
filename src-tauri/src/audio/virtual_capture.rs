/// virtual_capture.rs — Reads PCM audio from the Nodus kernel driver's shared ring buffer
/// and feeds it into the existing routing broadcast channel.
///
/// The driver (nodus_audio.sys) exposes a named file-mapping section
/// "Global\NodusVirtualAudio" containing a NODUS_RING_BUFFER struct.
/// We open it read-only, spin on WriteBytes advancing, and push frames.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::broadcast;
use tracing::debug;

use super::session::{AudioFrame, SessionError, CHANNEL_CAPACITY};

// Matches NODUS_RING_BUFFER layout in common.h  (little-endian, pack(4))
const RING_MAGIC: u32 = 0x4E4F4455; // 'NODU'
const RING_BYTES: usize = 48000 * 2 * 4 * 2; // 768 000 bytes — 2 s stereo f32 48 kHz

// ── Windows-only implementation ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use windows::{
        core::PCWSTR,
        Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ},
    };

    /// A live view into the kernel driver's shared ring buffer.
    /// Stays open for the lifetime of the VirtualCapture session.
    struct RingView {
        ptr: *const RingHeader,
        _handle: windows::Win32::Foundation::HANDLE,
    }

    // SAFETY: We only read from the mapping; the driver writes from kernel space.
    unsafe impl Send for RingView {}
    unsafe impl Sync for RingView {}

    // Matches NODUS_RING_BUFFER in common.h
    #[repr(C, packed(4))]
    struct RingHeader {
        magic: u32,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        ring_bytes: u32,
        write_bytes: u32, // volatile — driver increments
        read_bytes: u32,  // we update this locally (advisory only)
        data: [u8; RING_BYTES],
    }

    impl RingView {
        fn open() -> Result<Self, SessionError> {
            let name: Vec<u16> = "Global\\NodusVirtualAudio\0"
                .encode_utf16()
                .collect();

            unsafe {
                let handle =
                    OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr()))
                        .map_err(|e| {
                            SessionError::DeviceUnavailable(format!(
                                "NodusVirtualAudio section not found — is nodus_audio.sys loaded? {e}"
                            ))
                        })?;

                let ptr = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
                if ptr.Value.is_null() {
                    return Err(SessionError::DeviceUnavailable(
                        "MapViewOfFile failed".into(),
                    ));
                }

                let ring = &*(ptr.Value as *const RingHeader);
                if ring.magic != RING_MAGIC {
                    return Err(SessionError::DeviceUnavailable(
                        "bad magic — driver version mismatch".into(),
                    ));
                }

                Ok(RingView { ptr: ring as *const RingHeader, _handle: handle })
            }
        }

        #[inline]
        fn header(&self) -> &RingHeader {
            unsafe { &*self.ptr }
        }

        /// How many bytes are available to read.
        #[inline]
        fn available(&self, local_read: u32) -> u32 {
            let write = unsafe {
                std::ptr::read_volatile(&self.header().write_bytes)
            };
            write.wrapping_sub(local_read).min(RING_BYTES as u32)
        }

        /// Copy `n` bytes starting at `local_read` into `out`.
        fn read_bytes(&self, local_read: u32, out: &mut [f32]) {
            let h = self.header();
            let n = out.len() * 4;
            let mut src = (local_read as usize) % RING_BYTES;
            let dst = unsafe {
                std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, n)
            };
            for byte in dst.iter_mut() {
                *byte = unsafe { std::ptr::read_volatile(&h.data[src]) };
                src = (src + 1) % RING_BYTES;
            }
        }
    }

    impl Drop for RingView {
        fn drop(&mut self) {
            unsafe {
                let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.ptr as *mut _,
                });
            }
        }
    }

    // ── Public capture handle ────────────────────────────────────────────────

    pub struct VirtualCapture {
        stop_flag: Arc<AtomicBool>,
        sender: Option<broadcast::Sender<AudioFrame>>,
    }

    impl VirtualCapture {
        pub fn new() -> Self {
            Self {
                stop_flag: Arc::new(AtomicBool::new(false)),
                sender: None,
            }
        }

        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }

        /// Start pumping audio frames from the kernel ring into the broadcast channel.
        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(ref s) = self.sender {
                return Ok(s.subscribe());
            }

            // Verify driver is present before spawning thread
            let view = RingView::open()?;

            let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx.clone());
            self.stop_flag.store(false, Ordering::SeqCst);

            let stop = Arc::clone(&self.stop_flag);

            std::thread::spawn(move || {
                let mut local_read: u32 = unsafe {
                    std::ptr::read_volatile(&view.header().write_bytes)
                };
                const FRAMES_PER_CHUNK: usize = 480; // 10 ms at 48 kHz
                const SAMPLES_PER_CHUNK: usize = FRAMES_PER_CHUNK * 2; // stereo
                const BYTES_PER_CHUNK: u32 = (SAMPLES_PER_CHUNK * 4) as u32;

                debug!("VirtualCapture: reading from NodusVirtualAudio ring");

                while !stop.load(Ordering::SeqCst) {
                    if view.available(local_read) >= BYTES_PER_CHUNK {
                        let mut frame = vec![0f32; SAMPLES_PER_CHUNK];
                        view.read_bytes(local_read, &mut frame);
                        local_read = local_read.wrapping_add(BYTES_PER_CHUNK);
                        let _ = tx.send(frame);
                    } else {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                debug!("VirtualCapture: stopped");
            });

            Ok(rx)
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod platform {
    use super::*;

    pub struct VirtualCapture;
    impl VirtualCapture {
        pub fn new() -> Self { Self }
        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> { None }
        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            Err(SessionError::DeviceUnavailable("VirtualCapture not supported on non-Windows".into()))
        }
        pub fn stop(&self) {}
    }
}

pub use platform::VirtualCapture;
