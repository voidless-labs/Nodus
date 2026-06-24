import { useEffect, useRef, useState } from 'react';
import { winMinimize, winToggleMaximize, winHide, winIsMaximized, onWinResize } from '../bridge';
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
  windowControls = true,
  sceneEditing = true,
}: {
  scenes?: { id: string; name: string }[];
  activeId?: string;
  onSwitch?: (id: string) => void;
  onAdd?: () => void;
  onClose?: (id: string) => void;
  onRename?: (id: string, name: string) => void;
  /** Show the min/max/close window buttons (off in the tray flyout). */
  windowControls?: boolean;
  /** Show the add (+) / delete (−) scene buttons (off in the flyout — there you
   *  can only switch scenes; add/delete live in the main UI). */
  sceneEditing?: boolean;
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
    <header className="topbar" data-tauri-drag-region>
      <div className="topbar-left" data-tauri-drag-region>
        <span className="brand-mark" aria-hidden data-tauri-drag-region />
        <span className="brand-name" data-tauri-drag-region>nodus</span>
      </div>

      <nav className="scene-nav" aria-label="scenes">
        {sceneEditing && (
          <button
            className="scene-add scene-remove"
            aria-label="delete scene"
            title="delete scene"
            disabled={scenes.length <= 1}
            onClick={() => onClose?.(active.id)}
          >
            <Minus />
          </button>
        )}
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
        {sceneEditing && (
          <button className="scene-add" aria-label="new scene" title="new scene" onClick={() => onAdd?.()}>
            <Plus />
          </button>
        )}
      </nav>

      <div className="topbar-right">{windowControls && <WindowControls />}</div>
    </header>
  );
}

/**
 * WindowControls — custom title-bar buttons (replaces the native Windows
 * caption now that the window is frameless: tauri.conf decorations=false).
 * Minimize · Maximize/Restore · Close. No-ops in the browser preview.
 */
function WindowControls() {
  const [maxed, setMaxed] = useState(false);

  useEffect(() => {
    let unlisten = () => {};
    let alive = true;
    const sync = () => winIsMaximized().then((m) => alive && setMaxed(m));
    sync();
    onWinResize(sync).then((fn) => {
      if (alive) unlisten = fn;
      else fn();
    });
    return () => {
      alive = false;
      unlisten();
    };
  }, []);

  return (
    <div className="win-controls">
      <button
        className="win-btn"
        aria-label="Свернуть окно"
        title="Свернуть окно"
        onClick={() => winMinimize()}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round">
          <path d="M3.5 8h9" />
        </svg>
      </button>
      <button
        className="win-btn"
        aria-label={maxed ? 'Восстановить окно' : 'Развернуть окно'}
        title={maxed ? 'Восстановить окно' : 'Развернуть окно'}
        onClick={() => winToggleMaximize()}
      >
        {maxed ? (
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
            <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
            <path d="M6 4.5V3.5A1 1 0 0 1 7 2.5h5A1 1 0 0 1 13 3.5v5a1 1 0 0 1-1 1h-1" />
          </svg>
        ) : (
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
            <rect x="3.5" y="3.5" width="9" height="9" rx="1" />
          </svg>
        )}
      </button>
      <button
        className="win-btn win-close"
        aria-label="Свернуть в трей"
        title="Свернуть в трей (выход — из меню трея)"
        onClick={() => winHide()}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round">
          <path d="M4 4l8 8M12 4l-8 8" />
        </svg>
      </button>
    </div>
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
