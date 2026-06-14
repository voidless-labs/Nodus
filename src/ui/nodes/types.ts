/** Node taxonomy for the redesigned canvas (R4/R5). */

export type NodeKind =
  | 'source' // app or microphone the audio comes FROM
  | 'output' // headphones / speakers / OBS the audio goes TO
  | 'virtual' // a Nodus virtual device (speaker, or mic-sink)
  | 'hub' // optional routing hub ("mixer") node
  | 'fx' // an effect on a route
  | 'logic'; // PTT / hotkey / toggle

/** CSS custom-property name carrying each kind's color and glow. */
export const KIND_COLOR_VAR: Record<NodeKind, string> = {
  source: '--color-type-source',
  output: '--color-type-output',
  virtual: '--color-type-virtual',
  hub: '--color-type-hub',
  fx: '--color-type-fx',
  logic: '--color-type-logic',
};

export const KIND_GLOW_VAR: Partial<Record<NodeKind, string>> = {
  source: '--glow-source',
  output: '--glow-output',
  virtual: '--glow-virtual',
  fx: '--glow-fx',
};

/** Lowercase label shown above the card. A virtual mic-sink reads "mic". */
export function kindLabel(kind: NodeKind, isMicSink = false): string {
  if (kind === 'virtual' && isMicSink) return 'mic';
  return kind;
}

/** The data a NodeCard needs to render. Wired to the engine in R3. */
export interface NodeModel {
  id: string;
  kind: NodeKind;
  /** Whether a virtual node is the mic-sink (audio flows INTO it). */
  micSink?: boolean;
  name: string;
  /** Secondary line: system device name or status. */
  subtitle: string;
  /** Single-letter avatar fallback until real app icons land (R7). */
  avatar?: string;
  /** VU level 0..1 (live ~15fps once wired). */
  level: number;
  /** Route/source volume 0..1. */
  volume: number;
  muted?: boolean;
  /** Audio is currently flowing → static glow + live meter. */
  active?: boolean;
  /** For app sources: the process is not running. */
  running?: boolean;
  /** Canvas position (no pan/zoom yet; placed absolutely). */
  x: number;
  y: number;
  /** Has an input port (left) / output port (right). */
  hasInput?: boolean;
  hasOutput?: boolean;
  /** Selected → accent outline. */
  selected?: boolean;
  /** Compact → icon + name + meter only (no slider). */
  compact?: boolean;
}

/** A hub ("Stream Mix") node — per-input volumes → one mixed output. */
export interface HubModel {
  id: string;
  name: string;
  subtitle: string;
  /** Each connected input with its own level in the mix (0..1). */
  inputs: { id: string; label: string; volume: number }[];
  /** Output mix level 0..1 (for the bottom meter). */
  level: number;
  active?: boolean;
  selected?: boolean;
  x: number;
  y: number;
}

/** One wire from a source node's output port to a target node's input port. */
export interface EdgeModel {
  id: string;
  from: string; // source node id (output port)
  to: string; // target node id (input port)
  /** Target a specific labeled input (hub nodes); default = the node's single input. */
  toPort?: string;
  active?: boolean; // audio flowing → brighter + faint glow
  muted?: boolean; // dashed, dim red
}
