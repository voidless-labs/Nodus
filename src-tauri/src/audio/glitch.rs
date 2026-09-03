//! Audio-path health accounting (t30).
//!
//! The "crackling under high system load" bug is intermittent and load-dependent,
//! so it has to be *measured*, not guessed at — otherwise every fix looks like it
//! helped. Two counters, both meaning real audible damage:
//!
//!   * **lagged frames** — the capture→render broadcast overflowed and dropped
//!     buffers the renderer never saw. Audio that was captured and never played.
//!   * **underruns** — the device buffer ran dry *while audio was still flowing*,
//!     so WASAPI padded the gap with silence. That is the click/crackle itself.
//!
//! Counters are process-wide and monotonic; [`reset`] re-baselines before a run.
//! Everything is `Relaxed` — these are diagnostics, never control flow, and must
//! not cost the audio path anything measurable.

use std::sync::atomic::{AtomicU64, Ordering};

static LAG_EVENTS: AtomicU64 = AtomicU64::new(0);
static LAGGED_FRAMES: AtomicU64 = AtomicU64::new(0);
static UNDERRUNS: AtomicU64 = AtomicU64::new(0);
static BUFFERS_RENDERED: AtomicU64 = AtomicU64::new(0);
static LATENCY_PEAK_MS: AtomicU64 = AtomicU64::new(0);
static STALE_DROPPED: AtomicU64 = AtomicU64::new(0);
static RESYNCS: AtomicU64 = AtomicU64::new(0);

/// A snapshot of the audio-path health counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AudioHealth {
    /// How many times the renderer fell behind the capture at all.
    pub lag_events: u64,
    /// Total buffers dropped by those lag events (the actual lost audio).
    pub lagged_frames: u64,
    /// Times the device buffer ran dry mid-stream (audible crackle).
    pub underruns: u64,
    /// Buffers successfully handed to WASAPI — the denominator for the rest.
    pub buffers_rendered: u64,
    /// Highest end-to-end latency estimate seen since the last reset, in ms:
    /// what is queued inside the device plus what still waits in the channel.
    /// This is the number that climbs into the seconds when the latency ratchet
    /// bites (t32) — a peak, so a single excursion cannot hide behind an average.
    pub latency_peak_ms: u64,
    /// Buffers deliberately discarded to get back to the live edge (t32). Losing
    /// a little stale audio is the price of not replaying seconds of it.
    pub stale_dropped: u64,
    /// Live pipeline resyncs performed by the engine (t32 phase 3 — stays 0 until
    /// that lands, but the field exists so the shape of this report is stable).
    pub resyncs: u64,
}

impl AudioHealth {
    /// Did anything audible go wrong at all?
    pub fn is_clean(&self) -> bool {
        self.lag_events == 0 && self.underruns == 0 && self.stale_dropped == 0
    }
}

/// The renderer fell behind and `frames` buffers were dropped by the broadcast.
pub fn record_lag(frames: u64) {
    LAG_EVENTS.fetch_add(1, Ordering::Relaxed);
    LAGGED_FRAMES.fetch_add(frames, Ordering::Relaxed);
}

/// The device buffer was empty while audio was still flowing.
pub fn record_underrun() {
    UNDERRUNS.fetch_add(1, Ordering::Relaxed);
}

/// One buffer was written to the device.
pub fn record_buffer() {
    BUFFERS_RENDERED.fetch_add(1, Ordering::Relaxed);
}

/// Report this route's current end-to-end latency estimate; only the peak is kept.
pub fn record_latency(ms: u64) {
    LATENCY_PEAK_MS.fetch_max(ms, Ordering::Relaxed);
}

/// `n` queued buffers were dropped on purpose to resume at the live edge (t32).
pub fn record_stale_dropped(n: u64) {
    if n > 0 {
        STALE_DROPPED.fetch_add(n, Ordering::Relaxed);
    }
}

/// The engine resynchronised a route's pipeline in place (t32 phase 3).
pub fn record_resync() {
    RESYNCS.fetch_add(1, Ordering::Relaxed);
}

/// Current counters.
pub fn snapshot() -> AudioHealth {
    AudioHealth {
        lag_events: LAG_EVENTS.load(Ordering::Relaxed),
        lagged_frames: LAGGED_FRAMES.load(Ordering::Relaxed),
        underruns: UNDERRUNS.load(Ordering::Relaxed),
        buffers_rendered: BUFFERS_RENDERED.load(Ordering::Relaxed),
        latency_peak_ms: LATENCY_PEAK_MS.load(Ordering::Relaxed),
        stale_dropped: STALE_DROPPED.load(Ordering::Relaxed),
        resyncs: RESYNCS.load(Ordering::Relaxed),
    }
}

/// Zero the counters — call before a measurement run to get a clean baseline.
pub fn reset() {
    LAG_EVENTS.store(0, Ordering::Relaxed);
    LAGGED_FRAMES.store(0, Ordering::Relaxed);
    UNDERRUNS.store(0, Ordering::Relaxed);
    BUFFERS_RENDERED.store(0, Ordering::Relaxed);
    LATENCY_PEAK_MS.store(0, Ordering::Relaxed);
    STALE_DROPPED.store(0, Ordering::Relaxed);
    RESYNCS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The counters are process-wide statics, so the tests that mutate them must
    // not interleave with each other.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn records_and_resets() {
        let _g = serial();
        reset();
        record_lag(7);
        record_lag(3);
        record_underrun();
        record_buffer();
        record_stale_dropped(4);
        record_resync();

        let s = snapshot();
        assert_eq!(s.lag_events, 2);
        assert_eq!(s.lagged_frames, 10);
        assert_eq!(s.underruns, 1);
        assert_eq!(s.buffers_rendered, 1);
        assert_eq!(s.stale_dropped, 4);
        assert_eq!(s.resyncs, 1);

        reset();
        assert_eq!(snapshot(), AudioHealth::default());
    }

    #[test]
    fn clean_only_without_lag_underrun_or_shed() {
        let _g = serial();
        reset();
        // Rendering buffers alone is a healthy stream.
        record_buffer();
        assert!(snapshot().is_clean());

        record_underrun();
        assert!(!snapshot().is_clean());

        // Shedding stale audio is audible too — it must not read as "clean".
        reset();
        record_stale_dropped(1);
        assert!(!snapshot().is_clean());
        reset();
    }

    /// Latency is a gauge, not a counter: the peak has to survive later, lower
    /// readings, or a momentary excursion into seconds would vanish from the report.
    #[test]
    fn latency_keeps_the_peak() {
        let _g = serial();
        reset();
        record_latency(40);
        record_latency(1800);
        record_latency(55);
        assert_eq!(snapshot().latency_peak_ms, 1800);
        reset();
        assert_eq!(snapshot().latency_peak_ms, 0);
    }

    /// A zero shed must not register — otherwise every ordinary lag would look
    /// like the engine had thrown audio away.
    #[test]
    fn zero_shed_is_not_recorded() {
        let _g = serial();
        reset();
        record_stale_dropped(0);
        assert_eq!(snapshot().stale_dropped, 0);
        assert!(snapshot().is_clean());
        reset();
    }
}
