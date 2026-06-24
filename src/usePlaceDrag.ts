import { useCallback, useRef, useState } from 'react';

/**
 * usePlaceDrag — pointer-based "drag a palette item onto the canvas".
 *
 * Native HTML5 drag-and-drop does not reliably start inside WebView2 (a drag
 * attempt collapses into a click), so we implement placement with raw pointer
 * events instead: press on a palette row, move past a small threshold to start
 * a drag (a ghost follows the cursor), release over the canvas to drop. A press
 * without movement is treated as a tap (quick-add). Window-level listeners mean
 * the source element may unmount mid-drag without aborting anything.
 */
export interface PlacePayload {
  kind: 'device' | 'process' | 'type';
  id: string;
}

interface View {
  x: number;
  y: number;
  zoom: number;
}

interface DragState {
  payload: PlacePayload;
  label: string;
  onTap: () => void;
  startX: number;
  startY: number;
  active: boolean;
}

const THRESHOLD = 5; // px the pointer must travel before it counts as a drag

export function usePlaceDrag(opts: {
  canvasRef: React.RefObject<HTMLElement>;
  viewRef: React.MutableRefObject<View>;
  onPlace: (payload: PlacePayload, world: { x: number; y: number }) => void;
}) {
  const { canvasRef, viewRef } = opts;
  const onPlaceRef = useRef(opts.onPlace);
  onPlaceRef.current = opts.onPlace;

  const stateRef = useRef<DragState | null>(null);
  const [ghost, setGhost] = useState<{ label: string; x: number; y: number } | null>(null);

  // Stable handlers (no deps) so the same identities are added/removed.
  const onMove = useCallback((e: PointerEvent) => {
    const s = stateRef.current;
    if (!s) return;
    if (!s.active && Math.hypot(e.clientX - s.startX, e.clientY - s.startY) > THRESHOLD) {
      s.active = true;
    }
    if (s.active) setGhost({ label: s.label, x: e.clientX, y: e.clientY });
  }, []);

  const onUp = useCallback(
    (e: PointerEvent) => {
      const s = stateRef.current;
      stateRef.current = null;
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      setGhost(null);
      if (!s) return;
      if (!s.active) {
        s.onTap(); // a tap, not a drag → quick-add
        return;
      }
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      // Drop only counts if released over the canvas area.
      if (
        e.clientX < rect.left ||
        e.clientX > rect.right ||
        e.clientY < rect.top ||
        e.clientY > rect.bottom
      )
        return;
      const v = viewRef.current;
      onPlaceRef.current(s.payload, {
        x: (e.clientX - rect.left - v.x) / v.zoom,
        y: (e.clientY - rect.top - v.y) / v.zoom,
      });
    },
    [onMove, canvasRef, viewRef],
  );

  /** Begin a candidate drag from a palette row's pointerdown. */
  const begin = useCallback(
    (e: React.PointerEvent, payload: PlacePayload, label: string, onTap: () => void) => {
      if (e.button !== 0) return; // primary button only
      e.preventDefault();
      stateRef.current = {
        payload,
        label,
        onTap,
        startX: e.clientX,
        startY: e.clientY,
        active: false,
      };
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    },
    [onMove, onUp],
  );

  return { begin, ghost };
}
