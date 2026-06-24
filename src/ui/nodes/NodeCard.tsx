import { useRef, useState } from 'react';
import './NodeCard.css';
import { KIND_COLOR_VAR, kindLabel, type NodeModel } from './types';
import { NodeIcon } from './NodeIcon';
import { VolumeSlider } from './VolumeSlider';
import { NodeToolbar } from './NodeToolbar';
import { EditableName } from './EditableName';

/**
 * NodeCard — one node on the canvas (R4, matched to Node-design-v2).
 *
 * Anatomy (top→bottom): kind label above · spacious card · icon + name +
 * secondary · a labelled "input/output level" meter in a recessed track ·
 * a bottom row with the mute button, volume slider and percent · side ports.
 * The card border reacts to the cursor (soft type-color glow tracking the
 * pointer), throttled to one rAF and only while THIS card is hovered.
 */
function meterLabel(node: NodeModel): string {
  if (node.kind === 'source') return 'input level';
  if (node.kind === 'output' || (node.kind === 'virtual' && !node.micSink)) return 'output level';
  if (node.kind === 'virtual' && node.micSink) return 'input level';
  return 'level';
}

export function NodeCard({
  node,
  search,
  actions,
  onVolume,
  onMute,
  onSolo,
  onPin,
  pinned,
  onDuplicate,
  onDelete,
  onRename,
}: {
  node: NodeModel;
  search?: 'match' | 'dim';
  /** Show the action toolbar (sole selected node) — R20. */
  actions?: boolean;
  onVolume?: (id: string, volume: number) => void;
  onMute?: (id: string) => void;
  onSolo?: (id: string) => void;
  onPin?: (id: string) => void;
  pinned?: boolean;
  onDuplicate?: (id: string) => void;
  onDelete?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
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

  const classes = [
    'node',
    `node--${node.kind}`,
    node.active ? 'is-active' : '',
    node.muted ? 'is-muted' : '',
    node.running === false ? 'is-idle' : '',
    node.selected ? 'is-selected' : '',
    node.compact ? 'is-compact' : '',
    search === 'match' ? 'is-search-match' : '',
    search === 'dim' ? 'is-search-dim' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      className={classes}
      data-node-id={node.id}
      style={{ left: node.x, top: node.y, ['--type-color' as string]: colorVar }}
    >
      <div className="node-label">
        <span className="node-label-dot" />
        {kindLabel(node.kind, node.micSink)}
        {node.solo && <span className="node-solo-tag">solo</span>}
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onGlowMove}>
        <div className="node-glow" aria-hidden />

        {actions && (
          <NodeToolbar
            soloActive={node.solo}
            onSolo={onSolo ? () => onSolo(node.id) : undefined}
            pinActive={pinned}
            onPin={onPin ? () => onPin(node.id) : undefined}
            onDuplicate={() => onDuplicate?.(node.id)}
            onDelete={() => onDelete?.(node.id)}
          />
        )}

        {node.hasInput !== false && (
          <span className="node-port node-port--in" data-node={node.id} data-side="in" data-port="" />
        )}
        {node.hasOutput !== false && (
          <span className="node-port node-port--out" data-node={node.id} data-side="out" data-port="" />
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

        <div className="node-meter-block">
          <div className="node-meter-label">{meterLabel(node)}</div>
          <div className="node-meter" aria-hidden>
            <div className="node-meter-fill" style={{ width: `${node.level * 100}%` }} />
          </div>
        </div>

        <div className="node-vol">
          <button
            className="node-mute"
            title={node.muted ? 'unmute' : 'mute'}
            aria-label="mute"
            onClick={() => onMute?.(node.id)}
          >
            {node.muted ? <IconMuted /> : <IconSpeaker />}
          </button>
          <VolumeSlider
            value={node.volume}
            onChange={(v) => onVolume?.(node.id, v)}
            ariaLabel="volume"
          />
        </div>
      </div>
    </div>
  );
}

function IconSpeaker() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
    </svg>
  );
}

function IconMuted() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <line x1="22" y1="9" x2="16" y2="15" />
      <line x1="16" y1="9" x2="22" y2="15" />
    </svg>
  );
}
