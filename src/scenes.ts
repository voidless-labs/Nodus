import type { EdgeModel, HubModel, NodeModel } from './ui/nodes/types';

/** A canvas scene: leaf nodes, hub nodes and the edges between them. */
export interface Scene {
  nodes: NodeModel[];
  hubs: HubModel[];
  edges: EdgeModel[];
}

export const EMPTY_SCENE: Scene = { nodes: [], hubs: [], edges: [] };

/** Preset ids offered on the empty canvas (R16). */
export type PresetId = 'stream' | 'discord' | 'headphones';

/** The hero "streaming" scene: sources → Stream Mix → headphones/OBS/Nodus Mic. */
function streamScene(): Scene {
  const nodes: NodeModel[] = [
    { id: 'mic', kind: 'source', name: 'Microphone', subtitle: 'Shure MV7 · hold V', level: 0.34, volume: 0.9, active: true, hasInput: false, hasOutput: true, x: 40, y: 96 },
    { id: 'spotify', kind: 'source', name: 'Spotify', subtitle: 'playing · app', avatar: 'S', level: 0.62, volume: 0.55, active: true, selected: true, hasInput: false, hasOutput: true, x: 40, y: 300 },
    { id: 'game', kind: 'source', name: 'Cyberpunk', subtitle: 'running · app', avatar: 'C', level: 0.5, volume: 0.8, active: true, hasInput: false, hasOutput: true, x: 40, y: 504 },
    { id: 'headphones', kind: 'output', name: 'Headphones', subtitle: 'Galaxy Buds', level: 0.5, volume: 1, active: true, hasInput: true, hasOutput: false, x: 760, y: 96 },
    { id: 'obs', kind: 'output', name: 'OBS', subtitle: 'stream output', level: 0.4, volume: 0.7, active: true, hasInput: true, hasOutput: false, compact: true, x: 760, y: 300 },
    { id: 'nodusmic', kind: 'virtual', micSink: true, name: 'Nodus Mic', subtitle: 'virtual · to Discord', level: 0.4, volume: 0.85, active: true, hasInput: true, hasOutput: false, x: 760, y: 452 },
  ];
  const hubs: HubModel[] = [
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
  const edges: EdgeModel[] = [
    { id: 'e1', from: 'mic', to: 'mix', toPort: 'mic', active: true },
    { id: 'e2', from: 'spotify', to: 'mix', toPort: 'music', active: true },
    { id: 'e3', from: 'game', to: 'mix', toPort: 'game', active: true },
    { id: 'e4', from: 'mix', to: 'headphones', active: true },
    { id: 'e5', from: 'mix', to: 'obs', active: true },
    { id: 'e6', from: 'mix', to: 'nodusmic', active: true },
  ];
  return { nodes, hubs, edges };
}

/** "Music in Discord": Spotify + Microphone → Nodus Mic. */
function discordScene(): Scene {
  const nodes: NodeModel[] = [
    { id: 'spotify', kind: 'source', name: 'Spotify', subtitle: 'playing · app', avatar: 'S', level: 0.6, volume: 0.4, active: true, hasInput: false, hasOutput: true, x: 80, y: 150 },
    { id: 'mic', kind: 'source', name: 'Microphone', subtitle: 'Blue Yeti', level: 0.4, volume: 0.95, active: true, hasInput: false, hasOutput: true, x: 80, y: 360 },
    { id: 'nodusmic', kind: 'virtual', micSink: true, name: 'Nodus Mic', subtitle: 'virtual · to Discord', level: 0.5, volume: 1, active: true, hasInput: true, hasOutput: false, x: 560, y: 250 },
  ];
  const edges: EdgeModel[] = [
    { id: 'e1', from: 'spotify', to: 'nodusmic', active: true },
    { id: 'e2', from: 'mic', to: 'nodusmic', active: true },
  ];
  return { nodes, hubs: [], edges };
}

/** "Everything → headphones": sources straight to the headphones. */
function headphonesScene(): Scene {
  const nodes: NodeModel[] = [
    { id: 'spotify', kind: 'source', name: 'Spotify', subtitle: 'playing · app', avatar: 'S', level: 0.6, volume: 0.7, active: true, hasInput: false, hasOutput: true, x: 80, y: 130 },
    { id: 'game', kind: 'source', name: 'Cyberpunk', subtitle: 'running · app', avatar: 'C', level: 0.5, volume: 0.85, active: true, hasInput: false, hasOutput: true, x: 80, y: 340 },
    { id: 'headphones', kind: 'output', name: 'Headphones', subtitle: 'Galaxy Buds', level: 0.55, volume: 1, active: true, hasInput: true, hasOutput: false, x: 560, y: 235 },
  ];
  const edges: EdgeModel[] = [
    { id: 'e1', from: 'spotify', to: 'headphones', active: true },
    { id: 'e2', from: 'game', to: 'headphones', active: true },
  ];
  return { nodes, hubs: [], edges };
}

export function buildPreset(id: PresetId): Scene {
  switch (id) {
    case 'stream':
      return streamScene();
    case 'discord':
      return discordScene();
    case 'headphones':
      return headphonesScene();
  }
}
