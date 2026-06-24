import { useState } from 'react';
import './QuickPanel.css';
import type { HubModel, NodeModel } from './nodes/types';
import { VolumeSlider } from './nodes/VolumeSlider';

/**
 * QuickPanel — the quick-controls popup (t13, Phase A).
 *
 * Shows the scene's pinned nodes as a compact list (concept ref: a VPN app's
 * "groups" list). A pinned node with a single control (a leaf) shows inline; a
 * node with more than one VU slider (a Mixer/hub with several inputs) is
 * collapsed by default and expands on click to reveal all its sliders. A
 * start/stop engine toggle sits at the bottom. Controls reuse the live store
 * mutations (per-route volume/mute/solo) so they drive the canvas + engine.
 *
 * Phase B will host this from the system tray; for now it is an in-app overlay.
 */
export type QuickItem =
  | { kind: 'node'; node: NodeModel }
  | { kind: 'hub'; hub: HubModel };

export function QuickPanel({
  items,
  live,
  onToggleLive,
  onNodeVolume,
  onNodeMute,
  onNodeSolo,
  onHubInputVolume,
  onUnpin,
  onClose,
  variant = 'popover',
}: {
  items: QuickItem[];
  live: boolean;
  onToggleLive: () => void;
  onNodeVolume: (id: string, volume: number) => void;
  onNodeMute: (id: string) => void;
  onNodeSolo: (id: string) => void;
  onHubInputVolume: (hubId: string, inputId: string, volume: number) => void;
  onUnpin: (id: string) => void;
  onClose: () => void;
  /** 'popover' = floating in-app overlay; 'window' = fills a dedicated tray window. */
  variant?: 'popover' | 'window';
}) {
  return (
    <>
      {variant === 'popover' && <div className="qp-overlay" onClick={onClose} />}
      <div className={`qp ${variant === 'window' ? 'qp--window' : ''}`} role="dialog" aria-label="quick controls">
        <div className="qp-head">
          <span className="qp-title">quick controls</span>
          <button className="qp-x" aria-label="close" onClick={onClose}>
            <Close />
          </button>
        </div>

        <QuickList
          items={items}
          onNodeVolume={onNodeVolume}
          onNodeMute={onNodeMute}
          onNodeSolo={onNodeSolo}
          onHubInputVolume={onHubInputVolume}
          onUnpin={onUnpin}
        />

        <button
          className={`qp-engine ${live ? 'is-live' : ''}`}
          onClick={onToggleLive}
        >
          <span className="qp-engine-dot" />
          {live ? 'Stop engine' : 'Start engine'}
        </button>
      </div>
    </>
  );
}

/**
 * QuickList — the scrollable list of pinned-node control rows. Shared by the
 * in-app popover (QuickPanel) and the tray flyout (FlyoutApp). Hubs (multi-
 * slider) start collapsed and expand on click.
 */
export function QuickList({
  items,
  onNodeVolume,
  onNodeMute,
  onNodeSolo,
  onHubInputVolume,
  onUnpin,
}: {
  items: QuickItem[];
  onNodeVolume: (id: string, volume: number) => void;
  onNodeMute: (id: string) => void;
  onNodeSolo: (id: string) => void;
  onHubInputVolume: (hubId: string, inputId: string, volume: number) => void;
  onUnpin: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggle = (id: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  return (
    <div className="qp-list">
      {items.length === 0 && (
        <div className="qp-empty">pin a node (📌 in its toolbar) to control it here</div>
      )}
      {items.map((it) =>
        it.kind === 'node' ? (
          <NodeRow
            key={it.node.id}
            node={it.node}
            onVolume={onNodeVolume}
            onMute={onNodeMute}
            onSolo={onNodeSolo}
            onUnpin={onUnpin}
          />
        ) : (
          <HubRow
            key={it.hub.id}
            hub={it.hub}
            open={expanded.has(it.hub.id)}
            onToggle={() => toggle(it.hub.id)}
            onInputVolume={onHubInputVolume}
            onUnpin={onUnpin}
          />
        ),
      )}
    </div>
  );
}

function NodeRow({
  node,
  onVolume,
  onMute,
  onSolo,
  onUnpin,
}: {
  node: NodeModel;
  onVolume: (id: string, v: number) => void;
  onMute: (id: string) => void;
  onSolo: (id: string) => void;
  onUnpin: (id: string) => void;
}) {
  return (
    <div className={`qp-row ${node.muted ? 'is-muted' : ''}`}>
      <div className="qp-row-top">
        <span className="qp-name">{node.name}</span>
        <div className="qp-row-actions">
          <button
            className={`qp-mini ${node.solo ? 'is-active' : ''}`}
            title="solo"
            aria-label="solo"
            onClick={() => onSolo(node.id)}
          >
            S
          </button>
          <button
            className={`qp-mini ${node.muted ? 'is-active' : ''}`}
            title="mute"
            aria-label="mute"
            onClick={() => onMute(node.id)}
          >
            <Mute muted={!!node.muted} />
          </button>
          <button className="qp-mini" title="unpin" aria-label="unpin" onClick={() => onUnpin(node.id)}>
            <Pin />
          </button>
        </div>
      </div>
      <VolumeSlider
        className="qp-slider"
        value={node.volume ?? 0}
        onChange={(v) => onVolume(node.id, v)}
        ariaLabel={`${node.name} level`}
      />
    </div>
  );
}

function HubRow({
  hub,
  open,
  onToggle,
  onInputVolume,
  onUnpin,
}: {
  hub: HubModel;
  open: boolean;
  onToggle: () => void;
  onInputVolume: (hubId: string, inputId: string, v: number) => void;
  onUnpin: (id: string) => void;
}) {
  return (
    <div className={`qp-row qp-row--hub ${open ? 'is-open' : ''}`}>
      <button className="qp-row-top qp-row-toggle" onClick={onToggle} aria-expanded={open}>
        <Chevron open={open} />
        <span className="qp-name">{hub.name}</span>
        <span className="qp-count">{hub.inputs.length} inputs</span>
        <span
          className="qp-mini qp-unpin"
          role="button"
          aria-label="unpin"
          title="unpin"
          onClick={(e) => {
            e.stopPropagation();
            onUnpin(hub.id);
          }}
        >
          <Pin />
        </span>
      </button>
      {open && (
        <div className="qp-inputs">
          {hub.inputs.map((inp) => (
            <div className="qp-in-row" key={inp.id}>
              <span className="qp-in-name">{inp.label}</span>
              <VolumeSlider
                className="qp-slider"
                value={inp.volume ?? 0}
                onChange={(v) => onInputVolume(hub.id, inp.id, v)}
                ariaLabel={`${inp.label} level`}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const IC = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
function Close() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" {...IC}>
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}
function Chevron({ open }: { open: boolean }) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" {...IC} className={`qp-chev ${open ? 'is-open' : ''}`}>
      <path d="m9 18 6-6-6-6" />
    </svg>
  );
}
function Pin() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" {...IC}>
      <path d="M12 17v5M9 3h6l-1 6 3 3v1H7v-1l3-3-1-6Z" />
    </svg>
  );
}
function Mute({ muted }: { muted: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" {...IC}>
      <path d="M11 5 6 9H2v6h4l5 4V5Z" />
      {muted ? <path d="M22 9l-6 6M16 9l6 6" /> : <path d="M16 8a5 5 0 0 1 0 8" />}
    </svg>
  );
}
