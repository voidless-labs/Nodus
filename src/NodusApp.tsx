import { useState } from 'react';
import { Canvas } from './ui/Canvas';
import { Graph } from './ui/Graph';
import { Topbar } from './ui/Topbar';
import { EngineButton } from './ui/EngineButton';
import { BottomBar } from './ui/BottomBar';
import { ZoomControls } from './ui/ZoomControls';
import { AddPanel } from './ui/AddPanel';
import type { EdgeModel, HubModel, NodeModel } from './ui/nodes/types';

/**
 * NodusApp — root of the redesigned Nodus UI.
 *
 * Sample data lays out the hero scene (sources → Stream Mix hub → outputs) so
 * the node types and states can be reviewed. Real devices, live meters and the
 * engine are wired in R3.
 */
const SAMPLE_NODES: NodeModel[] = [
  {
    id: 'mic',
    kind: 'source',
    name: 'Microphone',
    subtitle: 'Shure MV7 · hold V',
    level: 0.34,
    volume: 0.9,
    active: true,
    hasInput: false,
    hasOutput: true,
    x: 40,
    y: 96,
  },
  {
    id: 'spotify',
    kind: 'source',
    name: 'Spotify',
    subtitle: 'playing · app',
    avatar: 'S',
    level: 0.62,
    volume: 0.55,
    active: true,
    selected: true,
    hasInput: false,
    hasOutput: true,
    x: 40,
    y: 300,
  },
  {
    id: 'game',
    kind: 'source',
    name: 'Cyberpunk',
    subtitle: 'running · app',
    avatar: 'C',
    level: 0.5,
    volume: 0.8,
    active: true,
    hasInput: false,
    hasOutput: true,
    x: 40,
    y: 504,
  },
  {
    id: 'headphones',
    kind: 'output',
    name: 'Headphones',
    subtitle: 'Galaxy Buds',
    level: 0.5,
    volume: 1,
    active: true,
    hasInput: true,
    hasOutput: false,
    x: 760,
    y: 96,
  },
  {
    id: 'obs',
    kind: 'output',
    name: 'OBS',
    subtitle: 'stream output',
    level: 0.4,
    volume: 0.7,
    active: true,
    hasInput: true,
    hasOutput: false,
    compact: true,
    x: 760,
    y: 300,
  },
  {
    id: 'nodusmic',
    kind: 'virtual',
    micSink: true,
    name: 'Nodus Mic',
    subtitle: 'virtual · to Discord',
    level: 0.4,
    volume: 0.85,
    active: true,
    hasInput: true,
    hasOutput: false,
    x: 760,
    y: 452,
  },
];

const SAMPLE_HUBS: HubModel[] = [
  {
    id: 'mix',
    name: 'Stream Mix',
    subtitle: 'routing engine',
    inputs: [
      { id: 'mic', label: 'mic', volume: 0.92 },
      { id: 'music', label: 'music', volume: 0.55 },
      { id: 'game', label: 'game', volume: 0.8 },
    ],
    level: 0.7,
    active: true,
    x: 400,
    y: 250,
  },
];

const SAMPLE_EDGES: EdgeModel[] = [
  { id: 'e1', from: 'mic', to: 'mix', toPort: 'mic', active: true },
  { id: 'e2', from: 'spotify', to: 'mix', toPort: 'music', active: true },
  { id: 'e3', from: 'game', to: 'mix', toPort: 'game', active: true },
  { id: 'e4', from: 'mix', to: 'headphones', active: true },
  { id: 'e5', from: 'mix', to: 'obs', active: true },
  { id: 'e6', from: 'mix', to: 'nodusmic', active: true },
];

export default function NodusApp() {
  const [live, setLive] = useState(false);
  const toggleLive = () => setLive((v) => !v);

  return (
    <div className="app-shell">
      <Topbar />
      <div className="canvas-area">
        <Canvas>
          <Graph nodes={SAMPLE_NODES} edges={SAMPLE_EDGES} hubs={SAMPLE_HUBS} />
        </Canvas>
        <EngineButton live={live} onToggleLive={toggleLive} />
        <ZoomControls />
        <BottomBar />
        <AddPanel nodes={SAMPLE_NODES.length + SAMPLE_HUBS.length} routes={SAMPLE_EDGES.length} />
      </div>
    </div>
  );
}
