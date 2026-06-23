import type { NodeModel } from './types';

/**
 * NodeIcon — the avatar inside a node card.
 *
 * R4 placeholder: a rounded-square glyph chosen by node kind, or a single
 * letter for app sources. Real extracted app icons replace this in R7; the
 * component boundary stays the same so the swap is local.
 */
export function NodeIcon({ node }: { node: NodeModel }) {
  // Real extracted app icon wins; otherwise a kind glyph / letter fallback.
  if (node.icon) {
    return (
      <div className="node-icon node-icon--img">
        <img className="node-icon-img" src={node.icon} alt="" draggable={false} />
      </div>
    );
  }

  let glyph: JSX.Element;
  if (node.kind === 'output') glyph = <GlyphHeadphones />;
  else if (node.kind === 'virtual') glyph = node.micSink ? <GlyphMic /> : <GlyphSpeaker />;
  else if (node.kind === 'hub') glyph = <GlyphHub />;
  else if (node.kind === 'splitter') glyph = <GlyphSplitter />;
  else if (node.kind === 'fx') glyph = <GlyphFx />;
  else if (node.kind === 'logic') glyph = <GlyphLogic />;
  else if (node.avatar) glyph = <span className="node-icon-letter">{node.avatar}</span>;
  else glyph = <GlyphMic />;

  return <div className="node-icon">{glyph}</div>;
}

function GlyphMic() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="2" width="6" height="11" rx="3" />
      <path d="M5 10a7 7 0 0 0 14 0M12 17v4" />
    </svg>
  );
}

function GlyphHeadphones() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 14v-2a8 8 0 0 1 16 0v2" />
      <rect x="2" y="14" width="5" height="7" rx="2" />
      <rect x="17" y="14" width="5" height="7" rx="2" />
    </svg>
  );
}

function GlyphSpeaker() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="5" y="2" width="14" height="20" rx="3" />
      <circle cx="12" cy="14" r="4" />
      <circle cx="12" cy="6" r="1" />
    </svg>
  );
}

function GlyphHub() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="4" y1="7" x2="20" y2="7" />
      <line x1="4" y1="12" x2="20" y2="12" />
      <line x1="4" y1="17" x2="14" y2="17" />
    </svg>
  );
}

function GlyphSplitter() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 12h6M10 12l8-5M10 12l8 5" />
      <circle cx="19" cy="7" r="1.6" />
      <circle cx="19" cy="17" r="1.6" />
    </svg>
  );
}

function GlyphFx() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 12h3l3-7 6 14 3-7h3" />
    </svg>
  );
}

function GlyphLogic() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z" />
    </svg>
  );
}
