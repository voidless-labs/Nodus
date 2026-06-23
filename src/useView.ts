import { useCallback, useMemo, useRef, useState } from 'react';
import type { HubModel, NodeModel } from './ui/nodes/types';

/**
 * useView — canvas pan/zoom state (R21).
 *
 * `view` is the world→screen transform applied to the graph: a screen point is
 * `view.x + world * view.zoom`. Pan adds screen pixels to x/y; zoom scales about
 * a focal point (cursor for the wheel, viewport centre for the buttons). `fit`
 * frames all nodes. The viewport size comes from `viewportRef` (the canvas area).
 */
export interface View {
  x: number;
  y: number;
  zoom: number;
}

const MIN_ZOOM = 0.3;
const MAX_ZOOM = 2.2;
const clampZoom = (z: number) => Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, z));

// Rough node footprint for fit-to-view (cards vary; this is close enough to frame).
const NODE_W = 230;
const NODE_H = 210;

export interface ViewController {
  view: View;
  setView: React.Dispatch<React.SetStateAction<View>>;
  /** Zoom about a focal point in viewport-local pixels (default: centre). */
  zoomBy: (factor: number, focusX?: number, focusY?: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
  fit: (items: (NodeModel | HubModel)[]) => void;
}

export function useView(viewportRef: React.RefObject<HTMLElement>): ViewController {
  const [view, setView] = useState<View>({ x: 0, y: 0, zoom: 1 });
  const viewRef = useRef(view);
  viewRef.current = view;

  const viewportSize = useCallback(() => {
    const r = viewportRef.current?.getBoundingClientRect();
    return { w: r?.width ?? 0, h: r?.height ?? 0 };
  }, [viewportRef]);

  const zoomBy = useCallback(
    (factor: number, focusX?: number, focusY?: number) => {
      setView((v) => {
        const { w, h } = viewportSize();
        const fx = focusX ?? w / 2;
        const fy = focusY ?? h / 2;
        const zoom = clampZoom(v.zoom * factor);
        const k = zoom / v.zoom;
        return { zoom, x: fx - (fx - v.x) * k, y: fy - (fy - v.y) * k };
      });
    },
    [viewportSize],
  );

  const zoomIn = useCallback(() => zoomBy(1.2), [zoomBy]);
  const zoomOut = useCallback(() => zoomBy(1 / 1.2), [zoomBy]);
  const resetZoom = useCallback(() => zoomBy((1 / viewRef.current.zoom) || 1), [zoomBy]);

  const fit = useCallback(
    (items: (NodeModel | HubModel)[]) => {
      const { w, h } = viewportSize();
      if (!items.length || !w || !h) {
        setView({ x: 0, y: 0, zoom: 1 });
        return;
      }
      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;
      for (const n of items) {
        minX = Math.min(minX, n.x);
        minY = Math.min(minY, n.y);
        maxX = Math.max(maxX, n.x + NODE_W);
        maxY = Math.max(maxY, n.y + NODE_H);
      }
      const pad = 80;
      const bw = maxX - minX;
      const bh = maxY - minY;
      const zoom = Math.max(MIN_ZOOM, Math.min(1.3, Math.min((w - pad * 2) / bw, (h - pad * 2) / bh)));
      setView({
        zoom,
        x: (w - bw * zoom) / 2 - minX * zoom,
        y: (h - bh * zoom) / 2 - minY * zoom,
      });
    },
    [viewportSize],
  );

  return useMemo(
    () => ({ view, setView, zoomBy, zoomIn, zoomOut, resetZoom, fit }),
    [view, zoomBy, zoomIn, zoomOut, resetZoom, fit],
  );
}
