/// virtual_render.rs — Writes routed PCM audio into the Nodus kernel driver's
/// virtual-microphone ring buffer ("Global\NodusRing-mic-0").
///
/// Counterpart of virtual_capture.rs with REVERSED counter roles (see
/// ring_layout.rs): here Nodus is the PRODUCER — we advance `write_bytes`
/// (monotonic byte counter, ring index = counter % RING_BYTES) and the driver
/// is the consumer — its PASSIVE-level thread advances `read_bytes`, but only
/// while some application actually records from the virtual mic.
///
/// Publication order per chunk (same as the ring_tone.rs reference tool):
///   1. copy PCM into `data` (max two spans across the wrap),
///   2. Release fence,
///   3. volatile store of `write_bytes` (driver reads it with a barrier).
///
/// Ring format is fixed by the contract: 48 kHz, stereo, 16-bit PCM LE.
/// Incoming frames arrive in the SOURCE capture format (f32 interleaved,
/// arbitrary rate/channels), so the writer converts: per-route mute/volume/pan
/// first (exact run_render semantics), then channel down/up-mix to stereo,
/// then an MVP linear-interpolation resample to 48 kHz if needed, then
/// f32 → i16 with clamping.
///
/// Driverless behaviour: opening the section happens INSIDE the writer thread.
/// If the section is missing (driver not installed / device stopped) the
/// thread logs one warning and exits — the route stays alive, frames simply
/// pile up unread in the broadcast channel (which tolerates slow/absent
/// receivers); no audio reaches the virtual mic, the app keeps running.
///
/// No pacing of our own: frames already arrive at real-time rate from the
/// capture side, so we write as they come. If our write counter runs more
/// than a full ring ahead of the driver's read counter (nobody is recording
/// from the mic), that is NOT an error — we keep writing and the driver
/// resyncs to the live edge on its own when capture starts.

use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};

use tokio::sync::broadcast;

use super::session::AudioFrame;
use super::wasapi::AudioFormat;

/// Fixed sample rate of the mic ring (see ring_layout.rs / common.h).
const RING_SAMPLE_RATE: u32 = 48_000;

// ── Pure conversion helpers (platform-independent, unit-tested) ──────────────

/// f32 sample in [-1.0, 1.0] → 16-bit PCM with clamping.
#[inline]
fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0) as i16
}

/// Apply per-route mute/volume/pan with the exact semantics of
/// `session::run_render`: mute zeroes everything; pan applies only to
/// 2-channel frames (pan<0 attenuates right, pan>0 attenuates left);
/// otherwise every sample is scaled by `vol`.
fn apply_route_dsp(frame: &mut [f32], channels: u16, is_muted: bool, vol: f32, pan: f32) {
    if is_muted {
        for sample in frame.iter_mut() {
            *sample = 0.0;
        }
    } else if channels == 2 && pan != 0.0 {
        let lg = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let rg = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        for pair in frame.chunks_exact_mut(2) {
            pair[0] *= vol * lg;
            pair[1] *= vol * rg;
        }
    } else {
        for sample in frame.iter_mut() {
            *sample *= vol;
        }
    }
}

/// Convert an interleaved frame to stereo: 2 ch → unchanged, 1 ch → duplicate
/// each sample, N>2 ch → keep the first two channels of every frame.
fn to_stereo(frame: &[f32], channels: u16) -> Vec<f32> {
    match channels {
        2 => frame.to_vec(),
        0 | 1 => {
            let mut out = Vec::with_capacity(frame.len() * 2);
            for &s in frame {
                out.push(s);
                out.push(s);
            }
            out
        }
        n => {
            let n = n as usize;
            let frames = frame.len() / n;
            let mut out = Vec::with_capacity(frames * 2);
            for f in 0..frames {
                out.push(frame[f * n]);
                out.push(frame[f * n + 1]);
            }
            out
        }
    }
}

/// MVP resampler: simple linear interpolation on stereo interleaved input.
/// Stateless per chunk (no phase carry-over between chunks) — good enough for
/// the MVP virtual-mic path; replace with a windowed-sinc resampler later.
/// Output length: floor(in_frames * to_rate / from_rate) frames.
fn resample_linear_stereo(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let in_frames = input.len() / 2;
    if from_rate == to_rate || in_frames == 0 {
        return input.to_vec();
    }
    let out_frames = (in_frames as u64 * to_rate as u64 / from_rate as u64) as usize;
    let step = from_rate as f64 / to_rate as f64;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * step;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let next = (idx + 1).min(in_frames - 1);
        let l = input[idx * 2] + (input[next * 2] - input[idx * 2]) * frac;
        let r = input[idx * 2 + 1] + (input[next * 2 + 1] - input[idx * 2 + 1]) * frac;
        out.push(l);
        out.push(r);
    }
    out
}

/// Full source-frame → ring-bytes pipeline (without the per-route DSP):
/// stereo-ize, resample to the ring rate if needed, convert to 16-bit LE bytes.
fn frame_to_ring_bytes(frame: &[f32], format: &AudioFormat) -> Vec<u8> {
    let stereo = to_stereo(frame, format.channels);
    let stereo = if format.sample_rate != RING_SAMPLE_RATE {
        resample_linear_stereo(&stereo, format.sample_rate, RING_SAMPLE_RATE)
    } else {
        stereo
    };
    let mut bytes = Vec::with_capacity(stereo.len() * 2);
    for &s in &stereo {
        bytes.extend_from_slice(&f32_to_i16(s).to_le_bytes());
    }
    bytes
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use std::{sync::atomic::Ordering, time::Duration};

    use tracing::{debug, warn};

    use crate::audio::ring_layout::{
        RingHeader, MIC_SECTION_NAME, RING_BYTES, RING_MAGIC, RING_VERSION,
    };
    use windows::{
        core::PCWSTR,
        Win32::System::Memory::{
            MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE,
        },
    };

    /// A writable live view into the driver's virtual-mic ring buffer.
    struct MicRingView {
        ptr: *mut RingHeader,
        _handle: windows::Win32::Foundation::HANDLE,
    }

    // SAFETY: the view is used by a single writer thread; the only concurrent
    // access is the driver reading `data`/`write_bytes` and writing
    // `read_bytes`, which we touch exclusively through volatile ops.
    unsafe impl Send for MicRingView {}

    impl MicRingView {
        fn open() -> Result<Self, String> {
            let name: Vec<u16> = format!("{MIC_SECTION_NAME}\0").encode_utf16().collect();

            // SAFETY: standard Win32 section open/map; `name` is NUL-terminated
            // and outlives the call. Header validation below guards against
            // mapping a foreign/stale section.
            unsafe {
                let handle = OpenFileMappingW(
                    FILE_MAP_READ.0 | FILE_MAP_WRITE.0,
                    false,
                    PCWSTR(name.as_ptr()),
                )
                .map_err(|e| {
                    format!("{MIC_SECTION_NAME} section not found — is nodus_audio.sys loaded? {e}")
                })?;

                let view = MapViewOfFile(handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0);
                if view.Value.is_null() {
                    return Err("MapViewOfFile failed (write access denied?)".into());
                }

                let ring = &*(view.Value as *const RingHeader);
                if ring.magic != RING_MAGIC || ring.version != RING_VERSION {
                    return Err(format!(
                        "ring header mismatch (magic 0x{:08X}, version {}) — driver/app version skew",
                        ring.magic, ring.version
                    ));
                }
                if ring.sample_rate != RING_SAMPLE_RATE
                    || ring.channels != 2
                    || ring.bits_per_sample != 16
                    || ring.ring_bytes as usize != RING_BYTES
                {
                    return Err(format!(
                        "unexpected ring format: {} Hz, {} ch, {} bit, {} bytes",
                        ring.sample_rate, ring.channels, ring.bits_per_sample, ring.ring_bytes
                    ));
                }

                Ok(MicRingView { ptr: view.Value as *mut RingHeader, _handle: handle })
            }
        }

        /// Our monotonic producer counter (bytes ever written).
        #[inline]
        fn write_counter(&self) -> u64 {
            // SAFETY: counter shared with the driver — volatile read, as in
            // virtual_capture.rs.
            unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*self.ptr).write_bytes)) }
        }

        /// Driver's monotonic consumer counter (advances only while an app records).
        #[inline]
        fn read_counter(&self) -> u64 {
            // SAFETY: as above.
            unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*self.ptr).read_bytes)) }
        }

        /// Copy `bytes` into the ring starting at monotonic offset `start`
        /// (max two spans across the wrap). Does NOT publish the counter.
        fn write_pcm(&self, start: u64, bytes: &[u8]) {
            let mut src = 0usize;
            let mut dst = (start % RING_BYTES as u64) as usize;
            while src < bytes.len() {
                let span = (bytes.len() - src).min(RING_BYTES - dst);
                // SAFETY: dst+span <= RING_BYTES by construction; the data area
                // is plain bytes the driver only reads up to `write_bytes`,
                // which we publish after this copy completes.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr().add(src),
                        std::ptr::addr_of_mut!((*self.ptr).data).cast::<u8>().add(dst),
                        span,
                    );
                }
                src += span;
                dst = (dst + span) % RING_BYTES;
            }
        }

        /// Publish the new producer counter: Release fence (data first), then
        /// volatile store — the driver reads the counter with a barrier.
        fn publish(&self, counter: u64) {
            std::sync::atomic::fence(Ordering::Release);
            // SAFETY: volatile store of the shared counter, after the fence.
            unsafe {
                std::ptr::write_volatile(std::ptr::addr_of_mut!((*self.ptr).write_bytes), counter)
            };
        }
    }

    impl Drop for MicRingView {
        fn drop(&mut self) {
            // SAFETY: ptr came from MapViewOfFile and is unmapped exactly once.
            unsafe {
                let _ = UnmapViewOfFile(
                    windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                        Value: self.ptr as *mut _,
                    },
                );
            }
        }
    }

    /// "Renderer" whose output device is the Nodus virtual microphone.
    /// API mirrors `AudioRenderer` so the engine can treat both as a route sink.
    pub struct VirtualRender {
        /// Format of the SOURCE frames (rate/channels of what the receiver yields).
        format: AudioFormat,
        stop_flag: Arc<AtomicBool>,
    }

    impl VirtualRender {
        pub fn new(format: AudioFormat) -> Self {
            Self {
                format,
                stop_flag: Arc::new(AtomicBool::new(false)),
            }
        }

        /// Start the writer thread. Section open happens inside the thread:
        /// if the driver isn't present, one warning is logged and the thread
        /// exits — the route stays alive, it just feeds nothing (documented
        /// driverless behaviour, see module docs).
        pub fn start(
            &self,
            mut receiver: broadcast::Receiver<AudioFrame>,
            volume: Arc<AtomicU32>,
            muted: Arc<AtomicBool>,
            pan: Arc<AtomicU32>,
        ) {
            let format = self.format;
            let stop = Arc::clone(&self.stop_flag);
            stop.store(false, Ordering::SeqCst);

            std::thread::spawn(move || {
                // 1 ms scheduler resolution — thread::sleep(1) is otherwise
                // ~15.6 ms and the writer drains the channel in bursts.
                let _timer = crate::audio::session::TimerResolutionGuard::acquire();

                let view = match MicRingView::open() {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(
                            "VirtualRender: mic ring unavailable ({e}); \
                             route to the virtual mic will carry no audio"
                        );
                        return;
                    }
                };

                // Continue from the section's current counter — a previous
                // producer (another route or ring_tone) may have advanced it.
                let mut written: u64 = view.write_counter();
                let mut overrun_noted = false;

                debug!(
                    "VirtualRender: writing to {MIC_SECTION_NAME} from counter {written} \
                     (source {} Hz, {} ch)",
                    format.sample_rate, format.channels
                );

                while !stop.load(Ordering::SeqCst) {
                    match receiver.try_recv() {
                        Ok(mut frame) => {
                            let is_muted = muted.load(Ordering::Relaxed);
                            let vol = f32::from_bits(volume.load(Ordering::Relaxed));
                            let pan_v = f32::from_bits(pan.load(Ordering::Relaxed));

                            apply_route_dsp(&mut frame, format.channels, is_muted, vol, pan_v);
                            let bytes = frame_to_ring_bytes(&frame, &format);

                            // Lapping the driver's reader is expected whenever
                            // no app records from the mic — the driver resyncs
                            // to the live edge itself. Note it once, keep going.
                            if !overrun_noted
                                && written.saturating_sub(view.read_counter())
                                    > RING_BYTES as u64
                            {
                                debug!(
                                    "VirtualRender: writer is a full ring ahead of the \
                                     driver (no app recording) — continuing normally"
                                );
                                overrun_noted = true;
                            }

                            view.write_pcm(written, &bytes);
                            written += bytes.len() as u64;
                            view.publish(written);
                        }
                        Err(broadcast::error::TryRecvError::Empty) => {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(broadcast::error::TryRecvError::Lagged(n)) => {
                            warn!("VirtualRender: lagged {n} frames behind the capture — skipping");
                            continue;
                        }
                        Err(broadcast::error::TryRecvError::Closed) => break,
                    }
                }
                debug!("VirtualRender: stopped");
            });
        }

        pub fn stop(&self) {
            self.stop_flag.store(true, Ordering::SeqCst);
        }
    }
}

// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
pub mod platform {
    use super::*;

    /// Stub: the virtual-mic ring only exists on Windows.
    pub struct VirtualRender;

    impl VirtualRender {
        pub fn new(_format: AudioFormat) -> Self {
            Self
        }
        pub fn start(
            &self,
            _receiver: broadcast::Receiver<AudioFrame>,
            _volume: Arc<AtomicU32>,
            _muted: Arc<AtomicBool>,
            _pan: Arc<AtomicU32>,
        ) {
        }
        pub fn stop(&self) {}
    }
}

pub use platform::VirtualRender;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_i16_clamps_and_scales() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(1.0), 32_767);
        assert_eq!(f32_to_i16(-1.0), -32_767);
        // Out-of-range input clips instead of wrapping.
        assert_eq!(f32_to_i16(2.5), 32_767);
        assert_eq!(f32_to_i16(-2.5), -32_767);
        assert_eq!(f32_to_i16(0.5), 16_383);
    }

    #[test]
    fn mono_duplicates_into_stereo() {
        assert_eq!(to_stereo(&[0.1, -0.2], 1), vec![0.1, 0.1, -0.2, -0.2]);
    }

    #[test]
    fn stereo_passes_through() {
        let f = [0.1, 0.2, 0.3, 0.4];
        assert_eq!(to_stereo(&f, 2), f.to_vec());
    }

    #[test]
    fn multichannel_keeps_first_two() {
        // Two 4-channel frames → first two channels of each.
        let f = [0.1, 0.2, 0.8, 0.9, 0.3, 0.4, 0.8, 0.9];
        assert_eq!(to_stereo(&f, 4), vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn resampler_output_length_44100_to_48000() {
        // 441 input frames at 44.1 kHz → 441 * 48000 / 44100 = 480 frames.
        let input = vec![0.0f32; 441 * 2];
        let out = resample_linear_stereo(&input, 44_100, 48_000);
        assert_eq!(out.len(), 480 * 2);
    }

    #[test]
    fn resampler_identity_at_same_rate() {
        let input = vec![0.1f32, 0.2, 0.3, 0.4];
        assert_eq!(resample_linear_stereo(&input, 48_000, 48_000), input);
    }

    #[test]
    fn resampler_preserves_constant_signal() {
        let input = vec![0.5f32; 100 * 2];
        let out = resample_linear_stereo(&input, 44_100, 48_000);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn dsp_mute_zeroes_frame() {
        let mut f = vec![0.5f32, -0.5, 0.25, -0.25];
        apply_route_dsp(&mut f, 2, true, 1.0, 0.0);
        assert!(f.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn dsp_pan_right_attenuates_left() {
        // pan > 0 attenuates the LEFT channel (run_render semantics).
        let mut f = vec![1.0f32, 1.0];
        apply_route_dsp(&mut f, 2, false, 1.0, 0.5);
        assert!((f[0] - 0.5).abs() < 1e-6);
        assert!((f[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dsp_volume_scales_all_samples() {
        let mut f = vec![1.0f32, -1.0, 0.5];
        apply_route_dsp(&mut f, 1, false, 0.5, 0.0);
        assert_eq!(f, vec![0.5, -0.5, 0.25]);
    }

    #[test]
    fn frame_to_ring_bytes_mono_44k_to_stereo_48k_i16() {
        // 441 mono frames at 44.1 kHz of a constant 0.5 signal →
        // 480 stereo frames * 4 bytes, every sample 16383 LE.
        let format = AudioFormat { sample_rate: 44_100, channels: 1, bits_per_sample: 32 };
        let frame = vec![0.5f32; 441];
        let bytes = frame_to_ring_bytes(&frame, &format);
        assert_eq!(bytes.len(), 480 * 2 * 2);
        let first = i16::from_le_bytes([bytes[0], bytes[1]]);
        assert_eq!(first, 16_383);
    }
}
