import type { AudioDevice, AudioProcess } from './bridge';
import type { EdgeModel, HubModel, NodeModel } from './ui/nodes/types';

/** A canvas scene: leaf nodes, hub nodes and the edges between them. */
export interface Scene {
  nodes: NodeModel[];
  hubs: HubModel[];
  edges: EdgeModel[];
  /** Ids of nodes/hubs pinned to the quick-controls popup (t13), per scene. */
  pinned?: string[];
}

export const EMPTY_SCENE: Scene = { nodes: [], hubs: [], edges: [], pinned: [] };

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
      name: 'Mixer',
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

/** Real devices/processes to bind a preset's placeholder nodes onto (R18). */
export interface PresetBinding {
  output?: AudioDevice;
  input?: AudioDevice;
  virtualMic?: AudioDevice;
  processes?: AudioProcess[];
}

const subFor = (d: AudioDevice) => d.original_name ?? d.name;

/**
 * bindScene — map a preset's curated placeholder nodes onto the user's real
 * hardware so the graph actually routes (R18):
 * - output nodes → the default (or first) output device;
 * - the mic source (a source with no app avatar) → the default input device;
 * - the Nodus virtual-mic sink → the user's own Nodus virtual device;
 * - app sources (with an avatar) → a running process matched by name; if the
 *   app isn't running the node stays a friendly placeholder to rebind later.
 * The curated display name is kept; the real binding rides on
 * deviceId/exeName/icon (+ a subtitle showing what it bound to). Pure.
 */
export function bindScene(scene: Scene, b: PresetBinding): Scene {
  const matchProc = (name: string): AudioProcess | undefined => {
    const key = name.toLowerCase();
    return (b.processes ?? []).find(
      (p) =>
        p.display_name.toLowerCase().includes(key) || p.exe_name.toLowerCase().includes(key),
    );
  };

  const nodes = scene.nodes.map((n): NodeModel => {
    if (n.kind === 'output' && b.output) {
      return { ...n, deviceId: b.output.id, subtitle: subFor(b.output) };
    }
    if (n.kind === 'virtual' && n.micSink && b.virtualMic) {
      return { ...n, deviceId: b.virtualMic.id, subtitle: subFor(b.virtualMic) };
    }
    if (n.kind === 'source') {
      if (n.avatar) {
        const p = matchProc(n.name);
        return p
          ? { ...n, exeName: p.exe_name, icon: p.icon ?? n.icon, subtitle: `${p.source_type} · app` }
          : n; // app not running → keep placeholder
      }
      // mic-like source (no avatar) → bind to the input device
      if (b.input) return { ...n, deviceId: b.input.id, subtitle: subFor(b.input) };
    }
    return n;
  });

  return { ...scene, nodes };
}
