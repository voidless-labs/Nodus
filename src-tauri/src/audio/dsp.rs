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

/// Gain: scale every sample by a linear multiplier. `gain` may exceed 1.0 (boost).
pub fn apply_gain(samples: &mut [f32], gain: f32) {
    if (gain - 1.0).abs() < f32::EPSILON {
        return; // unity — nothing to do
    }
    for s in samples.iter_mut() {
        *s *= gain;
    }
}

/// A simple noise gate with hysteresis: when the block's peak drops below
/// `close_thresh` the gate shuts (silences); it re-opens once the peak rises above
/// `open_thresh` (> close_thresh, so it doesn't chatter at the boundary).
///
/// Block-level for Wave 1 (decide once per buffer). Envelope/attack-release
/// smoothing is a later refinement; this already removes steady hiss/silence.
pub struct NoiseGate {
    open_thresh: f32,
    close_thresh: f32,
    open: bool,
}

impl NoiseGate {
    /// `open_db`/`close_db` are thresholds in dBFS (e.g. −45 open, −55 close).
    /// `open_db` must be ≥ `close_db`; if not, they're swapped defensively.
    pub fn new(open_db: f32, close_db: f32) -> Self {
        let (o, c) = if open_db >= close_db { (open_db, close_db) } else { (close_db, open_db) };
        Self { open_thresh: db_to_linear(o), close_thresh: db_to_linear(c), open: true }
    }

    /// Process one interleaved buffer in place. Returns whether the gate is open.
    pub fn process(&mut self, samples: &mut [f32]) -> bool {
        if samples.is_empty() {
            return self.open;
        }
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        // Hysteresis: flip only when crossing the far threshold for the current state.
        if self.open {
            if peak < self.close_thresh {
                self.open = false;
            }
        } else if peak >= self.open_thresh {
            self.open = true;
        }
        if !self.open {
            for s in samples.iter_mut() {
                *s = 0.0;
            }
        }
        self.open
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
    version: AtomicU32,
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
            version: AtomicU32::new(1),
        };
        p
    }

    /// Update live from a spec (kind is ignored — fixed at creation) and bump version.
    pub fn store(&self, spec: &FxSpec) {
        self.bypassed.store(spec.bypassed, Ordering::Relaxed);
        self.gain_db.store(spec.gain_db.to_bits(), Ordering::Relaxed);
        self.open_db.store(spec.open_db.to_bits(), Ordering::Relaxed);
        self.close_db.store(spec.close_db.to_bits(), Ordering::Relaxed);
        self.freq.store(spec.freq.to_bits(), Ordering::Relaxed);
        self.q.store(spec.q.to_bits(), Ordering::Relaxed);
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
    biquads: Vec<Biquad>, // one per channel (EQ)
}

impl FxProcessor {
    pub fn new(params: Arc<FxParams>, sample_rate: f32) -> Self {
        Self {
            params,
            last_version: 0, // forces a reload on the first buffer
            sample_rate,
            gain_lin: 1.0,
            gate: NoiseGate::new(-45.0, -55.0),
            biquads: Vec::new(),
        }
    }

    fn reload(&mut self, channels: usize) {
        match self.params.kind {
            FxKind::Gain => {
                self.gain_lin = db_to_linear(FxParams::load_f32(&self.params.gain_db));
            }
            FxKind::Gate => {
                self.gate = NoiseGate::new(
                    FxParams::load_f32(&self.params.open_db),
                    FxParams::load_f32(&self.params.close_db),
                );
            }
            FxKind::Eq => {
                let freq = FxParams::load_f32(&self.params.freq).clamp(20.0, self.sample_rate * 0.45);
                let q = FxParams::load_f32(&self.params.q).max(0.1);
                let gain_db = FxParams::load_f32(&self.params.gain_db);
                let proto = Biquad::peaking(self.sample_rate, freq, q, gain_db);
                self.biquads = vec![proto; channels.max(1)];
            }
            // UI-complete; DSP lands in the FX-functionality stage → pass through.
            FxKind::Limiter | FxKind::Compressor => {}
        }
    }

    /// Apply this FX to one interleaved buffer in place. No-op when bypassed.
    pub fn process(&mut self, frame: &mut [f32], channels: usize) {
        let v = self.params.version.load(Ordering::Relaxed);
        if v != self.last_version {
            self.reload(channels);
            self.last_version = v;
        }
        if self.params.bypassed.load(Ordering::Relaxed) {
            return;
        }
        match self.params.kind {
            FxKind::Gain => apply_gain(frame, self.gain_lin),
            FxKind::Gate => {
                self.gate.process(frame);
            }
            FxKind::Eq => {
                if self.biquads.len() < channels {
                    self.reload(channels); // channel count grew (format change)
                }
                for (c, bq) in self.biquads.iter_mut().enumerate().take(channels) {
                    let mut i = c;
                    while i < frame.len() {
                        frame[i] = bq.process_sample(frame[i]);
                        i += channels;
                    }
                }
            }
            // Pass-through until limiter/compressor DSP lands.
            FxKind::Limiter | FxKind::Compressor => {}
        }
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
    fn gate_silences_quiet_passes_loud() {
        let mut gate = NoiseGate::new(-40.0, -50.0);
        // Loud block (peak ~0.5 = −6 dB) → open, unchanged.
        let mut loud = vec![0.5, -0.4, 0.5];
        assert!(gate.process(&mut loud));
        assert_eq!(loud, vec![0.5, -0.4, 0.5]);
        // Quiet block (peak ~0.001 = −60 dB) < close → shuts, silenced.
        let mut quiet = vec![0.001, -0.001, 0.0005];
        assert!(!gate.process(&mut quiet));
        assert_eq!(quiet, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn gate_hysteresis_holds_between_thresholds() {
        let mut gate = NoiseGate::new(-40.0, -50.0); // open .01, close ~.00316
        // Force it closed with a very quiet block.
        let mut q = vec![0.0001];
        assert!(!gate.process(&mut q));
        // A mid-level block between close and open thresholds must NOT re-open.
        let peak_mid = db_to_linear(-45.0);
        let mut mid = vec![peak_mid];
        assert!(!gate.process(&mut mid)); // stays closed → silenced
        assert_eq!(mid, vec![0.0]);
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

    #[test]
    fn fx_processor_gate_silences_quiet() {
        let s = spec(FxKind::Gate); // open −45, close −55
        let mut fx = FxProcessor::new(Arc::new(FxParams::new(&s)), 48_000.0);
        let mut quiet = vec![0.0005, -0.0005]; // ~−66 dB < close → gate shuts
        fx.process(&mut quiet, 1);
        assert_eq!(quiet, vec![0.0, 0.0]);
    }
}
