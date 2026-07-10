import './Canvas.css';

/**
 * Canvas — the infinite work surface backdrop (redesign R2).
 *
 * For now it renders only the static backdrop: graphite fill, a dim dot grid,
 * and an edge vignette. Nodes, edges and interaction come in later sub-tasks
 * (R4+). The dot grid is pure CSS (a tiled radial-gradient) so it costs nothing
 * to paint — important for the weak-laptop performance budget.
 */
export function Canvas({
  children,
  view,
}: {
  children?: React.ReactNode;
  /** Pan/zoom transform — offsets and scales the dot grid so it reads as anchored. */
  view?: { x: number; y: number; zoom: number };
}) {
  const gridStyle = view
    ? {
        backgroundPosition: `${view.x}px ${view.y}px`,
        backgroundSize: `${24 * view.zoom}px ${24 * view.zoom}px`,
      }
    : undefined;
  return (
    <div className="canvas" role="application" aria-label="Nodus routing canvas">
      <div className="canvas-grid" aria-hidden style={gridStyle} />
      <div className="canvas-vignette" aria-hidden />
      <div className="canvas-content">{children}</div>
    </div>
  );
}
