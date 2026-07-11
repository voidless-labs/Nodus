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

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
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
}
