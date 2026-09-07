//! DSP building blocks for real FX on routes (t18, Wave 1).
//!
//! Pure sample math — NO WASAPI, NO engine wiring yet. These are the primitives the
//! routing engine will apply per route once the FX contract lands (parameters flow
//! UI → routingGraph → engine; DSP is applied in the render chain). Keeping them
//! pure means they're unit-testable here without hardware.
//!
//! Sample format matches the engine: interleaved f32, one buffer per route. Where a
//! filter is stateful (biquad), one instance is needed PER CHANNEL — the engine will
//! own N instances for an N-channel route (see `Biquad` note).

/// Convert decibels to a linear amplitude multiplier (0 dB → 1.0, −6 dB → ~0.501).
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Centre frequencies of the 5-band graphic EQ. Mirrors `EQ_FREQS` in `bridge.ts` —
/// the UI draws its curve through these, so the two must not drift apart.
pub const EQ_BAND_FREQS: [f32; 5] = [60.0, 250.0, 1000.0, 4000.0, 16000.0];

/// Q of each graphic-EQ band. The bands sit two octaves apart, and a peaking
/// filter covering two octaves has Q = f0/BW = 1/(2 − 0.5) ≈ 0.67 — wide enough
/// that five bands blanket the spectrum without holes between them.
const EQ_BAND_Q: f32 = 0.67;

/// Gain: scale every sample by a linear multiplier. `gain` may exceed 1.0 (boost).
pub fn apply_gain(samples: &mut [f32], gain: f32) {
    if (gain - 1.0).abs() < f32::EPSILON {
        return; // unity — nothing to do
    }
    for s in samples.iter_mut() {
        *s *= gain;
    }
}

/// Gate timing. Opening must be quick or the front of every word is clipped;
/// closing is gentler so decaying tails are not chopped off.
const GATE_ATTACK_S: f32 = 0.002;
const GATE_RELEASE_S: f32 = 0.080;
/// How fast the detection envelope falls. Nothing to do with the gain ramp — it
/// only decides how long a signal is still considered "present".
const GATE_DETECT_RELEASE_S: f32 = 0.050;
/// A fully shut gate has infinite reduction; report it as this instead, matching
/// the range the node's meter is drawn against.
const GATE_MAX_REDUCTION_DB: f32 = 80.0;

/// A noise gate with hysteresis and smoothed gain: below `close_thresh` it fades
/// to silence, and it re-opens once the signal rises past `open_thresh` (which is
/// higher, so it cannot chatter at the boundary).
///
/// The open/close decision runs off a peak FOLLOWER, not the instantaneous frame
/// peak. Any waveform passes through zero every cycle, so deciding on the raw peak
/// would slam the gate shut mid-cycle on a perfectly loud signal.
pub struct NoiseGate {
    open_thresh: f32,
    close_thresh: f32,
    open: bool,
    attack: f32,
    release: f32,
    detect_release: f32,
    /// Detection envelope — rises instantly, falls slowly.
    detect: f32,
    /// Current gain applied, 1.0 = fully open.
    gain: f32,
    reduction_db: f32,
}

impl NoiseGate {
    /// `open_db`/`close_db` are thresholds in dBFS (e.g. −45 open, −55 close).
    /// `open_db` must be ≥ `close_db`; if not, they're swapped defensively.
    pub fn new(fs: f32, open_db: f32, close_db: f32) -> Self {
        let (o, c) = if open_db >= close_db { (open_db, close_db) } else { (close_db, open_db) };
        Self {
            open_thresh: db_to_linear(o),
            close_thresh: db_to_linear(c),
            open: true,
            attack: smoothing_coeff(fs, GATE_ATTACK_S),
            release: smoothing_coeff(fs, GATE_RELEASE_S),
            detect_release: smoothing_coeff(fs, GATE_DETECT_RELEASE_S),
            detect: 0.0,
            gain: 1.0,
            reduction_db: 0.0,
        }
    }

    /// Retune in place, keeping the envelope and the open/closed state.
    pub fn set_thresholds(&mut self, open_db: f32, close_db: f32) {
        let (o, c) = if open_db >= close_db { (open_db, close_db) } else { (close_db, open_db) };
        self.open_thresh = db_to_linear(o);
        self.close_thresh = db_to_linear(c);
    }

    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// The level the gate ACTUALLY decides on — its peak-follower, linear.
    ///
    /// The face must meter this and nothing else. Showing an RMS beside the same
    /// threshold made the UI disagree with the engine: music sits 10–15 dB below
    /// its own peaks, so the node drew a signal under the threshold and called
    /// itself closed while the gate was open and passing audio.
    pub fn detect_level(&self) -> f32 {
        self.detect
    }

    /// Whether the gate is currently passing — the engine's own state, not a guess.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Process one interleaved buffer in place. Returns whether the gate is open.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) -> bool {
        if samples.is_empty() {
            return self.open;
        }
        let ch = channels.max(1);
        let mut deepest = 1.0f32;
        for frame in samples.chunks_mut(ch) {
            let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            // Instant rise, slow fall — this is the signal's envelope, not its
            // zero crossings.
            self.detect = if peak > self.detect {
                peak
            } else {
                self.detect + (peak - self.detect) * self.detect_release
            };
            // Hysteresis: flip only past the far threshold for the current state.
            if self.open {
                if self.detect < self.close_thresh {
                    self.open = false;
                }
            } else if self.detect >= self.open_thresh {
                self.open = true;
            }
            let target = if self.open { 1.0 } else { 0.0 };
            let coeff = if target > self.gain { self.attack } else { self.release };
            self.gain += (target - self.gain) * coeff;
            for s in frame.iter_mut() {
                *s *= self.gain;
            }
            deepest = deepest.min(self.gain);
        }
        self.reduction_db = if deepest > 0.0 {
            (-20.0 * deepest.log10()).min(GATE_MAX_REDUCTION_DB)
        } else {
            GATE_MAX_REDUCTION_DB
        };
        self.open
    }
}

/// One-pole smoothing coefficient for a time constant at a given sample rate:
/// the fraction of the remaining distance an envelope closes each sample.
fn smoothing_coeff(fs: f32, seconds: f32) -> f32 {
    if seconds <= 0.0 {
        return 1.0; // instantaneous
    }
    1.0 - (-1.0 / (seconds * fs.max(1.0))).exp()
}

/// Below this much gain reduction a dynamics FX reports itself idle — a fraction
/// of a dB is not "working", it is arithmetic noise.
const ACTIVE_GR_DB: f32 = 0.2;

/// How fast the reporting follower falls; only affects meters, never the audio.
const REPORT_RELEASE_S: f32 = 0.050;

/// Linear amplitude → the 0..1 dBFS scale (−100..0 dB) every meter in the app uses.
fn dbfs_scaled(linear: f32) -> f32 {
    ((20.0 * linear.max(1e-7).log10() + 100.0) / 100.0).clamp(0.0, 1.0)
}

/// Limiter attack/release. Fixed for now — the Advanced modal does not expose them
/// yet, and inventing contract fields for a UI that does not exist is worse than a
/// well-chosen constant. Fast enough to catch transients without look-ahead, slow
/// enough on release that steady programme material does not pump.
const LIMITER_ATTACK_S: f32 = 0.001;
const LIMITER_RELEASE_S: f32 = 0.100;

/// A peak limiter: holds the signal down at `threshold`, with a brick-wall clamp
/// at `ceiling` as the guarantee of last resort.
///
/// Deliberately WITHOUT look-ahead. A look-ahead limiter catches transients by
/// delaying the signal, and that delay is precisely the latency fought elsewhere
/// in this engine (t32) — so instead the attack is fast, and `ceiling` hard-clamps
/// whatever slips through the first samples of a transient.
pub struct Limiter {
    threshold: f32, // linear
    ceiling: f32,   // linear
    attack: f32,
    release: f32,
    /// Current gain applied, 1.0 = no reduction.
    gain: f32,
    /// Deepest reduction during the last processed buffer, in dB (≥ 0).
    reduction_db: f32,
}

impl Limiter {
    pub fn new(fs: f32, threshold_db: f32, ceiling_db: f32) -> Self {
        Self {
            threshold: db_to_linear(threshold_db),
            ceiling: db_to_linear(ceiling_db),
            attack: smoothing_coeff(fs, LIMITER_ATTACK_S),
            release: smoothing_coeff(fs, LIMITER_RELEASE_S),
            gain: 1.0,
            reduction_db: 0.0,
        }
    }

    /// Retune without dropping the envelope — same reason `Biquad::set_coeffs`
    /// exists: rebuilding a running dynamics processor jumps its gain and clicks.
    pub fn set_thresholds(&mut self, threshold_db: f32, ceiling_db: f32) {
        self.threshold = db_to_linear(threshold_db);
        self.ceiling = db_to_linear(ceiling_db);
    }

    /// How hard the limiter is working right now, in dB of reduction (0 = idle).
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Process one interleaved buffer in place.
    ///
    /// The gain is computed from the peak ACROSS the channels of each frame and
    /// applied to all of them equally — per-channel gain would let the stereo
    /// image wander sideways whenever one channel limits harder than the other.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        let ch = channels.max(1);
        let mut deepest = 1.0f32;
        for frame in samples.chunks_mut(ch) {
            let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let target = if peak > self.threshold {
                self.threshold / peak
            } else {
                1.0
            };
            // Attack when more reduction is needed, release when less.
            let coeff = if target < self.gain { self.attack } else { self.release };
            self.gain += (target - self.gain) * coeff;
            for s in frame.iter_mut() {
                *s = (*s * self.gain).clamp(-self.ceiling, self.ceiling);
            }
            deepest = deepest.min(self.gain);
        }
        self.reduction_db = if deepest < 1.0 { -20.0 * deepest.log10() } else { 0.0 };
    }
}

/// Compressor timing and knee. Slower than the limiter on purpose: a compressor
/// shapes dynamics musically rather than catching peaks, so a 10 ms attack lets
/// transients through before it clamps down.
const COMPRESSOR_ATTACK_S: f32 = 0.010;
const COMPRESSOR_RELEASE_S: f32 = 0.200;
/// Soft-knee width in dB, centred on the threshold — without it the gain curve has
/// a corner at the threshold and quiet passages audibly "grab".
const COMPRESSOR_KNEE_DB: f32 = 6.0;

/// A peak-sensing compressor with a soft knee: above `threshold` the level is
/// reduced by `ratio`, blended smoothly across the knee.
///
/// No makeup gain — that is what a Gain node downstream is for, and baking it in
/// here would silently change loudness when the user only touched the ratio.
pub struct Compressor {
    threshold_db: f32,
    /// Always ≥ 1.0; a ratio of 1 is the identity (no compression).
    ratio: f32,
    attack: f32,
    release: f32,
    gain: f32,
    reduction_db: f32,
}

impl Compressor {
    pub fn new(fs: f32, threshold_db: f32, ratio: f32) -> Self {
        Self {
            threshold_db,
            ratio: ratio.max(1.0),
            attack: smoothing_coeff(fs, COMPRESSOR_ATTACK_S),
            release: smoothing_coeff(fs, COMPRESSOR_RELEASE_S),
            gain: 1.0,
            reduction_db: 0.0,
        }
    }

    /// Retune in place, keeping the envelope (see `Limiter::set_thresholds`).
    pub fn set_params(&mut self, threshold_db: f32, ratio: f32) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
    }

    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// The static gain curve: what linear gain this peak level should receive.
    fn target_gain(&self, peak: f32) -> f32 {
        if peak <= 0.0 {
            return 1.0;
        }
        let level_db = 20.0 * peak.log10();
        let over = level_db - self.threshold_db;
        let half_knee = COMPRESSOR_KNEE_DB / 2.0;
        let out_db = if over <= -half_knee {
            level_db // below the knee — untouched
        } else if over >= half_knee {
            self.threshold_db + over / self.ratio // above — full ratio
        } else {
            // Inside the knee: quadratic interpolation, so the curve has no corner.
            let x = over + half_knee;
            level_db + (1.0 / self.ratio - 1.0) * x * x / (2.0 * COMPRESSOR_KNEE_DB)
        };
        db_to_linear(out_db - level_db)
    }

    /// Process one interleaved buffer in place. Gain comes from the peak across
    /// the frame's channels, for the same stereo-image reason as the limiter.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        let ch = channels.max(1);
        let mut deepest = 1.0f32;
        for frame in samples.chunks_mut(ch) {
            let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let target = self.target_gain(peak);
            let coeff = if target < self.gain { self.attack } else { self.release };
            self.gain += (target - self.gain) * coeff;
            for s in frame.iter_mut() {
                *s *= self.gain;
            }
            deepest = deepest.min(self.gain);
        }
        self.reduction_db = if deepest < 1.0 { -20.0 * deepest.log10() } else { 0.0 };
    }
}

/// A biquad filter (Direct Form I). One instance is MONO — process a single channel.
/// For stereo/N-channel the engine holds one `Biquad` per channel and de-interleaves.
/// Coefficients are normalized (a0 == 1). Default = passthrough (b0 = 1).
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
}

impl Biquad {
    /// Peaking EQ (RBJ cookbook): boost/cut `gain_db` around `f0` with sharpness `q`.
    /// `fs` = sample rate. At 0 dB the coefficients reduce to unity (passthrough).
    pub fn peaking(fs: f32, f0: f32, q: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * (f0 / fs);
        let (sin, cos) = (w0.sin(), w0.cos());
        let alpha = sin / (2.0 * q.max(1e-4));

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Adopt another filter's coefficients while KEEPING this one's history.
    /// Retuning a running filter must not clear x/y state: rebuilding it instead
    /// drops the tail mid-signal, which clicks on every drag of an EQ band.
    /// Constant-peak-gain bandpass (RBJ cookbook): unity at `f0`, falling away on
    /// both sides. Analysis only — this one never sits in the signal path, it just
    /// measures how much energy lives in its band.
    pub fn bandpass(fs: f32, f0: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * (f0 / fs);
        let (sin, cos) = (w0.sin(), w0.cos());
        let alpha = sin / (2.0 * q.max(1e-4));
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            ..Default::default()
        }
    }

    pub fn set_coeffs(&mut self, other: &Biquad) {
        self.b0 = other.b0;
        self.b1 = other.b1;
        self.b2 = other.b2;
        self.a1 = other.a1;
        self.a2 = other.a2;
    }

    /// Process one mono sample.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    /// Process a mono buffer in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            *s = self.process_sample(*s);
        }
    }
}

// ── Spectrum analyser behind the EQ curve (t18 wave 6b) ────────────────────

/// Bands in the spectrum drawn behind the EQ curve. Enough to read as a silhouette
/// at 220 px wide; more would be finer than the node can draw.
pub const SPECTRUM_BANDS: usize = 32;
const SPECTRUM_LO_HZ: f32 = 40.0;
const SPECTRUM_HI_HZ: f32 = 16_000.0;
/// Fast enough to show a transient, slow enough that the silhouette doesn't
/// strobe at the meter's refresh rate.
const SPECTRUM_ATTACK_S: f32 = 0.02;
const SPECTRUM_RELEASE_S: f32 = 0.20;

/// Log-spaced filter bank measuring the level in each band.
///
/// A filter bank rather than an FFT deliberately: the biquads already exist, it adds
/// no dependency and no windowing/overlap machinery, and the frequency resolution an
/// FFT would buy is precision this backdrop cannot display. If a real analyser is
/// ever needed, the inside of this type can be swapped without touching the contract
/// or the UI.
pub struct SpectrumAnalyzer {
    filters: Vec<Biquad>,
    env: [f32; SPECTRUM_BANDS],
    attack: f32,
    release: f32,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let ratio = SPECTRUM_HI_HZ / SPECTRUM_LO_HZ;
        let steps = (SPECTRUM_BANDS - 1) as f32;
        // Q comes from the spacing itself: each band spans exactly one step of the
        // log grid, so neighbours meet instead of overlapping into mush (too low) or
        // leaving holes the signal falls through (too high).
        let k = 2f32.powf(ratio.log2() / steps / 2.0);
        let q = 1.0 / (k - 1.0 / k);
        let nyquist_guard = sample_rate * 0.45;
        let filters = (0..SPECTRUM_BANDS)
            .map(|i| {
                let f0 = SPECTRUM_LO_HZ * ratio.powf(i as f32 / steps);
                Biquad::bandpass(sample_rate, f0.min(nyquist_guard), q)
            })
            .collect();
        Self {
            filters,
            env: [0.0; SPECTRUM_BANDS],
            attack: smoothing_coeff(sample_rate, SPECTRUM_ATTACK_S),
            release: smoothing_coeff(sample_rate, SPECTRUM_RELEASE_S),
        }
    }

    /// Measure one interleaved buffer. Runs on the channel average: the face draws a
    /// single silhouette, so running the whole bank per channel would cost double for
    /// a picture nobody could tell apart.
    pub fn process(&mut self, frame: &[f32], channels: usize) {
        let ch = channels.max(1);
        for block in frame.chunks(ch) {
            let mono = block.iter().sum::<f32>() / ch as f32;
            for (b, filter) in self.filters.iter_mut().enumerate() {
                let mag = filter.process_sample(mono).abs();
                let coeff = if mag > self.env[b] { self.attack } else { self.release };
                self.env[b] += (mag - self.env[b]) * coeff;
            }
        }
    }

    /// Per-band levels, dBFS-scaled to 0..1 exactly like every other meter here.
    pub fn levels(&self) -> [f32; SPECTRUM_BANDS] {
        std::array::from_fn(|i| dbfs_scaled(self.env[i]))
    }
}

// ── Live FX parameters + per-route processor (t18) ─────────────────────────
use crate::routing::node::{FxKind, FxSpec};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// Live, shared FX parameters for one FX node. The engine holds one `Arc<FxParams>`
/// per FX node; every renderer whose chain passes through that node clones the Arc.
/// `set_fx_params` stores new values and bumps `version`; renderers recompute
/// coefficients only when they see a new version (never per-sample). `kind` is fixed
/// at creation (it's the node's type — a type change rebuilds the graph).
pub struct FxParams {
    kind: FxKind,
    bypassed: AtomicBool,
    gain_db: AtomicU32,
    open_db: AtomicU32,
    close_db: AtomicU32,
    freq: AtomicU32,
    q: AtomicU32,
    /// 5-band graphic EQ gains (dB) at [`EQ_BAND_FREQS`]. These reach the DSP as of
    /// stage 4; before that the contract carried them but nothing read them, so the
    /// EQ node's face and its sound were simply not connected.
    eq_bands: [AtomicU32; 5],
    threshold_db: AtomicU32,
    ceiling_db: AtomicU32,
    ratio: AtomicU32,
    version: AtomicU32,
    /// OUTBOUND (DSP → UI): how much gain a dynamics FX is shaving off right now,
    /// in dB (0 = idle). Written by the render thread every buffer, read by the
    /// engine for the node's gain-reduction meter.
    ///
    /// Note the one imprecision: with splitter fan-out several renderers share one
    /// FX node and each writes its own figure, so the last writer wins. For a meter
    /// that is honest enough; a per-route breakdown would need per-route storage.
    gain_reduction_db: AtomicU32,
    /// OUTBOUND (DSP → UI): the level ARRIVING at this FX, dBFS-scaled to 0..1 the
    /// same way every other meter in the app is. Without it an FX node's face has no
    /// idea what it is being fed — it fell back to the node model's static `level`,
    /// which is always 0 — so "the effect does nothing" and "the signal never
    /// reaches its threshold" looked exactly alike.
    input_level: AtomicU32,
    /// OUTBOUND: is the effect engaged right now (gate open / dynamics reducing)?
    active: AtomicBool,
    /// OUTBOUND: the per-band level ARRIVING at this FX, dBFS-scaled 0..1 — the
    /// backdrop behind the EQ curve. Measured pre-EQ on purpose: the curve already
    /// draws what the node is doing, so showing the input lets you read cause and
    /// effect in one glance. Only ever written by an EQ.
    spectrum: [AtomicU32; SPECTRUM_BANDS],
}

/// One FX node's live telemetry, as published to the UI.
///
/// This is the whole truth the face is allowed to draw. Anything the node renders
/// about its own state must come from here, never from a parallel calculation on
/// the UI side — that is precisely how the gate ended up drawing "closed" while
/// the engine had it open.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FxLevel {
    /// Gain reduction applied right now, in dB (0 = idle).
    pub reduction_db: f32,
    /// The level the effect DECIDES ON, dBFS-scaled to 0..1 — so comparing it
    /// against the threshold on screen gives the same verdict the DSP reached.
    pub input_level: f32,
    /// Is the effect engaged right now? Gate: open (passing). Limiter/Compressor:
    /// actively reducing gain.
    pub active: bool,
    /// Input spectrum for the EQ backdrop: one quantised level per band, 0..255.
    /// Empty for every other kind, so nothing else pays for it. Quantised to u8
    /// because 32 floats per node at meter refresh rate is a lot of JSON for a
    /// silhouette that is never read numerically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spectrum: Vec<u8>,
}

impl FxParams {
    pub fn new(spec: &FxSpec) -> Self {
        let p = Self {
            kind: spec.kind,
            bypassed: AtomicBool::new(spec.bypassed),
            gain_db: AtomicU32::new(spec.gain_db.to_bits()),
            open_db: AtomicU32::new(spec.open_db.to_bits()),
            close_db: AtomicU32::new(spec.close_db.to_bits()),
            freq: AtomicU32::new(spec.freq.to_bits()),
            q: AtomicU32::new(spec.q.to_bits()),
            eq_bands: std::array::from_fn(|i| AtomicU32::new(spec.eq_bands[i].to_bits())),
            threshold_db: AtomicU32::new(spec.threshold_db.to_bits()),
            ceiling_db: AtomicU32::new(spec.ceiling_db.to_bits()),
            ratio: AtomicU32::new(spec.ratio.to_bits()),
            version: AtomicU32::new(1),
            gain_reduction_db: AtomicU32::new(0f32.to_bits()),
            input_level: AtomicU32::new(0f32.to_bits()),
            active: AtomicBool::new(false),
            spectrum: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
        };
        p
    }

    /// Everything this FX node currently reports to the UI.
    pub fn level(&self) -> FxLevel {
        FxLevel {
            reduction_db: Self::load_f32(&self.gain_reduction_db),
            input_level: Self::load_f32(&self.input_level),
            active: self.active.load(Ordering::Relaxed),
            // Only an EQ has a backdrop to draw; everything else ships an empty vec
            // rather than 32 zeroes nobody will render.
            spectrum: if matches!(self.kind, FxKind::Eq) {
                self.spectrum
                    .iter()
                    .map(|a| (Self::load_f32(a) * 255.0).clamp(0.0, 255.0) as u8)
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    fn set_spectrum(&self, bands: &[f32; SPECTRUM_BANDS]) {
        for (slot, v) in self.spectrum.iter().zip(bands.iter()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    /// Current gain reduction for this FX node's meter, in dB (0 = idle).
    pub fn gain_reduction_db(&self) -> f32 {
        Self::load_f32(&self.gain_reduction_db)
    }

    fn set_gain_reduction(&self, db: f32) {
        self.gain_reduction_db.store(db.to_bits(), Ordering::Relaxed);
    }

    fn set_input_level(&self, level: f32) {
        self.input_level.store(level.to_bits(), Ordering::Relaxed);
    }

    fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    /// Update live from a spec (kind is ignored — fixed at creation) and bump version.
    pub fn store(&self, spec: &FxSpec) {
        self.bypassed.store(spec.bypassed, Ordering::Relaxed);
        self.gain_db.store(spec.gain_db.to_bits(), Ordering::Relaxed);
        self.open_db.store(spec.open_db.to_bits(), Ordering::Relaxed);
        self.close_db.store(spec.close_db.to_bits(), Ordering::Relaxed);
        self.freq.store(spec.freq.to_bits(), Ordering::Relaxed);
        self.q.store(spec.q.to_bits(), Ordering::Relaxed);
        for (slot, v) in self.eq_bands.iter().zip(spec.eq_bands.iter()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.threshold_db.store(spec.threshold_db.to_bits(), Ordering::Relaxed);
        self.ceiling_db.store(spec.ceiling_db.to_bits(), Ordering::Relaxed);
        self.ratio.store(spec.ratio.to_bits(), Ordering::Relaxed);
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    fn load_f32(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Ordering::Relaxed))
    }
}

/// A stateful FX processor bound to one FX node's live params, owned by a renderer.
/// Recomputes coefficients when the params version changes; applies DSP per buffer.
pub struct FxProcessor {
    params: Arc<FxParams>,
    last_version: u32,
    sample_rate: f32,
    gain_lin: f32,
    gate: NoiseGate,
    /// Graphic EQ: one biquad per (band, channel), laid out `band * channels + ch`.
    /// All five bands always exist — a 0 dB peaking filter is mathematically unity,
    /// so keeping flat bands allocated costs almost nothing and, crucially, keeps
    /// the layout stable while a band is dragged through zero.
    eq: Vec<Biquad>,
    /// Channel count the `eq` layout was built for.
    eq_channels: usize,
    /// Present only on an EQ — the other kinds have no backdrop to feed, and the
    /// bank is ~1.5 KB of filter state that would otherwise sit in every processor.
    spectrum: Option<Box<SpectrumAnalyzer>>,
    limiter: Limiter,
    compressor: Compressor,
    /// Reporting peak-follower state (linear). Kept per processor rather than read
    /// back from the shared params, so fan-out routes don't fight over it.
    in_level: f32,
    report_release: f32,
}

impl FxProcessor {
    pub fn new(params: Arc<FxParams>, sample_rate: f32) -> Self {
        Self {
            params,
            last_version: 0, // forces a reload on the first buffer
            sample_rate,
            gain_lin: 1.0,
            gate: NoiseGate::new(sample_rate, -45.0, -55.0),
            eq: Vec::new(),
            eq_channels: 0,
            spectrum: None,
            limiter: Limiter::new(sample_rate, 0.0, 0.0),
            compressor: Compressor::new(sample_rate, 0.0, 1.0),
            in_level: 0.0,
            report_release: smoothing_coeff(sample_rate, REPORT_RELEASE_S),
        }
    }

    fn reload(&mut self, channels: usize) {
        match self.params.kind {
            FxKind::Gain => {
                self.gain_lin = db_to_linear(FxParams::load_f32(&self.params.gain_db));
            }
            FxKind::Gate => {
                // Retune, don't rebuild — rebuilding would reset the envelope and
                // slam the gate on every parameter drag.
                self.gate.set_thresholds(
                    FxParams::load_f32(&self.params.open_db),
                    FxParams::load_f32(&self.params.close_db),
                );
            }
            FxKind::Eq => {
                // Five peaking bands in series per channel, retuned in place so a
                // live drag never resets filter state (that would click).
                let ch = channels.max(1);
                if self.eq_channels != ch {
                    self.eq = vec![Biquad::default(); EQ_BAND_FREQS.len() * ch];
                    self.eq_channels = ch;
                }
                // Built once, on the first EQ buffer — never rebuilt on a parameter
                // change, or a drag would keep resetting the envelopes and the
                // backdrop would flicker in time with the user's mouse.
                if self.spectrum.is_none() {
                    self.spectrum = Some(Box::new(SpectrumAnalyzer::new(self.sample_rate)));
                }
                let nyquist_guard = self.sample_rate * 0.45;
                for (b, &f0) in EQ_BAND_FREQS.iter().enumerate() {
                    let gain_db = FxParams::load_f32(&self.params.eq_bands[b]);
                    // 16 kHz would fold over at low sample rates — keep it legal.
                    let proto =
                        Biquad::peaking(self.sample_rate, f0.min(nyquist_guard), EQ_BAND_Q, gain_db);
                    for c in 0..ch {
                        self.eq[b * ch + c].set_coeffs(&proto);
                    }
                }
            }
            FxKind::Limiter => {
                self.limiter.set_thresholds(
                    FxParams::load_f32(&self.params.threshold_db),
                    FxParams::load_f32(&self.params.ceiling_db),
                );
            }
            FxKind::Compressor => {
                self.compressor.set_params(
                    FxParams::load_f32(&self.params.threshold_db),
                    FxParams::load_f32(&self.params.ratio),
                );
            }
        }
    }

    /// Apply this FX to one interleaved buffer in place. No-op when bypassed.
    pub fn process(&mut self, frame: &mut [f32], channels: usize) {
        let v = self.params.version.load(Ordering::Relaxed);
        if v != self.last_version {
            self.reload(channels);
            self.last_version = v;
        }
        // A bypassed FX still meters its input, so the node doesn't go dark — but it
        // decides nothing, so it reports itself idle.
        if self.params.bypassed.load(Ordering::Relaxed) {
            let lvl = self.follow_peak(frame);
            self.params.set_input_level(dbfs_scaled(lvl));
            self.params.set_gain_reduction(0.0);
            self.params.set_active(false);
            return;
        }
        match self.params.kind {
            FxKind::Gain => {
                apply_gain(frame, self.gain_lin);
                let lvl = self.follow_peak(frame);
                self.params.set_input_level(dbfs_scaled(lvl));
                self.params.set_active((self.gain_lin - 1.0).abs() > f32::EPSILON);
            }
            FxKind::Gate => {
                self.gate.process(frame, channels);
                self.params.set_gain_reduction(self.gate.reduction_db());
                // Report the gate's OWN detector and OWN state. Metering anything
                // else here is what let the face say "closed" while the engine was
                // open and passing audio.
                self.params.set_input_level(dbfs_scaled(self.gate.detect_level()));
                self.params.set_active(self.gate.is_open());
            }
            FxKind::Eq => {
                let lvl = self.follow_peak(frame);
                self.params.set_input_level(dbfs_scaled(lvl));
                self.params.set_active(false); // an EQ is never "engaged" or not
                let ch = channels.max(1);
                if self.eq_channels != ch {
                    self.reload(channels); // channel count changed (format change)
                }
                // Measure BEFORE the bands run: the backdrop shows what arrived, the
                // curve on top shows what this node does to it.
                if let Some(an) = self.spectrum.as_mut() {
                    an.process(frame, ch);
                    self.params.set_spectrum(&an.levels());
                }
                // One pass over the buffer, each sample through all five bands —
                // cheaper on cache than five passes over the whole frame.
                for c in 0..ch {
                    let mut i = c;
                    while i < frame.len() {
                        let mut x = frame[i];
                        for b in 0..EQ_BAND_FREQS.len() {
                            x = self.eq[b * ch + c].process_sample(x);
                        }
                        frame[i] = x;
                        i += ch;
                    }
                }
            }
            FxKind::Limiter => {
                let lvl = self.follow_peak(frame); // pre-processing input
                self.limiter.process(frame, channels);
                let gr = self.limiter.reduction_db();
                self.params.set_gain_reduction(gr);
                self.params.set_input_level(dbfs_scaled(lvl));
                self.params.set_active(gr > ACTIVE_GR_DB);
            }
            FxKind::Compressor => {
                let lvl = self.follow_peak(frame);
                self.compressor.process(frame, channels);
                let gr = self.compressor.reduction_db();
                self.params.set_gain_reduction(gr);
                self.params.set_input_level(dbfs_scaled(lvl));
                self.params.set_active(gr > ACTIVE_GR_DB);
            }
        }
    }

    /// Instant-attack / slow-release peak follower, used to report the input of
    /// effects that have no detector of their own. Same shape as the gate's
    /// detector, so every face meters a comparable quantity — and, being a
    /// follower, it does not collapse to zero between notes the way a raw
    /// per-buffer peak does.
    fn follow_peak(&mut self, frame: &[f32]) -> f32 {
        if frame.is_empty() {
            return self.in_level;
        }
        let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.in_level = if peak > self.in_level {
            peak
        } else {
            self.in_level + (peak - self.in_level) * self.report_release
        };
        self.in_level
    }
}

/// Apply a whole FX chain (in signal order) to one buffer in place.
pub fn apply_fx_chain(chain: &mut [FxProcessor], frame: &mut [f32], channels: usize) {
    for p in chain.iter_mut() {
        p.process(frame, channels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn spec(kind: FxKind) -> FxSpec {
        FxSpec {
            kind,
            bypassed: false,
            gain_db: 0.0,
            open_db: -45.0,
            close_db: -55.0,
            freq: 1000.0,
            q: 1.0,
            eq_bands: [0.0; 5],
            threshold_db: 0.0,
            ceiling_db: 0.0,
            ratio: 0.0,
        }
    }

    /// Feed a pure tone and check the bank puts its energy where the tone actually
    /// is. A spectrum that merely *moves* looks convincing while being wrong, so the
    /// band index is asserted, not just "something lit up".
    #[test]
    fn spectrum_lands_a_tone_in_the_band_that_contains_it() {
        let sr = 48_000.0;
        let mut an = SpectrumAnalyzer::new(sr);
        let tone: Vec<f32> = (0..sr as usize / 2)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin() * 0.5)
            .collect();
        an.process(&tone, 1);
        let levels = an.levels();

        // Band centres are 40 Hz * 400^(i/31); 1 kHz sits at i = 31*ln(25)/ln(400).
        let expected = (31.0 * 25f32.ln() / 400f32.ln()).round() as usize;
        let peak = (0..SPECTRUM_BANDS)
            .max_by(|&a, &b| levels[a].total_cmp(&levels[b]))
            .expect("bands exist");
        assert!(
            peak.abs_diff(expected) <= 1,
            "1 kHz landed in band {peak}, expected ~{expected}: {levels:?}"
        );
        // And the far ends stay quiet — a bank that leaks everywhere would still
        // pass the peak check above.
        assert!(levels[0] < levels[peak] * 0.6, "40 Hz band should be near-silent");
        assert!(levels[SPECTRUM_BANDS - 1] < levels[peak] * 0.6, "16 kHz band too");
    }

    #[test]
    fn spectrum_reads_silence_as_silence() {
        let mut an = SpectrumAnalyzer::new(48_000.0);
        an.process(&vec![0.0f32; 4800], 2);
        assert!(an.levels().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn db_to_linear_known_points() {
        assert!(close(db_to_linear(0.0), 1.0, 1e-6));
        assert!(close(db_to_linear(-6.0), 0.501_187, 1e-4));
        assert!(close(db_to_linear(6.0), 1.995_262, 1e-4));
    }

    #[test]
    fn gain_scales_and_unity_noops() {
        let mut b = vec![0.5, -0.5, 0.25];
        apply_gain(&mut b, 1.0);
        assert_eq!(b, vec![0.5, -0.5, 0.25]); // unity untouched
        apply_gain(&mut b, 2.0);
        assert_eq!(b, vec![1.0, -1.0, 0.5]);
        apply_gain(&mut b, 0.0);
        assert_eq!(b, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn gate_closes_on_sustained_quiet_and_reopens_on_loud() {
        let mut gate = NoiseGate::new(48_000.0, -40.0, -50.0);
        // Loud: an open gate must not attenuate.
        let mut loud = vec![0.5f32; 480];
        assert!(gate.process(&mut loud, 1));
        assert!(peak(&loud) > 0.45);

        // Sustained near-silence (~−66 dB): the detector has to fall from 0.5 first
        // (~0.26 s), and only then does the gain start its 80 ms fade — hence two
        // seconds of signal and a tail measured well after both.
        let mut quiet = vec![0.0005f32; 96_000];
        assert!(!gate.process(&mut quiet, 1));
        let tail = peak(&quiet[48_000..]);
        assert!(tail < 1e-6, "a shut gate must reach silence, got {tail}");

        // Loud again → it re-opens.
        let mut again = vec![0.5f32; 4_800];
        assert!(gate.process(&mut again, 1));
        assert!(peak(&again[2_400..]) > 0.45, "gate must re-open on a loud signal");
    }

    #[test]
    fn gate_hysteresis_holds_between_thresholds() {
        let mut gate = NoiseGate::new(48_000.0, -40.0, -50.0); // open .01, close ~.00316
        let mut q = vec![0.00001f32; 48_000];
        assert!(!gate.process(&mut q, 1)); // forced shut
        // A level BETWEEN close and open must not re-open it.
        let mut mid = vec![db_to_linear(-45.0); 4_800];
        assert!(!gate.process(&mut mid, 1), "must stay shut between the thresholds");
    }

    /// Why the detector exists: a loud tone crosses zero every cycle, and deciding
    /// on the raw frame peak would slam the gate shut mid-cycle on a perfectly
    /// healthy signal.
    #[test]
    fn gate_stays_open_through_zero_crossings() {
        let mut gate = NoiseGate::new(48_000.0, -40.0, -50.0);
        let mut tone = sine_1k(4_800); // 0.2 amplitude ≈ −14 dB, well above open
        assert!(gate.process(&mut tone, 1));
        let reference = sine_1k(4_800);
        for (got, want) in tone[2_400..].iter().zip(reference[2_400..].iter()) {
            assert!(close(*got, *want, 1e-3), "gate chattered on a steady tone");
        }
    }

    #[test]
    fn compressor_leaves_signal_below_threshold_alone() {
        let mut comp = Compressor::new(48_000.0, -12.0, 4.0);
        let input = vec![0.05f32; 480]; // ≈ −26 dB, well under the knee
        let mut buf = input.clone();
        comp.process(&mut buf, 1);
        for (got, want) in buf.iter().zip(input.iter()) {
            assert!(close(*got, *want, 1e-6));
        }
        assert_eq!(comp.reduction_db(), 0.0);
    }

    #[test]
    fn compressor_applies_its_ratio_above_threshold() {
        // −6 dB in, threshold −18, ratio 4:1 → 12 dB over becomes 3 → −15 dB out.
        let mut comp = Compressor::new(48_000.0, -18.0, 4.0);
        let mut buf = vec![db_to_linear(-6.0); 48_000]; // 1 s ≫ the 10 ms attack
        comp.process(&mut buf, 1);
        let settled_db = 20.0 * peak(&buf[24_000..]).log10();
        assert!(
            (settled_db - (-15.0)).abs() < 1.0,
            "4:1 over a −18 dB threshold should settle near −15 dB, got {settled_db}"
        );
    }

    #[test]
    fn compressor_ratio_one_is_the_identity() {
        let mut comp = Compressor::new(48_000.0, -30.0, 1.0);
        let input = vec![0.6f32; 1_000];
        let mut buf = input.clone();
        comp.process(&mut buf, 1);
        for (got, want) in buf.iter().zip(input.iter()) {
            assert!(close(*got, *want, 1e-4), "1:1 must change nothing");
        }
    }

    /// The contract's serde default for `ratio` is 0.0 — that must not turn into a
    /// division by zero in the gain curve.
    #[test]
    fn compressor_guards_against_a_zero_ratio() {
        let mut comp = Compressor::new(48_000.0, -20.0, 0.0);
        let mut buf = vec![0.5f32; 480];
        comp.process(&mut buf, 1);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn fx_processor_compressor_uses_contract_and_reports_reduction() {
        let mut s = spec(FxKind::Compressor);
        s.threshold_db = -18.0;
        s.ratio = 4.0;
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);

        let amp = db_to_linear(-6.0);
        let mut buf = vec![amp; 48_000];
        fx.process(&mut buf, 1);

        assert!(peak(&buf) < amp, "a 4:1 compressor must pull down a −6 dB signal");
        assert!(
            params.gain_reduction_db() > 1.0,
            "gain reduction must reach the shared params for the meter"
        );
    }

    #[test]
    fn biquad_default_is_passthrough() {
        let mut bq = Biquad::default();
        let input = [0.1f32, -0.3, 0.7, -0.2, 0.0];
        for &x in &input {
            assert!(close(bq.process_sample(x), x, 1e-6));
        }
    }

    #[test]
    fn peaking_zero_gain_is_unity() {
        // 0 dB peaking → coefficients collapse to passthrough.
        let mut bq = Biquad::peaking(48_000.0, 1_000.0, 1.0, 0.0);
        let input = [0.2f32, -0.5, 0.9, -0.1, 0.33];
        for &x in &input {
            assert!(close(bq.process_sample(x), x, 1e-5));
        }
    }

    #[test]
    fn peaking_boosts_dc_gain_stays_finite() {
        // A boosting peaking filter must stay stable (finite) on a DC-ish input.
        let mut bq = Biquad::peaking(48_000.0, 1_000.0, 1.0, 12.0);
        let mut out = 0.0;
        for _ in 0..1000 {
            out = bq.process_sample(1.0);
        }
        assert!(out.is_finite());
    }

    #[test]
    fn fx_processor_gain_applies() {
        let mut s = spec(FxKind::Gain);
        s.gain_db = 6.0; // ~×1.995
        let mut fx = FxProcessor::new(Arc::new(FxParams::new(&s)), 48_000.0);
        let mut buf = vec![0.1, -0.2, 0.3];
        fx.process(&mut buf, 1);
        assert!(close(buf[0], 0.1 * 1.995_262, 1e-4));
    }

    #[test]
    fn fx_processor_bypass_is_noop() {
        let mut s = spec(FxKind::Gain);
        s.gain_db = 12.0;
        s.bypassed = true;
        let mut fx = FxProcessor::new(Arc::new(FxParams::new(&s)), 48_000.0);
        let mut buf = vec![0.1, -0.2];
        fx.process(&mut buf, 1);
        assert_eq!(buf, vec![0.1, -0.2]); // untouched
    }

    #[test]
    fn fx_processor_live_update_takes_effect() {
        let mut s = spec(FxKind::Gain);
        s.gain_db = 0.0; // unity
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);
        let mut buf = vec![0.5];
        fx.process(&mut buf, 1);
        assert!(close(buf[0], 0.5, 1e-6)); // unity → unchanged

        s.gain_db = 6.0;
        params.store(&s); // live bump
        let mut buf2 = vec![0.5];
        fx.process(&mut buf2, 1);
        assert!(close(buf2[0], 0.5 * 1.995_262, 1e-4)); // picked up new gain
    }

    /// Retuning a live filter must carry its history across, or every drag of an
    /// EQ band cuts the tail mid-signal and clicks.
    #[test]
    fn set_coeffs_keeps_filter_history() {
        let tuned = Biquad::peaking(48_000.0, 1_000.0, 0.67, 6.0);
        let mut live = Biquad::peaking(48_000.0, 1_000.0, 0.67, 3.0);
        for _ in 0..8 {
            live.process_sample(0.7); // give it something to ring with
        }
        let mut fresh = tuned; // identical coefficients, no history
        live.set_coeffs(&tuned);
        // Fed silence, a filter holding history keeps ringing; a fresh one is mute.
        assert!(fresh.process_sample(0.0).abs() < 1e-9);
        assert!(live.process_sample(0.0).abs() > 1e-6);
    }

    fn sine_1k(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 48_000.0).sin() * 0.2
            })
            .collect()
    }

    fn peak(b: &[f32]) -> f32 {
        b.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn eq_flat_bands_pass_through() {
        let s = spec(FxKind::Eq); // every band at 0 dB → unity
        let mut fx = FxProcessor::new(Arc::new(FxParams::new(&s)), 48_000.0);
        let input = vec![0.2f32, -0.5, 0.9, -0.1, 0.33, 0.0, 0.4, -0.4];
        let mut buf = input.clone();
        fx.process(&mut buf, 2);
        for (got, want) in buf.iter().zip(input.iter()) {
            assert!(close(*got, *want, 1e-4), "a flat EQ must not colour the signal");
        }
    }

    /// The stage-4 gap, pinned: `eq_bands` used to live in the contract but never
    /// reached the DSP, so the EQ node's face and its sound were disconnected.
    /// Also covers the live path — dragging a band must not need a re-wire.
    #[test]
    fn eq_bands_reach_the_dsp_and_update_live() {
        let mut s = spec(FxKind::Eq);
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);

        let mut flat = sine_1k(480);
        fx.process(&mut flat, 1);
        let flat_peak = peak(&flat);

        s.eq_bands[2] = 12.0; // +12 dB at the 1 kHz band
        params.store(&s); // live, no rewiring
        let mut boosted = sine_1k(480);
        fx.process(&mut boosted, 1);
        let boosted_peak = peak(&boosted);

        assert!(
            boosted_peak > flat_peak * 1.5,
            "+12 dB at 1 kHz must lift a 1 kHz tone (flat {flat_peak}, boosted {boosted_peak})"
        );
        assert!(boosted.iter().all(|s| s.is_finite()), "filter must stay stable");
    }

    #[test]
    fn limiter_holds_loud_signal_at_threshold() {
        let thr = db_to_linear(-6.0);
        let mut lim = Limiter::new(48_000.0, -6.0, 0.0);
        // 100 cycles of a 1 kHz tone at 0.9 — far above the threshold.
        let mut buf: Vec<f32> = (0..4800)
            .map(|i| (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 48_000.0).sin() * 0.9)
            .collect();
        lim.process(&mut buf, 1);
        // Judge the settled tail, not the attack ramp at the start.
        let tail = peak(&buf[3800..]);
        assert!(
            tail <= thr * 1.15 && tail >= thr * 0.85,
            "settled peak should sit at the threshold {thr}, got {tail}"
        );
    }

    #[test]
    fn limiter_leaves_quiet_signal_alone() {
        let mut lim = Limiter::new(48_000.0, -6.0, 0.0);
        let input = vec![0.2f32, -0.15, 0.05, -0.2, 0.1];
        let mut buf = input.clone();
        lim.process(&mut buf, 1);
        for (got, want) in buf.iter().zip(input.iter()) {
            assert!(close(*got, *want, 1e-6), "below threshold must pass untouched");
        }
        assert_eq!(lim.reduction_db(), 0.0, "an idle limiter reports no reduction");
    }

    /// The ceiling is the promise the limiter must never break — including during
    /// the first samples of a transient, where the attack has not caught up yet.
    #[test]
    fn limiter_never_exceeds_ceiling() {
        let ceiling = db_to_linear(-3.0);
        let mut lim = Limiter::new(48_000.0, -6.0, -3.0);
        let mut step = vec![0.99f32; 256]; // instant jump to near full scale
        lim.process(&mut step, 1);
        for s in &step {
            assert!(s.abs() <= ceiling + 1e-6, "{s} broke the ceiling {ceiling}");
        }
    }

    #[test]
    fn limiter_reports_gain_reduction() {
        let mut lim = Limiter::new(48_000.0, -12.0, 0.0);
        let mut loud = vec![0.9f32; 480];
        lim.process(&mut loud, 1);
        assert!(lim.reduction_db() > 1.0, "limiting hard must show up on the meter");
    }

    /// Wave 3 end to end: the limiter's contract fields reach the DSP and the
    /// reduction comes back out on the shared params for the node's GR meter.
    #[test]
    fn fx_processor_limiter_uses_contract_and_reports_reduction() {
        let mut s = spec(FxKind::Limiter);
        s.threshold_db = -12.0;
        s.ceiling_db = 0.0;
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);

        let mut buf = vec![0.9f32; 2400];
        fx.process(&mut buf, 1);

        assert!(peak(&buf) < 0.9, "a limiter set to −12 dB must pull 0.9 down");
        assert!(
            params.gain_reduction_db() > 1.0,
            "gain reduction must reach the shared params for the meter"
        );
    }

    /// The face draws `input_level` against the threshold and `active` as its
    /// badge, so those two may never disagree. A node reading "closed" while the
    /// engine was open and passing audio is exactly the bug this pins: the level
    /// reported must be the one the gate itself decides on.
    #[test]
    fn gate_reported_level_agrees_with_its_own_state() {
        let s = spec(FxKind::Gate); // open −45, close −55
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);
        let thr = (-45.0 + 100.0) / 100.0; // the mapping the face uses

        // A −14 dB tone: comfortably above the open threshold.
        let mut tone = sine_1k(4_800);
        fx.process(&mut tone, 1);
        let loud = params.level();
        assert!(loud.active, "the engine must have the gate open on a −14 dB tone");
        assert!(
            loud.input_level > thr,
            "and the reported level must read above the threshold too, got {}",
            loud.input_level
        );

        // Sustained −66 dB: below the close threshold.
        let mut quiet = vec![0.0005f32; 96_000];
        fx.process(&mut quiet, 1);
        let soft = params.level();
        assert!(!soft.active, "the engine must have shut the gate");
        assert!(
            soft.input_level < thr,
            "and the reported level must read below the threshold too, got {}",
            soft.input_level
        );
    }

    /// What the mute fix rests on. Mute now zeroes the buffer BEFORE the chain, so
    /// a muted route feeds the effects silence — and on silence the dynamics must
    /// actually let go: release the gain and report themselves idle. Otherwise a
    /// muted route would keep showing a working limiter, and unmuting would dump a
    /// still-ducked signal.
    #[test]
    fn limiter_releases_and_reports_idle_on_silence() {
        let mut s = spec(FxKind::Limiter);
        s.threshold_db = -12.0;
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);

        let mut loud = vec![0.9f32; 4_800];
        fx.process(&mut loud, 1);
        assert!(params.level().active, "limiter must engage on a loud signal");

        // Two buffers on purpose: the reduction figure is the DEEPEST point within a
        // buffer, and the first one still opens at the ducked gain it is releasing
        // from. The second shows the settled state.
        let mut silence = vec![0.0f32; 48_000];
        fx.process(&mut silence, 1);
        let mut settled = vec![0.0f32; 4_800];
        fx.process(&mut settled, 1);

        let lv = params.level();
        assert!(!lv.active, "on silence the limiter must report itself idle");
        assert!(
            lv.reduction_db < 0.5,
            "and stop reporting reduction, got {}",
            lv.reduction_db
        );
        assert!(silence.iter().all(|v| *v == 0.0), "silence in, silence out");
    }

    #[test]
    fn fx_processor_gate_silences_quiet() {
        let s = spec(FxKind::Gate); // open −45, close −55
        let params = Arc::new(FxParams::new(&s));
        let mut fx = FxProcessor::new(Arc::clone(&params), 48_000.0);
        let mut quiet = vec![0.0005f32; 96_000]; // ≈ −66 dB, below close
        fx.process(&mut quiet, 1);
        assert!(
            peak(&quiet[48_000..]) < 1e-6,
            "the gate must fade a quiet signal all the way to silence"
        );
        assert!(
            params.gain_reduction_db() > 20.0,
            "a shut gate must report deep reduction for its meter"
        );
    }
}
