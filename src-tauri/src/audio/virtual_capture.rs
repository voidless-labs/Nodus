/// virtual_capture.rs — Reads PCM audio from the Nodus kernel driver's shared ring buffer
/// and feeds it into the existing routing broadcast channel.
///
/// The driver (nodus_audio.sys) exposes one named file-mapping section per virtual
/// device: "Global\NodusRing-<id>" (id = 0 for the single Phase-1 device), holding a
/// NODUS_RING_BUFFER v2 struct — see driver/nodus_audio/common.h, which is the
/// authoritative layout. The ring carries the endpoint's fixed render format
/// (48 kHz, 2 ch, 16-bit PCM); we convert to interleaved f32 for the engine.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::broadcast;
use tracing::{debug, warn};

use super::session::{AudioFrame, SessionError, CHANNEL_CAPACITY};

// Mirrors common.h — bump together with NODUS_RING_VERSION there.
const RING_MAGIC: u32 = 0x4E4F_4455; // 'NODU'
const RING_VERSION: u32 = 2;
const RING_BYTES: usize = 384_000; // 2 s of 48 kHz stereo 16-bit
const SECTION_NAME: &str = "Global\\NodusRing-0";

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

    // Matches NODUS_RING_BUFFER (pack(8)) in common.h: 64-byte header + data.
    #[repr(C)]
    struct RingHeader {
        magic: u32,           // offset 0
        version: u32,         // offset 4
        sample_rate: u32,     // offset 8
        channels: u16,        // offset 12
        bits_per_sample: u16, // offset 14
        ring_bytes: u32,      // offset 16
        _reserved0: u32,      // offset 20
        write_bytes: u64,     // offset 24 — driver's monotonic producer counter
        read_bytes: u64,      // offset 32 — advisory, unused by us
        _reserved1: [u64; 3], // offset 40
        data: [u8; RING_BYTES], // offset 64
    }

    impl RingView {
        fn open() -> Result<Self, SessionError> {
            let name: Vec<u16> = format!("{SECTION_NAME}\0").encode_utf16().collect();

            unsafe {
                let handle =
                    OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr()))
                        .map_err(|e| {
                            SessionError::DeviceUnavailable(format!(
                                "{SECTION_NAME} section not found — is nodus_audio.sys loaded? {e}"
                            ))
                        })?;

                let ptr = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
                if ptr.Value.is_null() {
                    return Err(SessionError::DeviceUnavailable(
                        "MapViewOfFile failed".into(),
                    ));
                }

                let ring = &*(ptr.Value as *const RingHeader);
                if ring.magic != RING_MAGIC || ring.version != RING_VERSION {
                    return Err(SessionError::DeviceUnavailable(format!(
                        "ring header mismatch (magic 0x{:08X}, version {}) — driver/app version skew",
                        ring.magic, ring.version
                    )));
                }
                if ring.sample_rate != 48_000
                    || ring.channels != 2
                    || ring.bits_per_sample != 16
                    || ring.ring_bytes as usize != RING_BYTES
                {
                    return Err(SessionError::DeviceUnavailable(format!(
                        "unexpected ring format: {} Hz, {} ch, {} bit, {} bytes",
                        ring.sample_rate, ring.channels, ring.bits_per_sample, ring.ring_bytes
                    )));
                }

                Ok(RingView { ptr: ring as *const RingHeader, _handle: handle })
            }
        }

        #[inline]
        fn header(&self) -> &RingHeader {
            unsafe { &*self.ptr }
        }

        /// Driver's monotonic write counter (bytes ever produced).
        #[inline]
        fn write_counter(&self) -> u64 {
            unsafe { std::ptr::read_volatile(&self.header().write_bytes) }
        }

        /// How many unread bytes sit between our cursor and the driver's counter.
        #[inline]
        fn available(&self, local_read: u64) -> u64 {
            self.write_counter().saturating_sub(local_read)
        }

        /// Copy 16-bit PCM starting at `local_read` and convert to f32 into `out`.
        fn read_chunk(&self, local_read: u64, out: &mut [f32]) {
            let h = self.header();
            let n_bytes = out.len() * 2; // one i16 per f32 sample
            let mut tmp = vec![0u8; n_bytes];

            // The ring wraps at most once per chunk → max two copy spans.
            let mut src = (local_read % RING_BYTES as u64) as usize;
            let mut copied = 0usize;
            while copied < n_bytes {
                let span = (n_bytes - copied).min(RING_BYTES - src);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        h.data.as_ptr().add(src),
                        tmp.as_mut_ptr().add(copied),
                        span,
                    );
                }
                copied += span;
                src = (src + span) % RING_BYTES;
            }

            for (i, sample) in out.iter_mut().enumerate() {
                let v = i16::from_le_bytes([tmp[2 * i], tmp[2 * i + 1]]);
                *sample = f32::from(v) / 32_768.0;
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

    // The driver asserts the same offsets with C_ASSERT in ring.cpp — both sides
    // pin the contract from common.h independently.
    #[cfg(test)]
    mod layout_tests {
        use super::*;
        use std::mem::{offset_of, size_of};

        #[test]
        fn ring_header_layout_matches_common_h() {
            assert_eq!(offset_of!(RingHeader, magic), 0);
            assert_eq!(offset_of!(RingHeader, version), 4);
            assert_eq!(offset_of!(RingHeader, sample_rate), 8);
            assert_eq!(offset_of!(RingHeader, channels), 12);
            assert_eq!(offset_of!(RingHeader, bits_per_sample), 14);
            assert_eq!(offset_of!(RingHeader, ring_bytes), 16);
            assert_eq!(offset_of!(RingHeader, write_bytes), 24);
            assert_eq!(offset_of!(RingHeader, read_bytes), 32);
            assert_eq!(offset_of!(RingHeader, data), 64);
            assert_eq!(size_of::<RingHeader>(), 64 + RING_BYTES);
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
                const FRAMES_PER_CHUNK: usize = 480; // 10 ms at 48 kHz
                const SAMPLES_PER_CHUNK: usize = FRAMES_PER_CHUNK * 2; // stereo
                const BYTES_PER_CHUNK: u64 = (SAMPLES_PER_CHUNK * 2) as u64; // 16-bit

                // Without this the 2 ms poll below is really ~15.6 ms and the
                // ring is forwarded in bursts that starve the render buffer.
                let _timer = crate::audio::session::TimerResolutionGuard::acquire();

                let mut local_read: u64 = view.write_counter();

                debug!("VirtualCapture: reading from {SECTION_NAME} ring");

                while !stop.load(Ordering::SeqCst) {
                    let avail = view.available(local_read);

                    // If we fell behind by most of the ring the oldest bytes are
                    // already being overwritten — jump close to the live edge.
                    if avail > (RING_BYTES as u64) * 3 / 4 {
                        warn!("VirtualCapture: reader lagged, resyncing to live edge");
                        local_read = view.write_counter().saturating_sub(BYTES_PER_CHUNK);
                        continue;
                    }

                    if avail >= BYTES_PER_CHUNK {
                        let mut frame = vec![0f32; SAMPLES_PER_CHUNK];
                        view.read_chunk(local_read, &mut frame);
                        local_read += BYTES_PER_CHUNK;
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

#[cfg(test)]
mod tests {
    use super::*;

    // The layout constants must stay in lockstep with driver/nodus_audio/common.h.
    #[test]
    fn ring_constants_match_contract() {
        assert_eq!(RING_MAGIC, 0x4E4F_4455);
        assert_eq!(RING_VERSION, 2);
        // 2 seconds * 48000 frames * 2 ch * 2 bytes
        assert_eq!(RING_BYTES, 2 * 48_000 * 2 * 2);
        assert_eq!(SECTION_NAME, "Global\\NodusRing-0");
    }

}
