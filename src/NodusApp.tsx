import { Canvas } from './ui/Canvas';
import { NodeCard } from './ui/nodes/NodeCard';
import type { NodeModel } from './ui/nodes/types';

/**
 * NodusApp — root of the redesigned Nodus UI.
 *
 * R4 milestone: sample nodes placed on the canvas so the node card design and
 * its states can be reviewed. The data here is static sample data; the engine
 * wiring (real devices, live meters) is R3, which comes next.
 */
const SAMPLE_NODES: NodeModel[] = [
  {
    id: 'spotify',
    kind: 'source',
    name: 'Spotify',
    subtitle: 'playing · app',
    avatar: 'S',
    level: 0.62,
    volume: 0.55,
    active: true,
    hasInput: false,
    hasOutput: true,
    x: 60,
    y: 110,
  },
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
    x: 60,
    y: 340,
  },
  {
    id: 'game',
    kind: 'source',
    name: 'Cyberpunk',
    subtitle: 'not running',
    avatar: 'C',
    level: 0,
    volume: 0.8,
    running: false,
    hasInput: false,
    hasOutput: true,
    x: 60,
    y: 570,
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
    x: 560,
    y: 110,
  },
  {
    id: 'obs',
    kind: 'output',
    name: 'OBS',
    subtitle: 'stream output · muted',
    level: 0,
    volume: 0.7,
    muted: true,
    hasInput: true,
    hasOutput: false,
    x: 560,
    y: 340,
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
    x: 560,
    y: 570,
  },
];

export default function NodusApp() {
  return (
    <div className="app-shell">
      <Canvas>
        {SAMPLE_NODES.map((n) => (
          <NodeCard key={n.id} node={n} />
        ))}
      </Canvas>
    </div>
  );
}
