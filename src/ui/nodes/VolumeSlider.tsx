import { useState } from 'react';

/**
 * VolumeSlider — a range input with a value bubble that appears above the thumb
 * on hover/drag (replaces the always-on percent readout).
 *
 * Controlled when both `value` and `onChange` are given (node master volume);
 * otherwise uncontrolled around `defaultValue` (hub inputs, until live-wired).
 * The bubble's horizontal position is driven by the `--p` custom property.
 */
export function VolumeSlider({
  value,
  defaultValue,
  onChange,
  ariaLabel,
  className = '',
}: {
  /** Controlled value 0..1. */
  value?: number;
  /** Uncontrolled initial value 0..1. */
  defaultValue?: number;
  onChange?: (v: number) => void;
  ariaLabel: string;
  className?: string;
}) {
  const controlled = value !== undefined && onChange !== undefined;
  const [internal, setInternal] = useState(Math.round((defaultValue ?? value ?? 0) * 100));
  const pct = controlled ? Math.round((value as number) * 100) : internal;

  const handle = (e: React.ChangeEvent<HTMLInputElement>) => {
    const p = Number(e.target.value);
    if (controlled) onChange!(p / 100);
    else setInternal(p);
  };

  return (
    <span className={`vslider ${className}`} style={{ ['--p' as string]: pct }}>
      <input
        className="node-slider"
        type="range"
        min={0}
        max={100}
        value={pct}
        onChange={handle}
        aria-label={ariaLabel}
      />
      <span className="vslider-bubble" aria-hidden>
        {pct}
      </span>
    </span>
  );
}
