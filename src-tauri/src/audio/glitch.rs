//! Audio-path health accounting (t30, reworked in t35).
//!
//! The "crackling under high system load" bug is intermittent and load-dependent,
//! so it has to be *measured*, not guessed at — otherwise every fix looks like it
//! helped. What counts as real audible damage:
//!
//!   * **dropout** — while a stream was flowing, the device did not get audio from
//!     us for some stretch of time and played silence instead. Measured in ms by
//!     [`DropoutMeter`]. That is the click/crackle itself.
//!   * **lagged frames** — the capture→render broadcast overflowed and dropped
//!     buffers the renderer never saw. Audio that was captured and never played.
//!
//! The first version counted an "underrun" whenever the device buffer was found
//! empty at a write. On healthy streams that fired ~12 times a second — an empty
//! buffer at that instant is almost always refilled before the audio engine reads
//! it — so it counted near-misses, flooded the log and could not tell a crackle
//! from normal operation (t35). Dropouts replace it.
//!
//! Counters are process-wide and monotonic; [`reset`] re-baselines before a run.
//! Everything is `Relaxed` — these are diagnostics, never control flow, and must
//! not cost the audio path anything measurable.

use std::sync::atomic::{AtomicU64, Ordering};

static LAG_EVENTS: AtomicU64 = AtomicU64::new(0);
static LAGGED_FRAMES: AtomicU64 = AtomicU64::new(0);
static DROPOUT_MS: AtomicU64 = AtomicU64::new(0);
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
    /// Total time, in ms, the device played silence while a stream was flowing —
    /// the audible dropouts. Only windows above the measurement-noise floor count.
    pub dropout_ms: u64,
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
        self.lag_events == 0 && self.dropout_ms == 0 && self.stale_dropped == 0
    }
}

/// The renderer fell behind and `frames` buffers were dropped by the broadcast.
pub fn record_lag(frames: u64) {
    LAG_EVENTS.fetch_add(1, Ordering::Relaxed);
    LAGGED_FRAMES.fetch_add(frames, Ordering::Relaxed);
}

/// `ms` of silence were played in place of flowing audio.
pub fn record_dropout_ms(ms: u64) {
    if ms > 0 {
        DROPOUT_MS.fetch_add(ms, Ordering::Relaxed);
    }
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
        dropout_ms: DROPOUT_MS.load(Ordering::Relaxed),
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
    DROPOUT_MS.store(0, Ordering::Relaxed);
    BUFFERS_RENDERED.store(0, Ordering::Relaxed);
    LATENCY_PEAK_MS.store(0, Ordering::Relaxed);
    STALE_DROPPED.store(0, Ordering::Relaxed);
    RESYNCS.store(0, Ordering::Relaxed);
}

// ── Dropout measurement ─────────────────────────────────────────────────────

/// How far back the deficit floor looks. Several device periods, so the floor is
/// found whatever phase the samples land on, yet short enough that a dropout shows
/// up in the window it happened in, not seconds later.
const FLOOR_WINDOW_S: f64 = 0.1;
/// Samples kept for the floor. Fixed-size ring — this runs on the render thread and
/// must never allocate. Sized for a sample every ~1 ms across the floor window.
const RING: usize = 128;

/// Measures how much audio a device did NOT get from a stream while it was flowing.
///
/// The idea: while audio flows, the device must take `elapsed_time × sample_rate`
/// frames. What it actually took is `frames_written − frames_still_buffered`. The
/// difference — the **deficit** — can only grow when the device played something
/// other than our audio: silence, because our buffer ran dry or the device itself
/// stalled (a Bluetooth link hiccup). Either way it is what the listener hears as a
/// dropout.
///
/// Three things make the raw deficit noisy, and all are handled here:
/// * the device consumes in whole periods (~10 ms), so between two gulps the deficit
///   climbs by up to a period and then drops back — a sawtooth, not a signal;
/// * the render thread can be preempted between reading the buffer and reading the
///   clock, which inflates single samples;
/// * **phase locking** (found on real hardware, t35): sampled only when buffers
///   arrive, on their own ~10 ms rhythm, every sample lands on nearly the same point
///   of a gulp while that point slowly drifts — the measured floor creeps up a whole
///   gulp and snaps back, which read as a steady trickle of 10–12 ms "dropouts".
///
/// The first two only push samples UP, so the meter tracks the **floor**: the minimum
/// deficit over the last [`FLOOR_WINDOW_S`]. Against the third it counts only how far
/// the floor rises past the **highest floor already seen** in the segment: a swing
/// inside one gulp is counted at most once, a real dropout lifts the floor for good.
/// The renderer also samples while it waits for a buffer, so samples no longer march
/// in step with arrivals at all.
///
/// Pauses between tracks are not dropouts: the renderer closes the segment with
/// [`DropoutMeter::pause`] when audio resumes after a long gap, and that segment is
/// settled as of the last buffer that arrived — samples taken while waiting saw the
/// device drain its cushion into the pause, and that is nobody's dropout.
#[derive(Debug, Clone)]
pub struct DropoutMeter {
    sample_rate: f64,
    /// Start of the current flowing segment: (time in s, frames consumed by then).
    segment: Option<(f64, u64)>,
    /// Recent (time, deficit) samples, newest at `head - 1`, in time order.
    ring: [(f64, f64); RING],
    len: usize,
    head: usize,
    /// Highest floor seen in this segment — only a rise past it is a dropout.
    high: f64,
    /// Time of the last sample taken right after a buffer arrived.
    last_arrival: Option<f64>,
    /// Dropout accumulated in the current report window, in frames.
    window_frames: f64,
}

impl DropoutMeter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: f64::from(sample_rate.max(1)),
            segment: None,
            ring: [(0.0, 0.0); RING],
            len: 0,
            head: 0,
            high: 0.0,
            last_arrival: None,
            window_frames: 0.0,
        }
    }

    /// Record one observation: `t` in seconds on a monotonic clock, `consumed` = frames
    /// the device has taken from this stream so far (written − buffered), `at_arrival`
    /// = taken right after writing a buffer that just arrived, rather than while waiting
    /// for one. Read the device buffer BEFORE the clock, so preemption errs toward more
    /// deficit, which the floor discards.
    pub fn sample(&mut self, t: f64, consumed: u64, at_arrival: bool) {
        let (t0, c0) = *self.segment.get_or_insert((t, consumed));
        let deficit = (t - t0) * self.sample_rate - consumed.saturating_sub(c0) as f64;
        self.ring[self.head] = (t, deficit);
        self.head = (self.head + 1) % RING;
        self.len = (self.len + 1).min(RING);
        if at_arrival {
            self.last_arrival = Some(t);
        }
    }

    /// The source went quiet (track ended, app paused): settle the segment as of the
    /// last arrival and close it, so the silence until audio resumes is not counted.
    pub fn pause(&mut self) {
        if let Some(floor) = self.last_arrival.and_then(|t| self.floor_until(t)) {
            self.account(floor);
        }
        self.segment = None;
        self.len = 0;
        self.high = 0.0;
        self.last_arrival = None;
    }

    /// Close the report window: dropout since the previous call, in ms.
    pub fn take_window_ms(&mut self) -> u64 {
        if let Some(floor) = self.newest_t().and_then(|t| self.floor_until(t)) {
            self.account(floor);
        }
        let ms = self.window_frames * 1000.0 / self.sample_rate;
        self.window_frames = 0.0;
        ms.round() as u64
    }

    /// Count only a rise past the highest floor seen — never a swing back up to it.
    fn account(&mut self, floor: f64) {
        if floor > self.high {
            self.window_frames += floor - self.high;
            self.high = floor;
        }
    }

    fn newest_t(&self) -> Option<f64> {
        (self.len > 0).then(|| self.ring[(self.head + RING - 1) % RING].0)
    }

    /// Minimum deficit over the [`FLOOR_WINDOW_S`] of samples ending at `until`.
    fn floor_until(&self, until: f64) -> Option<f64> {
        let mut floor: Option<f64> = None;
        for i in 0..self.len {
            let (t, deficit) = self.ring[(self.head + RING - 1 - i) % RING];
            if t > until {
                continue;
            }
            if until - t > FLOOR_WINDOW_S {
                break;
            }
            floor = Some(floor.map_or(deficit, |f| f.min(deficit)));
        }
        floor
    }
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
        record_dropout_ms(25);
        record_buffer();
        record_stale_dropped(4);
        record_resync();

        let s = snapshot();
        assert_eq!(s.lag_events, 2);
        assert_eq!(s.lagged_frames, 10);
        assert_eq!(s.dropout_ms, 25);
        assert_eq!(s.buffers_rendered, 1);
        assert_eq!(s.stale_dropped, 4);
        assert_eq!(s.resyncs, 1);

        reset();
        assert_eq!(snapshot(), AudioHealth::default());
    }

    #[test]
    fn clean_only_without_lag_dropout_or_shed() {
        let _g = serial();
        reset();
        // Rendering buffers alone is a healthy stream.
        record_buffer();
        assert!(snapshot().is_clean());

        record_dropout_ms(12);
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

    // ── DropoutMeter ────────────────────────────────────────────────────────

    const SR: u32 = 48_000;
    /// One device period at 48 kHz: 10 ms.
    const PERIOD: u64 = 480;
    /// What a healthy stream may read as, given the meter works off samples.
    const NOISE_MS: u64 = 3;

    /// A simulated device that takes one period at every 10 ms tick — except for
    /// ticks in `[stall.0, stall.1)`, where it takes nothing and never catches up
    /// (the missed audio was replaced by silence).
    fn consumed_at(t: f64, stall: Option<(u64, u64)>) -> u64 {
        let ticks = (t * 100.0 + 1e-9).floor() as u64;
        let missed = match stall {
            Some((a, b)) if ticks >= a => ticks.min(b - 1) + 1 - a,
            _ => 0,
        };
        (ticks - missed) * PERIOD
    }

    /// Feed `seconds` of samples every 7 ms (a spacing that walks through every
    /// phase of the device's 10 ms gulps), starting at sample index `*k`.
    fn run(m: &mut DropoutMeter, k: &mut u64, seconds: f64, stall: Option<(u64, u64)>) {
        let end = *k + (seconds / 0.007) as u64;
        while *k < end {
            let t = *k as f64 * 0.007;
            m.sample(t, consumed_at(t, stall), true);
            *k += 1;
        }
    }

    /// The failure this meter replaces: a perfectly healthy stream must not read
    /// as damage just because the device drains in gulps.
    #[test]
    fn healthy_stream_reads_as_no_dropout() {
        let mut m = DropoutMeter::new(SR);
        let mut k = 0;
        for window in 0..4 {
            run(&mut m, &mut k, 5.0, None);
            let ms = m.take_window_ms();
            assert!(ms <= NOISE_MS, "window {window}: healthy stream reported {ms} ms");
        }
    }

    /// A 50 ms stall must read as ~50 ms, in the window it happened — and must not
    /// be counted again in later windows.
    #[test]
    fn stall_is_measured_once_and_in_its_own_window() {
        let mut m = DropoutMeter::new(SR);
        let mut k = 0;
        let stall = Some((200, 205)); // ticks 2.00–2.05 s: 5 × 10 ms
        run(&mut m, &mut k, 5.0, stall);
        let first = m.take_window_ms();
        assert!((40..=60).contains(&first), "50 ms stall read as {first} ms");
        for window in 1..3 {
            run(&mut m, &mut k, 5.0, stall);
            let ms = m.take_window_ms();
            assert!(ms <= NOISE_MS, "window {window} re-counted the stall: {ms} ms");
        }
    }

    /// Silence between tracks is not a dropout: the source stops, the renderer
    /// closes the segment, and the gap until audio resumes costs nothing.
    #[test]
    fn a_pause_between_tracks_is_not_a_dropout() {
        let mut m = DropoutMeter::new(SR);
        let mut k = 0;
        let stall = Some((200, 300)); // the device idles for 1 s…
        run(&mut m, &mut k, 2.0, stall); // …because no audio arrives from 2.0 s
        m.pause(); // the renderer notices the gap when audio comes back at 3.0 s
        k = (3.0 / 0.007) as u64;
        run(&mut m, &mut k, 2.0, stall);
        let ms = m.take_window_ms();
        assert!(ms <= NOISE_MS, "a 1 s pause read as {ms} ms of dropout");
    }

    /// A preempted render thread reads the clock late, which inflates single
    /// samples. That is noise, not silence — the floor must ignore it.
    #[test]
    fn late_clock_reads_do_not_look_like_dropouts() {
        let mut m = DropoutMeter::new(SR);
        for k in 0..(5.0 / 0.007) as u64 {
            let t = k as f64 * 0.007;
            // Every 10th sample: buffer read at `t`, clock read 8 ms later.
            let clock = if k % 10 == 0 { t + 0.008 } else { t };
            m.sample(clock, consumed_at(t, None), true);
        }
        let ms = m.take_window_ms();
        assert!(ms <= NOISE_MS, "late clock reads were counted as {ms} ms");
    }

    /// The real-hardware failure of 15.09 (t35): the meter was sampled once per
    /// arriving buffer, buffers arrive on their own ~10 ms clock and the device drains
    /// on another, so every sample lands at almost the same point of a gulp — and that
    /// point drifts slowly. The measured floor then creeps up a whole gulp and snaps
    /// back. A healthy stream must not read as a steady trickle of dropouts from that.
    #[test]
    fn phase_locked_sampling_does_not_creep_into_dropouts() {
        const PERIOD_MS: u64 = 10;
        let mut m = DropoutMeter::new(SR);
        // One sample per buffer, 0.3% slower than the device period: the sampling
        // phase walks through a whole gulp every ~3.3 s, out of step with the window.
        let spacing = 0.010 * (1.0 + 1.0 / 330.0);
        let mut k = 0u64;
        for window in 0..6u64 {
            let end = ((window + 1) as f64 * 5.0 / spacing) as u64;
            while k < end {
                let t = k as f64 * spacing;
                m.sample(t, consumed_at(t, None), true);
                k += 1;
            }
            let ms = m.take_window_ms();
            // The first window may settle by up to one gulp; after that, nothing.
            let limit = if window == 0 { PERIOD_MS + NOISE_MS } else { NOISE_MS };
            assert!(ms <= limit, "window {window}: healthy phase-locked stream read {ms} ms");
        }
    }

    /// With the renderer also sampling while it waits (~every 1 ms), the floor is found
    /// exactly whatever rhythm buffers arrive on — clean from the very first window.
    #[test]
    fn dense_sampling_reads_a_healthy_stream_as_clean_from_the_first_window() {
        let mut m = DropoutMeter::new(SR);
        let arrival_every = 0.010 * (1.0 + 1.0 / 330.0);
        let mut next_arrival = 0.0;
        let mut idle_step = 0u64;
        for window in 0..4u64 {
            let end = (window + 1) as f64 * 5.0;
            loop {
                let t_idle = idle_step as f64 * 0.001;
                if t_idle >= end && next_arrival >= end {
                    break;
                }
                // Feed both kinds of sample in time order, as the render loop would.
                if next_arrival <= t_idle {
                    m.sample(next_arrival, consumed_at(next_arrival, None), true);
                    next_arrival += arrival_every;
                } else {
                    m.sample(t_idle, consumed_at(t_idle, None), false);
                    idle_step += 1;
                }
            }
            let ms = m.take_window_ms();
            assert!(ms <= NOISE_MS, "window {window}: healthy densely sampled stream read {ms} ms");
        }
    }

    /// When a track pauses, the renderer keeps sampling for a moment before it can tell
    /// it is a pause — and meanwhile the device drains its cushion. That drain is not a
    /// dropout: the segment is settled as of the last buffer that arrived.
    #[test]
    fn waiting_samples_that_run_into_a_pause_are_not_counted() {
        let mut m = DropoutMeter::new(SR);
        // Buffers every 10 ms until 2.0 s; then the device plays its cushion and has
        // nothing more until audio returns at 3.0 s.
        let drain = Some((205, 300));
        for t_ms in 0..=2200u64 {
            let t = t_ms as f64 * 0.001;
            m.sample(t, consumed_at(t, drain), t_ms % 10 == 0 && t_ms <= 2000);
        }
        m.pause(); // audio back at 3.0 s after a 1 s gap: that was a pause
        for t_ms in 3000..=5000u64 {
            let t = t_ms as f64 * 0.001;
            m.sample(t, consumed_at(t, drain), t_ms % 10 == 0);
        }
        let ms = m.take_window_ms();
        assert!(ms <= NOISE_MS, "draining into a pause read as {ms} ms of dropout");
    }
}
