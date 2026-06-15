import './EmptyCanvas.css';
import type { PresetId } from '../scenes';

/**
 * EmptyCanvas — first-run empty state (R16).
 *
 * A ghost source→output hint in the centre, a one-line prompt, and a few
 * preset cards that build a ready graph. Shown whenever the scene has no
 * nodes; disappears as soon as the first node lands.
 */
const PRESETS: { id: PresetId; emoji: string; title: string }[] = [
  { id: 'stream', emoji: '🎮', title: 'Streaming · OBS hears the game, you don’t' },
  { id: 'discord', emoji: '🎵', title: 'Music in Discord' },
  { id: 'headphones', emoji: '🎧', title: 'Everything → headphones' },
];

export function EmptyCanvas({ onPreset }: { onPreset: (id: PresetId) => void }) {
  return (
    <div className="empty-canvas">
      <div className="ec-ghost" aria-hidden>
        <div className="ec-ghost-node" />
        <svg className="ec-ghost-wire" width="120" height="40" viewBox="0 0 120 40">
          <path d="M2 20 C 40 20, 80 20, 118 20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
          <path d="M110 13 l8 7 -8 7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <div className="ec-ghost-node" />
      </div>

      <p className="ec-prompt">drag an app or device here — or start from a preset</p>

      <div className="ec-presets">
        {PRESETS.map((p) => (
          <button key={p.id} className="ec-preset" onClick={() => onPreset(p.id)}>
            <span className="ec-preset-emoji">{p.emoji}</span>
            <span className="ec-preset-title">{p.title}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
