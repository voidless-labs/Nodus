import { useRef } from 'react';
import './NodeCard.css';
import { KIND_COLOR_VAR, kindLabel, type NodeModel } from './types';
import { NodeIcon } from './NodeIcon';

/**
 * NodeCard — one node on the canvas (R4).
 *
 * Anatomy (top→bottom): kind label above the card · frosted card with
 * icon + name + secondary line + mute · VU meter · volume slider · side ports.
 * The card border reacts to the cursor: a soft glow in the node's type color
 * tracks the pointer along the nearest edge. The handler only runs while THIS
 * card is hovered (one node computes at a time) and is throttled to one
 * rAF per move — cheap enough for the weak-laptop budget.
 *
 * R4 is visual only: props are sample data; wiring to the engine is R3.
 */
export function NodeCard({ node }: { node: NodeModel }) {
  const cardRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number | null>(null);

  const colorVar = `var(${KIND_COLOR_VAR[node.kind]})`;

  const onMove = (e: React.MouseEvent) => {
    if (rafRef.current != null) return;
    const card = cardRef.current;
    if (!card) return;
    const { clientX, clientY } = e;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      const r = card.getBoundingClientRect();
      const mx = ((clientX - r.left) / r.width) * 100;
      const my = ((clientY - r.top) / r.height) * 100;
      card.style.setProperty('--mx', `${mx}%`);
      card.style.setProperty('--my', `${my}%`);
    });
  };

  const classes = [
    'node',
    `node--${node.kind}`,
    node.active ? 'is-active' : '',
    node.muted ? 'is-muted' : '',
    node.running === false ? 'is-idle' : '',
  ]
    .filter(Boolean)
    .join(' ');

  const pct = Math.round(node.volume * 100);

  return (
    <div
      className={classes}
      style={{ left: node.x, top: node.y, ['--type-color' as string]: colorVar }}
    >
      <div className="node-label">
        <span className="node-label-dot" />
        {kindLabel(node.kind, node.micSink)}
      </div>

      <div className="node-card" ref={cardRef} onMouseMove={onMove}>
        <div className="node-glow" aria-hidden />

        {node.hasInput !== false && <span className="node-port node-port--in" />}
        {node.hasOutput !== false && <span className="node-port node-port--out" />}

        <div className="node-head">
          <NodeIcon node={node} />
          <div className="node-titles">
            <div className="node-name">{node.name}</div>
            <div className="node-sub">{node.subtitle}</div>
          </div>
          <button className="node-mute" title={node.muted ? 'unmute' : 'mute'} aria-label="mute">
            {node.muted ? <IconMuted /> : <IconSpeaker />}
          </button>
        </div>

        <div className="node-meter" aria-hidden>
          <div className="node-meter-fill" style={{ width: `${node.level * 100}%` }} />
        </div>

        <div className="node-vol">
          <input
            className="node-slider"
            type="range"
            min={0}
            max={100}
            defaultValue={pct}
            aria-label="volume"
          />
          <span className="node-vol-pct">{pct}%</span>
        </div>
      </div>
    </div>
  );
}

function IconSpeaker() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
    </svg>
  );
}

function IconMuted() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5 6 9H2v6h4l5 4V5z" />
      <line x1="22" y1="9" x2="16" y2="15" />
      <line x1="16" y1="9" x2="22" y2="15" />
    </svg>
  );
}
