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
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::audio::session::{AudioFrame, SessionError, CHANNEL_CAPACITY};

// ── Windows-only implementation ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    // Shared contract mirror (header layout + constants) — see ring_layout.rs.
    use crate::virtual_audio::ring_layout::{
        render_section_name, RingHeader, RING_BYTES, RING_MAGIC, RING_VERSION,
    };
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

    impl RingView {
        fn open(ring_id: u32) -> Result<Self, SessionError> {
            let section = render_section_name(ring_id);
            let name: Vec<u16> = format!("{section}\0").encode_utf16().collect();

            unsafe {
                let handle =
                    OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr()))
                        .map_err(|e| {
                            SessionError::DeviceUnavailable(format!(
                                "{section} section not found — is nodus_audio.sys loaded? {e}"
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

    // Layout/constant tests for the shared contract mirror live in ring_layout.rs.

    // ── Public capture handle ────────────────────────────────────────────────

    pub struct VirtualCapture {
        /// Kernel render ring to read — 0 = static virtual speaker, 1..8 = a
        /// dynamically-created virtual output (its driver device id). (t8)
        ring_id: u32,
        stop_flag: Arc<AtomicBool>,
        sender: Option<broadcast::Sender<AudioFrame>>,
        /// RMS level [0,1] (dBFS-scaled) of the last read chunk, for the source
        /// VU meter (t21). f32 stored as bits; updated on the reader thread.
        level: Arc<AtomicU32>,
    }

    impl VirtualCapture {
        pub fn new(ring_id: u32) -> Self {
            Self {
                ring_id,
                stop_flag: Arc::new(AtomicBool::new(false)),
                sender: None,
                level: Arc::new(AtomicU32::new(0)),
            }
        }

        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> {
            self.sender.as_ref().map(|s| s.subscribe())
        }

        /// Current RMS level [0,1] for the VU meter (t21).
        pub fn current_level(&self) -> f32 {
            f32::from_bits(self.level.load(Ordering::Relaxed))
        }

        /// Start pumping audio frames from the kernel ring into the broadcast channel.
        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            if let Some(ref s) = self.sender {
                return Ok(s.subscribe());
            }

            // Verify driver is present before spawning thread
            let ring_id = self.ring_id;
            let view = RingView::open(ring_id)?;

            let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
            self.sender = Some(tx.clone());
            self.stop_flag.store(false, Ordering::SeqCst);

            let stop = Arc::clone(&self.stop_flag);
            let level = Arc::clone(&self.level);

            std::thread::spawn(move || {
                const FRAMES_PER_CHUNK: usize = 480; // 10 ms at 48 kHz
                const SAMPLES_PER_CHUNK: usize = FRAMES_PER_CHUNK * 2; // stereo
                const BYTES_PER_CHUNK: u64 = (SAMPLES_PER_CHUNK * 2) as u64; // 16-bit

                // Without this the 2 ms poll below is really ~15.6 ms and the
                // ring is forwarded in bursts that starve the render buffer.
                let _timer = crate::audio::session::TimerResolutionGuard::acquire();

                let mut local_read: u64 = view.write_counter();
                // Chunks arrive in ~10 ms bursts; only a real gap decays the VU,
                // so it doesn't flicker between reads. (t21)
                let mut empty_reads: u32 = 0;

                debug!("VirtualCapture: reading from {} ring", render_section_name(ring_id));

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
                        // RMS level for the source VU meter, dBFS [-60,0] → [0,1] —
                        // raw per chunk, like the real captures; the shared CSS
                        // meter transition does the visual smoothing (t21).
                        empty_reads = 0;
                        let sum_sq: f32 = frame.iter().map(|s| s * s).sum();
                        let rms = (sum_sq / frame.len() as f32).sqrt();
                        let db = 20.0 * rms.max(1e-7_f32).log10();
                        level.store(((db + 60.0) / 60.0).clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                        let _ = tx.send(frame);
                    } else {
                        // Only a real gap (>~24 ms) decays the meter, so it stays
                        // smooth between the ~10 ms chunk reads.
                        empty_reads += 1;
                        if empty_reads > 12 {
                            let prev = f32::from_bits(level.load(Ordering::Relaxed));
                            level.store(
                                if prev > 0.001 { (prev * 0.85).to_bits() } else { 0 },
                                Ordering::Relaxed,
                            );
                        }
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
        pub fn new(_ring_id: u32) -> Self { Self }
        pub fn subscribe(&self) -> Option<broadcast::Receiver<AudioFrame>> { None }
        pub fn current_level(&self) -> f32 { 0.0 }
        pub fn start(&mut self) -> Result<broadcast::Receiver<AudioFrame>, SessionError> {
            Err(SessionError::DeviceUnavailable("VirtualCapture not supported on non-Windows".into()))
        }
        pub fn stop(&self) {}
    }
}

pub use platform::VirtualCapture;
