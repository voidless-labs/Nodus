import { useEffect, useRef, useState } from 'react';
import './FxNode.css';
import { KIND_COLOR_VAR, kindLabel, type LinkStatus, type NodeModel } from '@/features/nodes/types';
import type { FxSpec } from '@/shared/bridge';
import { NodeIcon } from '@/features/nodes/ui/NodeIcon';
import { NodeToolbar } from '@/features/nodes/ui/NodeToolbar';
import { EditableName } from '@/shared/ui/EditableName';

/**
 * FxNode — an effect node's face (t28 FX port). A dedicated component: FX nodes
 * have a bespoke body per effect (Gain = knob, Gate = meter, …) rather than the
 * generic meter+fader of NodeCard. The shell (label block, ports, glow, toolbar,
 * states) matches every other node; only the substrate body differs.
 *
 * Wave 1: Gain (a rotary knob driving fx.gain_db, −24…+24 dB). Other kinds fall
 * back to a placeholder until they're ported.
 */
const GAIN_MIN = -24;
const GAIN_MAX = 24;
const GAIN_SWEEP = 135; // pointer degrees at each extreme (270° total)
const KNOB_MAJ = [-135, -90, -45, 0, 45, 90, 135];
const KNOB_MIN = [-112.5, -67.5, -22.5, 22.5, 67.5, 112.5];

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Per-effect card footprint (from the design book). */
const FX_DIMS: Record<string, { w: number; h: number }> = {
  gain: { w: 143, h: 195 },
  gate: { w: 238, h: 147 },
  eq: { w: 238, h: 195 },
  limiter: { w: 238, h: 147 },
  compressor: { w: 238, h: 195 },
};
const fxDim = (kind?: string) => FX_DIMS[kind ?? ''] ?? FX_DIMS.gain;

// ── Noise Gate geometry (card 238×147, book frame 168:107) ──
const G_INX = 23;
const G_INW = 194; // inner VU-meter x / width
const G_INY = 78;
const G_INH = 20;
const G_CAP_Y = 75;
const G_CAP_H = 26;
// Threshold range. −100 matches everything around it: the node's own dB scale, the
// signal mapping below, and the engine's −100 dBFS metering (t29). It used to stop
// at −80 to match the old FxPopover inspector, which no longer exists.
const GATE_OPEN_MIN = -100;
const GATE_OPEN_MAX = 0;
/// The gate closes this far below where it opens. Dragging the face must carry the
/// close threshold along, or it ends up ABOVE the open one and the DSP has to swap
/// them defensively — leaving the real threshold somewhere the UI never showed.
const GATE_HYSTERESIS_DB = 10;
/// Full scale of the gain-reduction bar, matching the DSP's own clamp.
const GATE_MAX_REDUCTION_DB = 80;
// dB scale: 21 ticks across the inner VU width; labels at −100/−50/0.
const G_SCY = 110;
const G_LBY = 121;

/** The threshold marker (spec SVG, orange), rendered 6×20. */
function ThrMarker() {
  return (
    <svg width="6" height="20" viewBox="0 0 7 24" fill="none" aria-hidden>
      <path
        d="M5.04689 0H1.00096C0.293494 0 -0.190263 0.714527 0.0724823 1.37139L1.4524 4.82119C1.49965 4.93931 1.52393 5.06536 1.52393 5.19258V18.8074C1.52393 18.9346 1.49965 19.0607 1.4524 19.1788L0.072482 22.6286C-0.190264 23.2855 0.293494 24 1.00096 24H5.04689C5.75436 24 6.23811 23.2855 5.97537 22.6286L4.59545 19.1788C4.5482 19.0607 4.52393 18.9346 4.52393 18.8074V5.19258C4.52393 5.06536 4.5482 4.93931 4.59545 4.82119L5.97537 1.37139C6.23812 0.714526 5.75436 0 5.04689 0Z"
        fill="#FB923C"
      />
    </svg>
  );
}

/** dB scale (21 ticks + 3 end/mid labels) across the inner VU width. Shared by
 *  gate (−100/−50/0) and limiter (−40/−20/0). */
function FxDbScale({ labels }: { labels: [string, string, string] }) {
  const ticks = [];
  const N = 21;
  for (let i = 0; i < N; i++) {
    const x = G_INX + Math.round((i * (G_INW - 1)) / (N - 1));
    const h = i === 0 || i === 10 || i === 20 ? 8 : i === 5 || i === 15 ? 6 : 4;
    ticks.push(<div key={i} className="fx-gate-tick" style={{ left: x, top: G_SCY, height: h }} />);
  }
  const labs: [string, number][] = [
    [labels[0], G_INX],
    [labels[1], G_INX + Math.floor(G_INW / 2)],
    [labels[2], G_INX + G_INW - 1],
  ];
  return (
    <>
      {ticks}
      {labs.map(([t, x]) => (
        <div key={t} className="fx-gate-lab" style={{ left: x, top: G_LBY }}>
          {t}
        </div>
      ))}
    </>
  );
}

/** Noise Gate body: gain-reduction bar + capsule with the inner VU-meter (passive
 *  below threshold, green above) + draggable threshold marker + dB scale + badge. */
function GateBody({
  node,
  fx,
  onChange,
  reductionDb = 0,
  inputLevel,
  active,
}: {
  node: NodeModel;
  fx: FxSpec;
  onChange?: (id: string, fx: FxSpec) => void;
  reductionDb?: number;
  inputLevel?: number;
  active?: boolean;
}) {
  const vuRef = useRef<HTMLDivElement>(null);
  const openDb = fx.open_db ?? -45;
  const thr = clamp((openDb + 100) / 100, 0, 1);
  // Live input from the DSP when the engine is running; the node model's static
  // level only as a fallback (an FX node never appears in the device/exe level map).
  const sig = clamp(inputLevel ?? node.level ?? 0, 0, 1);
  // The badge is the ENGINE's state, not a guess from the level. Recomputing it
  // here is what made the node read "closed" while the gate was open and passing
  // audio. Only without telemetry do we fall back to comparing.
  const isOpen = active ?? sig >= thr;
  const thrX = G_INX + G_INW * thr;
  const capCy = G_CAP_Y + G_CAP_H / 2;

  // Drag the threshold marker horizontally → open_db. Cursor position over the VU
  // track (post-transform rect) makes it zoom-correct.
  const onThrDown = (e: React.MouseEvent) => {
    if (!onChange) return;
    e.preventDefault();
    e.stopPropagation();
    const move = (ev: MouseEvent) => {
      const r = vuRef.current?.getBoundingClientRect();
      if (!r) return;
      const t = clamp((ev.clientX - r.left) / r.width, 0, 1);
      const db = clamp(Math.round(t * 100 - 100), GATE_OPEN_MIN, GATE_OPEN_MAX);
      onChange(node.id, {
        ...fx,
        open_db: db,
        close_db: Math.max(GATE_OPEN_MIN, db - GATE_HYSTERESIS_DB),
      });
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  return (
    <>
      <div className={`fx-gate-badge${isOpen ? ' is-open' : ''}`}>
        <span className="fx-gate-dot" />
        {isOpen ? 'open' : 'closed'}
      </div>
      <div className="fx-gate-panel" />
      <div className="fx-gate-gr">
        <div
          className="fx-gate-gr-fill"
          style={{ width: `${clamp(reductionDb / GATE_MAX_REDUCTION_DB, 0, 1) * 100}%` }}
        />
      </div>
      <div className="fx-gate-cap" />
      <div className="fx-gate-vu" ref={vuRef}>
        {/* ONE bar, not two. The colour break sits at the threshold via a gradient
            anchored to the TRACK (background-size), so only `width` animates and
            the grey and green halves can never drift apart mid-transition — which
            is what two independently animated elements did. */}
        <div
          className="fx-gate-fill"
          style={{
            width: G_INW * sig,
            backgroundImage: `linear-gradient(90deg, #373737 0 ${thr * 100}%, #4ade80 ${thr * 100}% 100%)`,
            backgroundSize: `${G_INW}px 100%`,
          }}
        />
      </div>
      <div className="fx-gate-thr" style={{ left: thrX, top: capCy }} onMouseDown={onThrDown} title="drag to set threshold">
        <ThrMarker />
      </div>
      <FxDbScale labels={['−100', '−50', '0dB']} />
    </>
  );
}

/** Full-height limiter pin (flag + stem + round foot). Ceiling flips it vertically. */
function LimPin({ color }: { color: string }) {
  return (
    <svg width="7" height="24" viewBox="0 0 7 24" fill="none" aria-hidden>
      <path
        d="M4.76196 22.5V5.19258C4.76196 5.06536 4.78624 4.93931 4.83349 4.82119L6.48768 0.685695C6.61906 0.357263 6.37718 0 6.02345 0H0.500479C0.146747 0 -0.0951318 0.357263 0.0362411 0.685695L1.69044 4.82119C1.73769 4.93931 1.76196 5.06408 1.76196 5.1913V22.5026C1.76196 23.331 2.43354 24 3.26196 24C4.09039 24 4.76196 23.3284 4.76196 22.5Z"
        fill={color}
      />
    </svg>
  );
}

const LIM_MIN = -40;
const LIM_MAX = 0;
/// Full scale of the limiter's gain-reduction bar. A limiter that is working
/// properly shaves a few dB, so 20 keeps the meter readable — unlike the gate,
/// which slams the whole way down.
const LIM_MAX_REDUCTION_DB = 20;

/** Limiter body: 3-zone VU (green clean · amber semi-danger thr→ceiling · red
 *  danger >ceiling) + draggable Threshold (orange) & Ceiling (yellow) pins + badge. */
function LimiterBody({
  node,
  fx,
  onChange,
  reductionDb = 0,
  inputLevel,
  active,
}: {
  node: NodeModel;
  fx: FxSpec;
  reductionDb?: number;
  inputLevel?: number;
  active?: boolean;
  onChange?: (id: string, fx: FxSpec) => void;
}) {
  const vuRef = useRef<HTMLDivElement>(null);
  const thrDb = fx.threshold_db ?? -12;
  const ceilDb = fx.ceiling_db ?? -1;
  const thr = clamp((thrDb - LIM_MIN) / (LIM_MAX - LIM_MIN), 0, 1);
  const ceil = clamp((ceilDb - LIM_MIN) / (LIM_MAX - LIM_MIN), 0, 1);
  // The level is normalised on −100…0; remap onto the limiter's −40…0 meter.
  const raw = clamp(inputLevel ?? node.level ?? 0, 0, 1);
  const sig = clamp((raw * 100 - 60) / 40, 0, 1);
  // The engine's own verdict (is it actually reducing gain?), not a comparison
  // recomputed here — see the note in GateBody. Falls back only without telemetry.
  const limiting = active ?? sig > thr;
  const thrX = G_INX + G_INW * thr;
  const ceilX = G_INX + G_INW * ceil;
  const cy = G_CAP_Y + G_CAP_H / 2; // 88
  const off = 3;
  const gW = G_INW * Math.min(thr, sig);
  const sW = sig > thr ? G_INW * (Math.min(ceil, sig) - thr) : 0;
  const dW = sig > ceil ? G_INW * (sig - ceil) : 0;
  const fullW = gW + sW + dW;

  const dragPin = (which: 'thr' | 'ceil') => (e: React.MouseEvent) => {
    if (!onChange) return;
    e.preventDefault();
    e.stopPropagation();
    const move = (ev: MouseEvent) => {
      const r = vuRef.current?.getBoundingClientRect();
      if (!r) return;
      const t = clamp((ev.clientX - r.left) / r.width, 0, 1);
      const db = Math.round((LIM_MIN + t * (LIM_MAX - LIM_MIN)) * 2) / 2;
      if (which === 'thr') onChange(node.id, { ...fx, threshold_db: clamp(db, LIM_MIN, ceilDb) });
      else onChange(node.id, { ...fx, ceiling_db: clamp(db, thrDb, LIM_MAX) });
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  return (
    <>
      <div className="fx-gate-badge">
        <span
          className="fx-gate-dot"
          style={{
            background: limiting ? '#fb923c' : '#4ade80',
            boxShadow: `0 0 8px 0 ${limiting ? '#fb923c' : '#4ade80'}`,
          }}
        />
        {limiting ? 'limiting' : 'idle'}
      </div>
      <div className="fx-gate-panel" />
      <div className="fx-gate-gr">
        <div
          className="fx-gate-gr-fill"
          style={{ width: `${clamp(reductionDb / LIM_MAX_REDUCTION_DB, 0, 1) * 100}%` }}
        />
      </div>
      <div className="fx-gate-cap" />
      <div className="fx-gate-vu" ref={vuRef}>
        {!limiting ? (
          <div className="fx-lim-z fx-lim-green" style={{ left: 0, width: gW, borderRadius: 4 }} />
        ) : (
          <>
            <div className="fx-lim-z fx-lim-green" style={{ left: 0, width: gW, borderRadius: '4px 0 0 4px' }} />
            {sW > 0 && (
              <div className="fx-lim-z fx-lim-amber" style={{ left: gW, width: sW, borderRadius: dW > 0 ? 0 : '0 4px 4px 0' }} />
            )}
            {dW > 0 && (
              <div className="fx-lim-z fx-lim-red" style={{ left: gW + sW, width: dW, borderRadius: '0 4px 4px 0' }} />
            )}
          </>
        )}
        {fullW > 0 && <div className="fx-lim-shade" style={{ width: fullW }} />}
      </div>
      <div className="fx-lim-pin" style={{ left: thrX, top: cy + off }} onMouseDown={dragPin('thr')} title="threshold">
        <LimPin color="#fb923c" />
      </div>
      <div className="fx-lim-pin fx-lim-pin-ceil" style={{ left: ceilX, top: cy - off }} onMouseDown={dragPin('ceil')} title="ceiling">
        <LimPin color="#f5c542" />
      </div>
      <FxDbScale labels={['−40', '−20', '0dB']} />
    </>
  );
}

// ── 5-band graphic EQ geometry (card 238×195, container 220×115) ──
const EQ_CX = 9;
const EQ_CY = 59;
const EQ_CW = 220;
const EQ_CH = 115;
const EQ_BAND_X = [23, 66, 110, 154, 197]; // container-relative x per band (60/250/1k/4k/16k)
const EQ_LABELS = ['60', '250', '1k', '4k', '16k'];
const EQ_MAX_DB = 12; // ± range mapped onto the graph height
const EQ_CENTER_Y = EQ_CH / 2;
const EQ_AMPL = 48; // px at ±EQ_MAX_DB
const r1 = (v: number) => Math.round(v * 10) / 10;
const eqGainToY = (g: number) => EQ_CENTER_Y - (clamp(g, -EQ_MAX_DB, EQ_MAX_DB) / EQ_MAX_DB) * EQ_AMPL;

/** Static spectrum silhouette behind the curve (deterministic; live spectrum later). */
const EQ_SPECTRUM = (() => {
  let seed = 7;
  const rnd = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
  const bars: { x: number; y: number; w: number; h: number }[] = [];
  const BW = 4.1;
  const PITCH = 6.45;
  for (let x = 4; x + BW <= EQ_CW - 2; x += PITCH) {
    const t = x / EQ_CW;
    const env = 0.35 + 0.55 * Math.exp(-(((t - 0.26) / 0.2) ** 2)) + 0.28 * Math.exp(-(((t - 0.62) / 0.3) ** 2));
    const h = Math.max(0.12, Math.min(0.9, 0.22 + 0.72 * env * (0.6 + 0.4 * rnd()))) * EQ_CH;
    bars.push({ x: r1(x), y: r1(EQ_CH - h), w: BW, h: r1(h) });
  }
  return bars;
})();

/** Smooth Catmull-Rom path through the band points (flat shelves at the edges). */
function eqCurvePath(pts: [number, number][]): string {
  const full = [pts[0], ...pts, pts[pts.length - 1]];
  let d = `M ${r1(pts[0][0])},${r1(pts[0][1])}`;
  for (let i = 1; i < full.length - 2; i++) {
    const p0 = full[i - 1];
    const p1 = full[i];
    const p2 = full[i + 1];
    const p3 = full[i + 2];
    const c1x = p1[0] + (p2[0] - p0[0]) / 6;
    const c1y = p1[1] + (p2[1] - p0[1]) / 6;
    const c2x = p2[0] - (p3[0] - p1[0]) / 6;
    const c2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += ` C ${r1(c1x)},${r1(c1y)} ${r1(c2x)},${r1(c2y)} ${r1(p2[0])},${r1(p2[1])}`;
  }
  return d;
}

/** EQ body: spectrum + frequency-response curve through 5 draggable band points. */
function EqBody({
  node,
  fx,
  onChange,
}: {
  node: NodeModel;
  fx: FxSpec;
  onChange?: (id: string, fx: FxSpec) => void;
}) {
  const graphRef = useRef<HTMLDivElement>(null);
  const bands = fx.eq_bands && fx.eq_bands.length === 5 ? fx.eq_bands : [0, 0, 0, 0, 0];
  const ys = bands.map(eqGainToY);
  const pts: [number, number][] = [
    [0, ys[0]],
    ...EQ_BAND_X.map((x, i): [number, number] => [x, ys[i]]),
    [EQ_CW, ys[4]],
  ];
  const curve = eqCurvePath(pts);
  const fill = `${curve} L ${EQ_CW},${EQ_CH} L 0,${EQ_CH} Z`;

  const onDotDown = (i: number) => (e: React.MouseEvent) => {
    if (!onChange) return;
    e.preventDefault();
    e.stopPropagation();
    const move = (ev: MouseEvent) => {
      const r = graphRef.current?.getBoundingClientRect();
      if (!r) return;
      const yc = clamp((ev.clientY - r.top) / r.height, 0, 1) * EQ_CH;
      const g = clamp(Math.round(((EQ_CENTER_Y - yc) / EQ_AMPL) * EQ_MAX_DB * 2) / 2, -EQ_MAX_DB, EQ_MAX_DB);
      const next = bands.slice();
      next[i] = g;
      onChange(node.id, { ...fx, eq_bands: next });
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  return (
    <>
      <div className="eq-badge">
        <span className="eq-badge-k">bands</span>
        <span className="eq-badge-v">5</span>
      </div>
      <div className="eq-graph" ref={graphRef}>
        <svg viewBox={`0 0 ${EQ_CW} ${EQ_CH}`} width="100%" height="100%" preserveAspectRatio="none" fill="none">
          {EQ_SPECTRUM.map((b, i) => (
            <rect key={i} x={b.x} y={b.y} width={b.w} height={b.h} rx="1" fill="#24242a" />
          ))}
          <path d={fill} fill="#FB923C" opacity="0.12" />
          <path
            d={curve}
            stroke="#FB923C"
            strokeWidth="4"
            fill="none"
            strokeLinejoin="round"
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
        </svg>
      </div>
      {EQ_BAND_X.map((x, i) => (
        <span
          key={i}
          className="eq-dot"
          style={{ left: EQ_CX + x, top: EQ_CY + ys[i] }}
          onMouseDown={onDotDown(i)}
          title={`${EQ_LABELS[i]} Hz`}
        />
      ))}
      {EQ_LABELS.map((l, i) => (
        <div key={l} className="eq-lab" style={{ left: EQ_CX + EQ_BAND_X[i], top: 176 }}>
          {l}
        </div>
      ))}
    </>
  );
}

// ── Compressor (card 238×195, time-domain graph 220×115) ──
const C_CX = 9;
const C_CY = 59;
const C_CW = 220;
const C_CH = 115;
const C_TOP = 8; // headroom at the top of the drawing
const C_BOT = 115;
const C_N = 72;
const cGx = (t: number) => t * C_CW;
const cGy = (l: number) => C_BOT - l * (C_BOT - C_TOP);
/** 0..1 level on the −100…0 dBFS reference scale → dB. */
const lvlToDb = (lvl: number) => lvl * 100 - 100;
/** dB → position on the compressor's own −40…0 display range. */
const dbToFrac = (db: number) => clamp((db + 40) / 40, 0, 1);

/** Fallback envelope, drawn only when the engine is off and no telemetry exists —
 *  otherwise the node would be an empty box in preview / with the engine stopped. */
const COMP_ENV = (() => {
  const out: number[] = [];
  for (let i = 0; i < C_N; i++) {
    const t = i / (C_N - 1);
    let v = 0.3;
    for (const [c, a] of [
      [0.13, 0.3],
      [0.4, 0.33],
      [0.67, 0.31],
      [0.9, 0.24],
    ]) {
      v += a * Math.exp(-(((t - c) / 0.085) ** 2));
    }
    out.push(Math.min(1, v));
  }
  return out;
})();

/** Compressor body: input (grey) vs output (green, compressed toward threshold)
 *  over a 0–5 s time axis; a draggable dashed threshold line + ratio badge. */
function CompressorBody({
  node,
  fx,
  onChange,
  reductionDb = 0,
  inputLevel,
}: {
  node: NodeModel;
  fx: FxSpec;
  onChange?: (id: string, fx: FxSpec) => void;
  reductionDb?: number;
  inputLevel?: number;
}) {
  const graphRef = useRef<HTMLDivElement>(null);
  const thrDb = fx.threshold_db ?? -18;
  const ratio = fx.ratio ?? 4;
  const thrFrac = clamp((thrDb - LIM_MIN) / (LIM_MAX - LIM_MIN), 0, 1);

  // Rolling history of what actually went through, built from the telemetry the
  // engine already publishes (~15 fps × 72 points ≈ the 5 s the axis claims).
  // The output line is not a formula applied to the input — it is the input minus
  // the reduction the DSP really performed, so the graph cannot disagree with the
  // sound the way a computed curve would.
  const [hist, setHist] = useState<{ inp: number; out: number }[]>([]);
  useEffect(() => {
    if (inputLevel == null) return;
    const db = lvlToDb(inputLevel);
    const point = { inp: dbToFrac(db), out: dbToFrac(db - reductionDb) };
    setHist((h) => {
      const next = [...h, point];
      return next.length > C_N ? next.slice(next.length - C_N) : next;
    });
  }, [inputLevel, reductionDb]);

  const live = hist.length >= 2;
  const inSeries = live ? hist.map((p) => p.inp) : COMP_ENV;
  const outSeries = live
    ? hist.map((p) => p.out)
    : COMP_ENV.map((v) => (v <= thrFrac ? v : thrFrac + (v - thrFrac) / ratio));
  // Newest sample sits at the right edge; while history is still filling, the
  // left of the graph stays empty rather than the curve being stretched to fit.
  const n = inSeries.length;
  const xAt = (i: number) => cGx((C_N - n + i) / (C_N - 1));
  const pts = (s: number[]) => s.map((v, i) => `${r1(xAt(i))},${r1(cGy(v))}`).join(' ');
  const inpts = pts(inSeries);
  const outpts = pts(outSeries);
  const fillpts = `${r1(xAt(0))},${C_BOT} ${outpts} ${r1(xAt(n - 1))},${C_BOT}`;
  const ty = cGy(thrFrac) - 2; // raised 2px (matches the book)
  const thrCardY = Math.round(C_CY + ty);

  const onThrDown = (e: React.MouseEvent) => {
    if (!onChange) return;
    e.preventDefault();
    e.stopPropagation();
    const move = (ev: MouseEvent) => {
      const r = graphRef.current?.getBoundingClientRect();
      if (!r) return;
      const yv = clamp((ev.clientY - r.top) / r.height, 0, 1) * C_CH;
      const frac = clamp((C_BOT - yv) / (C_BOT - C_TOP), 0, 1);
      const db = clamp(Math.round((LIM_MIN + frac * (LIM_MAX - LIM_MIN)) * 2) / 2, LIM_MIN, LIM_MAX);
      onChange(node.id, { ...fx, threshold_db: db });
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  return (
    <>
      <div className="comp-badge">
        <span className="comp-badge-k">ratio</span>
        <span className="comp-badge-v">{ratio}:1</span>
      </div>
      <div className="comp-graph" ref={graphRef}>
        <svg viewBox={`0 0 ${C_CW} ${C_CH}`} width="100%" height="100%" preserveAspectRatio="none" fill="none">
          <polyline points={inpts} stroke="#6a6a72" strokeWidth="1.3" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
          <polygon points={fillpts} fill="#4ade80" opacity="0.12" />
          <polyline points={outpts} stroke="#4ade80" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
          <line x1="0" y1={r1(ty)} x2={C_CW} y2={r1(ty)} stroke="#fb923c" strokeWidth="2" strokeDasharray="7 6" vectorEffect="non-scaling-stroke" />
        </svg>
      </div>
      <span className="comp-thr-dot" style={{ left: C_CX, top: thrCardY }} />
      <span className="comp-thr-dot" style={{ left: C_CX + C_CW, top: thrCardY }} />
      <div className="comp-thr-hit" style={{ top: thrCardY - 5 }} onMouseDown={onThrDown} title="drag to set threshold" />
      {[0, 1, 2, 3, 4, 5].map((k) => (
        <div key={k} className="comp-lab" style={{ left: C_CX + Math.round((k / 5) * C_CW), top: 176 }}>
          {k}s
        </div>
      ))}
    </>
  );
}

export function FxNode({
  node,
  search,
  actions,
  status,
  chainState,
  soloSkipped,
  onSolo,
  onPin,
  pinned,
  onDuplicate,
  onDelete,
  onRename,
  onFxParams,
  onAdvanced,
  outConnected,
  inSourceColorVar,
  reductionDb = 0,
  inputLevel,
  active,
}: {
  node: NodeModel;
  search?: 'match' | 'dim';
  actions?: boolean;
  status?: LinkStatus;
  chainState?: 'on' | 'off';
  soloSkipped?: boolean;
  onSolo?: (id: string) => void;
  onPin?: (id: string) => void;
  pinned?: boolean;
  onDuplicate?: (id: string) => void;
  onDelete?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
  /** Live FX parameter change (persists + mirrors). */
  onFxParams?: (id: string, fx: FxSpec) => void;
  /** Open the FX Advanced Setting modal for this node (t28). */
  onAdvanced?: (id: string) => void;
  outConnected?: boolean;
  inSourceColorVar?: string;
  /** Live gain reduction this FX is applying, in dB (0 = idle) — drives the GR meter. */
  reductionDb?: number;
  /** Live level arriving at this FX (0..1). Undefined when the engine is stopped. */
  inputLevel?: number;
  /** The ENGINE's own state: gate open / dynamics reducing. Undefined when stopped. */
  active?: boolean;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);
  const colorVar = `var(${KIND_COLOR_VAR[node.kind]})`;

  const onGlowMove = (e: React.MouseEvent) => {
    if (rafRef.current != null) return;
    const card = cardRef.current;
    if (!card) return;
    const { clientX, clientY } = e;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      const r = card.getBoundingClientRect();
      card.style.setProperty('--mx', `${((clientX - r.left) / r.width) * 100}%`);
      card.style.setProperty('--my', `${((clientY - r.top) / r.height) * 100}%`);
    });
  };

  const fx = node.fx;
  const gainDb = fx?.gain_db ?? 0;
  // Vertical drag on the knob → gain (up = louder). 0.2 dB/px ≈ full sweep in 240px.
  const onKnobDown = (e: React.MouseEvent) => {
    if (!fx || fx.kind !== 'gain' || !onFxParams) return;
    e.preventDefault();
    e.stopPropagation();
    const startY = e.clientY;
    const startDb = fx.gain_db ?? 0;
    let last = startDb;
    const move = (ev: MouseEvent) => {
      const db = clamp(startDb + (startY - ev.clientY) * 0.2, GAIN_MIN, GAIN_MAX);
      const snapped = Math.round(db * 2) / 2; // 0.5 dB steps
      if (snapped !== last) {
        last = snapped;
        onFxParams(node.id, { ...fx, gain_db: snapped });
      }
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  const classes = [
    'node',
    `node--${node.kind}`,
    node.active ? 'is-active' : '',
    node.muted ? 'is-muted' : '',
    node.running === false ? 'is-idle' : '',
    node.selected ? 'is-selected' : '',
    search === 'match' ? 'is-search-match' : '',
    search === 'dim' ? 'is-search-dim' : '',
    chainState === 'on' ? 'is-solo-on' : '',
    chainState === 'off' ? 'is-solo-off' : '',
    soloSkipped ? 'is-solo-skipped' : '',
  ]
    .filter(Boolean)
    .join(' ');

  const rot = (gainDb / GAIN_MAX) * GAIN_SWEEP;
  const dim = fxDim(fx?.kind);

  return (
    <div
      className={classes}
      data-node-id={node.id}
      style={{ left: node.x, top: node.y, width: dim.w, ['--type-color' as string]: colorVar }}
    >
      <div className="node-label">
        <span className="node-label-dot" />
        {kindLabel(node.kind, node.micSink)}
        {node.solo && <span className="node-solo-tag">solo</span>}
        {pinned && <span className="node-pin-tag">pinned</span>}
        {soloSkipped && (
          <span className="node-skip-tag" title="effect bypassed while solo is active">
            skipped
          </span>
        )}
        {status && <span className={`node-link node-link--${status}`}>{status}</span>}
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onGlowMove} style={{ height: dim.h }}>
        <div className="node-glow" aria-hidden />

        {actions && (
          <NodeToolbar
            onAdvanced={onAdvanced ? () => onAdvanced(node.id) : undefined}
            soloActive={node.solo}
            onSolo={onSolo ? () => onSolo(node.id) : undefined}
            pinActive={pinned}
            onPin={onPin ? () => onPin(node.id) : undefined}
            onDuplicate={() => onDuplicate?.(node.id)}
            onDelete={() => onDelete?.(node.id)}
          />
        )}

        {node.hasInput !== false && (
          <span
            className="node-port node-port--in"
            data-node={node.id}
            data-side="in"
            data-port=""
            style={
              inSourceColorVar
                ? {
                    background: `var(${inSourceColorVar})`,
                    boxShadow: `0 0 9px -1px color-mix(in srgb, var(${inSourceColorVar}) 60%, transparent)`,
                  }
                : undefined
            }
          />
        )}
        {node.hasOutput !== false && (
          <span
            className={`node-port node-port--out${outConnected ? '' : ' is-unconnected'}`}
            data-node={node.id}
            data-side="out"
            data-port=""
          />
        )}

        <div className="node-head">
          <NodeIcon node={node} />
          <div className="node-titles">
            <EditableName
              className="node-name"
              value={node.name}
              onRename={onRename ? (name) => onRename(node.id, name) : undefined}
            />
            <div className="node-sub">{node.subtitle}</div>
          </div>
        </div>

        {fx?.kind === 'gain' && (
          <div className="fx-panel">
            <div className="knobUI">
              <div className="gc-knobwrap">
                <div className="knob" onMouseDown={onKnobDown} title="drag to set gain">
                  {KNOB_MAJ.map((a, i) => (
                    <span
                      key={`M${i}`}
                      className="ktick maj"
                      style={{ transform: `translate(-50%,-50%) rotate(${a}deg) translateY(-41.5px)` }}
                    />
                  ))}
                  {KNOB_MIN.map((a, i) => (
                    <span
                      key={`m${i}`}
                      className="ktick min"
                      style={{ transform: `translate(-50%,-50%) rotate(${a}deg) translateY(-41.5px)` }}
                    />
                  ))}
                  <svg className="karc" viewBox="0 0 84 84" aria-hidden>
                    <path d="M18.14 65.86 A33.75 33.75 0 1 1 65.86 65.86" fill="none" stroke="#313134" strokeWidth="2" />
                  </svg>
                  <span className="kdial" />
                  <span className="kptr fx" style={{ ['--rot' as string]: `${rot}deg` }} />
                </div>
              </div>
              <div className="gc-val">{gainDb > 0 ? `+${gainDb}` : gainDb} dB</div>
            </div>
          </div>
        )}
        {fx?.kind === 'gate' && (
          <GateBody
            node={node}
            fx={fx}
            onChange={onFxParams}
            reductionDb={reductionDb}
            inputLevel={inputLevel}
            active={active}
          />
        )}
        {fx?.kind === 'eq' && <EqBody node={node} fx={fx} onChange={onFxParams} />}
        {fx?.kind === 'limiter' && (
          <LimiterBody
            node={node}
            fx={fx}
            onChange={onFxParams}
            reductionDb={reductionDb}
            inputLevel={inputLevel}
            active={active}
          />
        )}
        {fx?.kind === 'compressor' && (
          <CompressorBody
            node={node}
            fx={fx}
            onChange={onFxParams}
            reductionDb={reductionDb}
            inputLevel={inputLevel}
          />
        )}
        {fx &&
          fx.kind !== 'gain' &&
          fx.kind !== 'gate' &&
          fx.kind !== 'eq' &&
          fx.kind !== 'limiter' &&
          fx.kind !== 'compressor' && (
            <div className="fx-panel">
              <div className="fx-placeholder">{node.subtitle || 'effect'}</div>
            </div>
          )}
      </div>
    </div>
  );
}
