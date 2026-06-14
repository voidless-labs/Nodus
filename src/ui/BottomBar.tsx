import { useState } from 'react';
import './BottomBar.css';

/**
 * BottomBar — the floating node library (R12).
 *
 * A search field over a row of category tabs (recent / all / fx / logic /
 * misc), matched to the Claude-design bottom bar. Visual only for now: picking
 * a node and dropping it on the canvas is wired with the engine in R3/R13.
 */
type Tab = 'recent' | 'all' | 'fx' | 'logic' | 'misc';

export function BottomBar() {
  const [tab, setTab] = useState<Tab>('all');

  return (
    <div className="bottombar">
      <label className="bb-search">
        <SearchIcon />
        <input placeholder="search nodes · type a name to highlight…" />
      </label>
      <div className="bb-tabs">
        <BBTab active={tab === 'recent'} onClick={() => setTab('recent')} label="recent">
          <ClockIcon />
        </BBTab>
        <BBTab active={tab === 'all'} onClick={() => setTab('all')} label="all">
          <GridIcon />
        </BBTab>
        <BBTab active={tab === 'fx'} onClick={() => setTab('fx')} label="fx">
          <SlidersIcon />
        </BBTab>
        <BBTab active={tab === 'logic'} onClick={() => setTab('logic')} label="logic">
          <BoltIcon />
        </BBTab>
        <div className="bb-spacer" />
        <BBTab active={tab === 'misc'} onClick={() => setTab('misc')} label="misc">
          <SparkleIcon />
        </BBTab>
      </div>
    </div>
  );
}

function BBTab({
  active,
  onClick,
  label,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <button className={`bb-tab ${active ? 'is-active' : ''}`} onClick={onClick} title={label} aria-label={label}>
      {children}
    </button>
  );
}

const ic = {
  width: 16,
  height: 16,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

function SearchIcon() {
  return (
    <svg {...ic} width={15} height={15}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}
function ClockIcon() {
  return (
    <svg {...ic}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}
function GridIcon() {
  return (
    <svg {...ic}>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </svg>
  );
}
function SlidersIcon() {
  return (
    <svg {...ic}>
      <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" />
      <circle cx="15" cy="6" r="2" />
      <circle cx="9" cy="12" r="2" />
      <circle cx="17" cy="18" r="2" />
    </svg>
  );
}
function BoltIcon() {
  return (
    <svg {...ic}>
      <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z" />
    </svg>
  );
}
function SparkleIcon() {
  return (
    <svg {...ic}>
      <path d="M12 3v18M3 12h18M6 6l12 12M18 6 6 18" />
    </svg>
  );
}
