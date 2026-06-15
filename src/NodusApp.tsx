import { useState } from 'react';
import { Canvas } from './ui/Canvas';
import { Graph } from './ui/Graph';
import { Topbar } from './ui/Topbar';
import { EngineButton } from './ui/EngineButton';
import { BottomBar } from './ui/BottomBar';
import { ZoomControls } from './ui/ZoomControls';
import { AddPanel } from './ui/AddPanel';
import { EmptyCanvas } from './ui/EmptyCanvas';
import { VirtualDeviceModal } from './ui/VirtualDeviceModal';
import { useBackend } from './useBackend';
import { buildPreset, EMPTY_SCENE, type PresetId, type Scene } from './scenes';

/**
 * NodusApp — root of the redesigned Nodus UI.
 *
 * The scene starts empty (first-run), so the EmptyCanvas onboarding shows;
 * preset cards build a ready graph. Real devices, live meters and the engine
 * are wired in R3.
 */
export default function NodusApp() {
  const backend = useBackend();
  const [scene, setScene] = useState<Scene>(EMPTY_SCENE);
  const [setupOpen, setSetupOpen] = useState(true);
  const [search, setSearch] = useState('');

  const toggleLive = () => backend.setLive(!backend.live);
  const loadPreset = (id: PresetId) => setScene(buildPreset(id));

  const isEmpty = scene.nodes.length === 0 && scene.hubs.length === 0;
  const nodeCount = scene.nodes.length + scene.hubs.length;

  return (
    <div className="app-shell">
      <Topbar />
      <div className="canvas-area">
        <Canvas>
          {isEmpty ? (
            <EmptyCanvas onPreset={loadPreset} />
          ) : (
            <Graph nodes={scene.nodes} edges={scene.edges} hubs={scene.hubs} search={search} />
          )}
        </Canvas>
        <EngineButton live={backend.live} onToggleLive={toggleLive} />
        <ZoomControls />
        <BottomBar onSearch={setSearch} />
        <AddPanel
          nodes={nodeCount}
          routes={scene.edges.length}
          devices={backend.devices}
          processes={backend.processes}
        />
        {setupOpen && <VirtualDeviceModal onClose={() => setSetupOpen(false)} />}
      </div>
    </div>
  );
}
