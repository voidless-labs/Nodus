import './MiniStatus.css';

/**
 * MiniStatus — a dim monospace readout in the bottom-left corner (R15).
 * Nodes / routes count + engine state. Counts are sample values for now;
 * wired to the live graph in R3/R18.
 */
export function MiniStatus({
  nodes,
  routes,
  live,
}: {
  nodes: number;
  routes: number;
  live: boolean;
}) {
  return (
    <div className="mini-status" aria-hidden>
      <span>
        <b>nodes</b> {nodes}
      </span>
      <span>
        <b>routes</b> {routes}
      </span>
      <span>
        <b>engine</b> {live ? 'live' : 'idle'}
      </span>
    </div>
  );
}
