import { useEffect, useState } from 'react';
import './FxAdvancedModal.css';
import { EQ_FREQS, type FxKind, type FxSpec } from '@/shared/bridge';
import type { NodeModel } from '@/features/nodes/types';

/**
 * FxAdvancedModal — the single "Advanced Setting" panel for an FX node (t28 Э3).
 * Replaces the old anchored FxPopover (which duplicated the on-node controls).
 * The node face keeps its primary gesture (knob/threshold/curve); this modal is
 * the focus mode holding every parameter with precise numeric entry, plus Bypass
 * and interaction config (Step / Range). Centered overlay, outside the zoomed
 * canvas world. Every change drives the engine live via onChange → set_fx_params.
 */

const KIND_LABEL: Record<FxKind, string> = {
  gain: 'Gain',
  gate: 'Noise Gate',
  eq: 'EQ',
  limiter: 'Limiter',
  compressor: 'Compressor',
};

/** Per-kind defaults — matches useScene.defaultFxSpec / FxNode fallbacks. */
const DEFAULTS: Record<FxKind, Partial<FxSpec>> = {
  gain: { gain_db: 0 },
  gate: { open_db: -45, close_db: -55 },
  eq: { eq_bands: [0, 0, 0, 0, 0] },
  limiter: { threshold_db: -12, ceiling_db: -1 },
  compressor: { threshold_db: -18, ratio: 4 },
};

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
const fmtDb = (v: number) => (v > 0 ? `+${v.toFixed(1)}` : v.toFixed(1));
const eqFreqLabel = (hz: number) => (hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`);

/** A single Main-section parameter, resolved against the current fx spec. */
interface MainParam {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  unit: string;
  bipolar?: boolean;
  set: (v: number) => void;
}

/** Build the Main parameter rows for a given kind. */
function mainParams(fx: FxSpec, patch: (p: Partial<FxSpec>) => void): MainParam[] {
  switch (fx.kind) {
    case 'gain':
      return [
        { id: 'gain', label: 'Gain', value: fx.gain_db ?? 0, min: -24, max: 24, unit: 'dB', bipolar: true, set: (v) => patch({ gain_db: v }) },
      ];
    case 'gate':
      // −100, not −80: matches the node's own dB scale and the engine's −100 dBFS
      // metering. Open must stay ≥ Close, so each one pushes the other rather than
      // letting the pair invert — the DSP swaps inverted thresholds defensively,
      // which would leave the real threshold somewhere the UI never showed.
      return [
        {
          id: 'open',
          label: 'Open',
          value: fx.open_db ?? -45,
          min: -100,
          max: 0,
          unit: 'dB',
          set: (v) => patch({ open_db: v, close_db: Math.min(fx.close_db ?? -55, v) }),
        },
        {
          id: 'close',
          label: 'Close',
          value: fx.close_db ?? -55,
          min: -100,
          max: 0,
          unit: 'dB',
          set: (v) => patch({ close_db: v, open_db: Math.max(fx.open_db ?? -45, v) }),
        },
      ];
    case 'limiter':
      return [
        { id: 'thr', label: 'Threshold', value: fx.threshold_db ?? -12, min: -40, max: 0, unit: 'dB', set: (v) => patch({ threshold_db: v }) },
        { id: 'ceil', label: 'Ceiling', value: fx.ceiling_db ?? -1, min: -40, max: 0, unit: 'dB', set: (v) => patch({ ceiling_db: v }) },
      ];
    case 'compressor':
      return [
        { id: 'thr', label: 'Threshold', value: fx.threshold_db ?? -18, min: -40, max: 0, unit: 'dB', set: (v) => patch({ threshold_db: v }) },
        { id: 'ratio', label: 'Ratio', value: fx.ratio ?? 4, min: 1, max: 20, unit: ':1', set: (v) => patch({ ratio: v }) },
      ];
    case 'eq': {
      const bands = fx.eq_bands && fx.eq_bands.length === 5 ? fx.eq_bands : [0, 0, 0, 0, 0];
      return EQ_FREQS.map((hz, i) => ({
        id: `b${i}`,
        label: eqFreqLabel(hz),
        value: bands[i] ?? 0,
        min: -12,
        max: 12,
        unit: 'dB',
        bipolar: true,
        set: (v: number) => {
          const next = bands.slice();
          next[i] = v;
          patch({ eq_bands: next });
        },
      }));
    }
  }
}

/** Slider with an accent fill (bipolar fills from centre). Native range = keyboard
 *  + drag for free; the fill rides the input background like the node VU sliders. */
function Slider({
  value,
  min,
  max,
  step,
  bipolar,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  bipolar?: boolean;
  onChange: (v: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  let fill: string;
  if (bipolar) {
    const zero = ((0 - min) / (max - min)) * 100;
    const a = Math.min(zero, pct);
    const b = Math.max(zero, pct);
    fill = `linear-gradient(90deg, var(--color-vu-track) 0 ${a}%, var(--color-type-fx) ${a}% ${b}%, var(--color-vu-track) ${b}% 100%)`;
  } else {
    fill = `linear-gradient(90deg, var(--color-type-fx) 0 ${pct}%, var(--color-vu-track) ${pct}% 100%)`;
  }
  return (
    <input
      className="fxadv-slider"
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(parseFloat(e.target.value))}
      style={{ background: `${fill} center / 100% 4px no-repeat` }}
    />
  );
}

/** Numeric stepper box: editable value + unit + up/down arrows (steps by `step`). */
function NumBox({
  value,
  min,
  max,
  step,
  unit,
  bipolar,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  bipolar?: boolean;
  onChange: (v: number) => void;
}) {
  const snap = (v: number) => clamp(Math.round(v / step) * step, min, max);
  const shown = unit === ':1' ? value.toFixed(1) : bipolar ? fmtDb(value) : value.toFixed(1);
  return (
    <span className="fxadv-num">
      <input
        className="fxadv-num-input"
        type="text"
        inputMode="decimal"
        value={shown}
        onChange={(e) => {
          const v = parseFloat(e.target.value.replace('+', ''));
          if (!Number.isNaN(v)) onChange(clamp(v, min, max));
        }}
        aria-label="value"
      />
      <span className="fxadv-num-u">{unit}</span>
      <span className="fxadv-num-arrows">
        <button type="button" aria-label="increase" onClick={() => onChange(snap(value + step))}>
          <svg viewBox="0 0 10 6" fill="currentColor" aria-hidden><path d="M5 0l5 6H0z" /></svg>
        </button>
        <button type="button" aria-label="decrease" onClick={() => onChange(snap(value - step))}>
          <svg viewBox="0 0 10 6" fill="currentColor" aria-hidden><path d="M5 6L0 0h10z" /></svg>
        </button>
      </span>
    </span>
  );
}

const STEP_OPTS = [1, 0.5, 0.1];

/** Full scale of the gain-reduction bar in the modal — dynamics that are working
 *  properly shave a few dB, so 20 keeps the bar readable. */
const GR_FULL_SCALE_DB = 20;

export function FxAdvancedModal({
  node,
  onChange,
  onClose,
  reductionDb = 0,
}: {
  node: NodeModel;
  onChange: (id: string, fx: FxSpec) => void;
  onClose: () => void;
  /** Live gain reduction from the engine, dB (0 = idle). */
  reductionDb?: number;
}) {
  const fx = node.fx;
  // Step / Range are interaction config (UI-only, not contract fields) — local.
  const bipolar = fx?.kind === 'gain' || fx?.kind === 'eq';
  const [step, setStep] = useState(fx?.kind === 'gate' ? 1 : 0.5);
  const [range, setRange] = useState(fx?.kind === 'eq' ? 12 : 24);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  if (!fx) return null;
  const patch = (p: Partial<FxSpec>) => onChange(node.id, { ...fx, ...p });
  const params = mainParams(fx, patch);
  // Bipolar params honour the Range control; others keep their natural bounds.
  const boundOf = (p: MainParam) => (p.bipolar ? { min: -range, max: range } : { min: p.min, max: p.max });

  const reset = () => {
    onChange(node.id, { kind: fx.kind, bypassed: false, ...DEFAULTS[fx.kind] });
  };

  return (
    <div className="fxadv-overlay" onMouseDown={onClose}>
      <div
        className="fxadv"
        role="dialog"
        aria-label={`${KIND_LABEL[fx.kind]} settings`}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="fxadv-head">
          <span className="fxadv-ico">
            <FxGlyph kind={fx.kind} />
          </span>
          <div className="fxadv-titles">
            <div className="fxadv-title">{node.name}</div>
            <div className="fxadv-sub">{node.subtitle || `${KIND_LABEL[fx.kind]} · effect`}</div>
          </div>
          <button className="fxadv-x" aria-label="close" onClick={onClose}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden>
              <path d="M5 5l14 14M19 5L5 19" />
            </svg>
          </button>
        </header>
        <hr className="fxadv-hr" />

        <section className="fxadv-sect">
          <div className="fxadv-sect-lbl">{fx.kind === 'eq' ? 'Main · bands' : 'Main'}</div>
          {params.map((p) => {
            const b = boundOf(p);
            return (
              <div className="fxadv-row" key={p.id}>
                <span className="fxadv-row-lbl">{p.label}</span>
                <Slider
                  value={p.value}
                  min={b.min}
                  max={b.max}
                  step={step}
                  bipolar={p.bipolar}
                  onChange={(v) => p.set(clamp(v, b.min, b.max))}
                />
                <NumBox
                  value={p.value}
                  min={b.min}
                  max={b.max}
                  step={p.unit === ':1' ? 0.5 : step}
                  unit={p.unit}
                  bipolar={p.bipolar}
                  onChange={(v) => p.set(clamp(v, b.min, b.max))}
                />
              </div>
            );
          })}
        </section>
        <hr className="fxadv-hr" />

        <section className="fxadv-sect">
          <div className="fxadv-sect-lbl">Advanced</div>
          {/* Gain reduction lives here rather than on the node's face: the
              compressor's face is a time-domain curve with nowhere sensible to put
              a meter, and this is the one place you go when you want to know how
              hard it is actually working. Read-only — it is the engine reporting. */}
          {(fx.kind === 'compressor' || fx.kind === 'limiter') && (
            <div className="fxadv-gr-row">
              <span className="fxadv-gr-txt">Gain reduction</span>
              <span className="fxadv-gr-bar">
                <i style={{ width: `${clamp(reductionDb / GR_FULL_SCALE_DB, 0, 1) * 100}%` }} />
              </span>
              <span className="fxadv-gr-val">
                {reductionDb >= 0.1 ? `−${reductionDb.toFixed(1)} dB` : 'idle'}
              </span>
            </div>
          )}
          <div className="fxadv-tgl-row">
            <span className="fxadv-tgl-txt">Bypass effect</span>
            <button
              className={`fxadv-tgl${fx.bypassed ? ' is-on' : ''}`}
              role="switch"
              aria-checked={!!fx.bypassed}
              aria-label="bypass effect"
              onClick={() => patch({ bypassed: !fx.bypassed })}
            />
          </div>
          <div className="fxadv-seg-row">
            <span className="fxadv-seg-txt">Step</span>
            <span className="fxadv-seg">
              {STEP_OPTS.map((s) => (
                <button key={s} className={step === s ? 'is-on' : ''} onClick={() => setStep(s)}>
                  {s}
                </button>
              ))}
            </span>
          </div>
          {bipolar && (
            <div className="fxadv-seg-row">
              <span className="fxadv-seg-txt">Range</span>
              <span className="fxadv-seg">
                {(fx.kind === 'eq' ? [12, 24] : [12, 24, 48]).map((r) => (
                  <button key={r} className={range === r ? 'is-on' : ''} onClick={() => setRange(r)}>
                    ±{r}
                  </button>
                ))}
              </span>
            </div>
          )}
        </section>

        <footer className="fxadv-foot">
          <button className="fxadv-reset" onClick={reset}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
              <path d="M3 12a9 9 0 1 0 3-6.7M3 4v4h4" />
            </svg>
            {fx.kind === 'eq' ? 'Flatten all bands' : 'Reset to defaults'}
          </button>
          <span className="fxadv-foot-hint">{fx.bypassed ? 'bypassed' : `${params.length} param${params.length > 1 ? 's' : ''}`}</span>
        </footer>
      </div>
    </div>
  );
}

/** Per-kind glyph for the header chip (orange, matching the node icons). */
function FxGlyph({ kind }: { kind: FxKind }) {
  const P = { fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const };
  switch (kind) {
    case 'gain':
      return <svg viewBox="0 0 24 24" {...P}><path d="M4 14v-2M9 18V6M15 21V3M20 14v-2" /></svg>;
    case 'gate':
      return <svg viewBox="0 0 24 24" {...P}><path d="M3 12h5l2-6 4 12 2-6h5" /></svg>;
    case 'eq':
      return (
        <svg viewBox="0 0 24 24" {...P}>
          <path d="M5 20v-9M5 8V4M12 20v-6M12 11V4M19 20v-3M19 14V4" />
          <circle cx="5" cy="9.5" r="1.5" fill="currentColor" stroke="none" />
          <circle cx="12" cy="12.5" r="1.5" fill="currentColor" stroke="none" />
          <circle cx="19" cy="15.5" r="1.5" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'limiter':
      return <svg viewBox="0 0 24 24" {...P}><path d="M3 16c3 0 4-9 7-9s3 6 5 6 3-3 6-3M3 20h18" /></svg>;
    case 'compressor':
      return <svg viewBox="0 0 24 24" {...P}><path d="M3 17c4 0 4-8 8-8s4 6 8 6M3 21h18" /></svg>;
  }
}
