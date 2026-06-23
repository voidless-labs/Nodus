import { useEffect, useRef, useState } from 'react';
import './Topbar.css';

/**
 * Topbar — the single persistent chrome bar (R10 + R22 multi-scene).
 *
 * Left: compact NODUS mark + wordmark. Center: scene navigator
 * (‹ Scene name × › +) — arrows cycle scenes, the pill shows the active one
 * (double-click to rename), × closes it, + adds a scene. Right: a "⋯" menu.
 */
export function Topbar({
  scenes = [{ id: 'scene-1', name: 'Scene 1' }],
  activeId = 'scene-1',
  onSwitch,
  onAdd,
  onClose,
  onRename,
}: {
  scenes?: { id: string; name: string }[];
  activeId?: string;
  onSwitch?: (id: string) => void;
  onAdd?: () => void;
  onClose?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
}) {
  const idx = Math.max(0, scenes.findIndex((s) => s.id === activeId));
  const active = scenes[idx] ?? scenes[0];
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(active.name);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) {
      setDraft(active.name);
      inputRef.current?.focus();
      inputRef.current?.select();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);

  const commit = () => {
    const name = draft.trim();
    if (name && name !== active.name) onRename?.(active.id, name);
    setEditing(false);
  };

  const go = (delta: number) => {
    const next = scenes[idx + delta];
    if (next) onSwitch?.(next.id);
  };

  return (
    <header className="topbar">
      <div className="topbar-left">
        <span className="brand-mark" aria-hidden />
        <span className="brand-name">nodus</span>
      </div>

      <nav className="scene-nav" aria-label="scenes">
        <button
          className="scene-add scene-remove"
          aria-label="delete scene"
          title="delete scene"
          disabled={scenes.length <= 1}
          onClick={() => onClose?.(active.id)}
        >
          <Minus />
        </button>
        <button
          className="scene-arrow"
          aria-label="previous scene"
          disabled={idx <= 0}
          onClick={() => go(-1)}
        >
          <ChevronLeft />
        </button>
        <div className="scene-pill">
          <span className="scene-dot" />
          {editing ? (
            <input
              ref={inputRef}
              className="scene-name-input"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={commit}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commit();
                if (e.key === 'Escape') setEditing(false);
              }}
            />
          ) : (
            <span className="scene-name" onDoubleClick={() => setEditing(true)} title="double-click to rename">
              {active.name}
            </span>
          )}
        </div>
        <button
          className="scene-arrow"
          aria-label="next scene"
          disabled={idx >= scenes.length - 1}
          onClick={() => go(1)}
        >
          <ChevronRight />
        </button>
        <button className="scene-add" aria-label="new scene" title="new scene" onClick={() => onAdd?.()}>
          <Plus />
        </button>
      </nav>

      <div className="topbar-right">
        <button className="topbar-menu" aria-label="menu">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <circle cx="5" cy="12" r="1.6" />
            <circle cx="12" cy="12" r="1.6" />
            <circle cx="19" cy="12" r="1.6" />
          </svg>
        </button>
      </div>
    </header>
  );
}

function ChevronLeft() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}
function ChevronRight() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m9 18 6-6-6-6" />
    </svg>
  );
}
function Plus() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}
function Minus() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 12h14" />
    </svg>
  );
}
