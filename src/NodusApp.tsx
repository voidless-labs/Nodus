import { Canvas } from './ui/Canvas';

/**
 * NodusApp — root of the redesigned Nodus UI.
 *
 * Named NodusApp (not App) on purpose: the legacy orchestrator is `app.jsx`,
 * and Windows' case-insensitive filesystem would resolve `./App` to it.
 *
 * R2 milestone: just the canvas backdrop. The placeholder note below is a
 * temporary scaffold marker so the foundation is visible in preview; it is
 * removed once the first real chrome (topbar) lands in R10.
 */
export default function NodusApp() {
  return (
    <div className="app-shell">
      <Canvas>
        <div className="scaffold-note">
          Nodus — redesign foundation
          <span>canvas · dot grid · vignette · amber accent</span>
        </div>
      </Canvas>
    </div>
  );
}
