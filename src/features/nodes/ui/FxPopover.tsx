import './FxPopover.css';
import type { FxSpec } from '@/shared/bridge';
import type { NodeModel } from '@/features/nodes/types';

/**
 * FxPopover — parameter controls for the selected FX node (t18). Minimal/functional
 * (a full design pass comes before 1.0.0). Sliders drive the engine live via
 * onChange → set_fx_params; the node keeps the spec so it persists + mirrors.
 * Rendered inside the canvas world layer, anchored above the FX card.
 */
const KIND_LABEL: Record<string, string> = { gain: 'Gain', gate: 'Noise Gate', eq: 'EQ' };

function Slider({
  label,
  unit,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string;
  unit: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="fx-row">
      <span className="fx-row-label">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
      />
      <span className="fx-row-val">
        {Number.isInteger(value) ? value : value.toFixed(1)}
        {unit}
      </span>
    </label>
  );
}

export function FxPopover({
  node,
  onChange,
}: {
  node: NodeModel;
  onChange: (fx: FxSpec) => void;
}) {
  const fx = node.fx;
  if (!fx || (fx.kind !== 'gain' && fx.kind !== 'gate' && fx.kind !== 'eq')) return null;
  const set = (patch: Partial<FxSpec>) => onChange({ ...fx, ...patch });

  return (
    <div
      className={`fx-pop ${fx.bypassed ? 'is-bypassed' : ''}`}
      style={{ left: node.x, top: node.y - 96 }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div className="fx-pop-head">
        <span className="fx-pop-title">{KIND_LABEL[fx.kind]}</span>
        <button
          className={`fx-bypass ${fx.bypassed ? 'is-on' : ''}`}
          title="bypass this effect"
          onClick={() => set({ bypassed: !fx.bypassed })}
        >
          bypass
        </button>
      </div>

      {fx.kind === 'gain' && (
        <Slider
          label="gain"
          unit=" dB"
          min={-24}
          max={24}
          step={0.5}
          value={fx.gain_db ?? 0}
          onChange={(v) => set({ gain_db: v })}
        />
      )}

      {fx.kind === 'gate' && (
        <>
          <Slider
            label="open"
            unit=" dB"
            min={-80}
            max={0}
            step={1}
            value={fx.open_db ?? -45}
            onChange={(v) => set({ open_db: v })}
          />
          <Slider
            label="close"
            unit=" dB"
            min={-80}
            max={0}
            step={1}
            value={fx.close_db ?? -55}
            onChange={(v) => set({ close_db: v })}
          />
        </>
      )}

      {fx.kind === 'eq' && (
        <>
          <Slider
            label="freq"
            unit=" Hz"
            min={20}
            max={18000}
            step={10}
            value={fx.freq ?? 1000}
            onChange={(v) => set({ freq: v })}
          />
          <Slider
            label="Q"
            unit=""
            min={0.1}
            max={10}
            step={0.1}
            value={fx.q ?? 1}
            onChange={(v) => set({ q: v })}
          />
          <Slider
            label="gain"
            unit=" dB"
            min={-24}
            max={24}
            step={0.5}
            value={fx.gain_db ?? 0}
            onChange={(v) => set({ gain_db: v })}
          />
        </>
      )}
    </div>
  );
}
