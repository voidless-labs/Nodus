/* graph-data.jsx — type registry, glyphs, devices, and the streamer graph. Converted to ES module. */
import React from 'react';

const h = React.createElement;

const G = (children) => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.6, strokeLinecap: 'round', strokeLinejoin: 'round' }, children);

export const Icons = {
  source: () => G([
    h('circle', { key: 'c', cx: 8, cy: 12, r: 3.2, fill: 'currentColor', stroke: 'none' }),
    h('path', { key: 'a', d: 'M13 12 H20 M17 9 L20 12 L17 15' }),
  ]),
  app: () => G([
    h('rect', { key: 'r', x: 4, y: 4, width: 16, height: 16, rx: 2 }),
    h('path', { key: 'l', d: 'M8 8 H12 M8 12 H14' }),
  ]),
  effect: () => G([
    h('rect', { key: 'r', x: 4, y: 5, width: 16, height: 14, rx: 2 }),
    h('path', { key: 'l1', d: 'M9 9 V15' }),
    h('path', { key: 'l2', d: 'M15 9 V15' }),
  ]),
  limiter: () => G([
    h('rect', { key: 'r', x: 4, y: 5, width: 16, height: 14, rx: 2 }),
    h('path', { key: 'c', d: 'M7 9 H17' }),
    h('path', { key: 'w', d: 'M7 14 Q10 11 12 14 T17 14' }),
  ]),
  eq: () => G([
    h('path', { key: 'a', d: 'M7 5 V19 M12 5 V19 M17 5 V19' }),
    h('circle', { key: 'd1', cx: 7, cy: 9, r: 1.8, fill: 'currentColor', stroke: 'none' }),
    h('circle', { key: 'd2', cx: 12, cy: 14, r: 1.8, fill: 'currentColor', stroke: 'none' }),
    h('circle', { key: 'd3', cx: 17, cy: 10, r: 1.8, fill: 'currentColor', stroke: 'none' }),
  ]),
  gain: () => G([
    h('path', { key: 'r', d: 'M4 18 L20 7' }),
    h('path', { key: 'b', d: 'M4 18 H20' }),
  ]),
  mixer: () => G([
    h('path', { key: 'a', d: 'M4 7 H11' }),
    h('path', { key: 'b', d: 'M4 12 H11' }),
    h('path', { key: 'c', d: 'M4 17 H11' }),
    h('path', { key: 'm', d: 'M11 7 L18 12 L11 17' }),
    h('path', { key: 'o', d: 'M18 12 H20' }),
  ]),
  splitter: () => G([
    h('path', { key: 'i', d: 'M4 12 H11' }),
    h('path', { key: 'u', d: 'M11 12 L18 7 H20' }),
    h('path', { key: 'd', d: 'M11 12 L18 17 H20' }),
    h('circle', { key: 'n', cx: 11, cy: 12, r: 1.6, fill: 'currentColor', stroke: 'none' }),
  ]),
  logic: () => G([
    h('path', { key: 'd', d: 'M12 4 L20 12 L12 20 L4 12 Z' }),
    h('circle', { key: 'c', cx: 12, cy: 12, r: 1.7, fill: 'currentColor', stroke: 'none' }),
  ]),
  trigger: () => G([
    h('circle', { key: 'c', cx: 12, cy: 12, r: 2.4, fill: 'currentColor', stroke: 'none' }),
    h('path', { key: 'a', d: 'M12 4 V7 M12 17 V20 M4 12 H7 M17 12 H20 M6.5 6.5 L8.6 8.6 M15.4 15.4 L17.5 17.5 M17.5 6.5 L15.4 8.6 M8.6 15.4 L6.5 17.5' }),
  ]),
  output: () => G([
    h('rect', { key: 'r', x: 5, y: 5, width: 14, height: 14, rx: 2 }),
    h('path', { key: 'a', d: 'M4 12 H11 M8 9 L11 12 L8 15' }),
  ]),
  headphone: () => G([
    h('path', { key: 'a', d: 'M5 13 V11 a7 7 0 0 1 14 0 V13' }),
    h('rect', { key: 'l', x: 4, y: 13, width: 3.5, height: 6, rx: 1.4, fill: 'currentColor', stroke: 'none' }),
    h('rect', { key: 'r', x: 16.5, y: 13, width: 3.5, height: 6, rx: 1.4, fill: 'currentColor', stroke: 'none' }),
  ]),
};

export const TYPE_META = {
  source:   { cat: 'input',  io: 'in',  device: true, vol: 'volume', glyph: 'source' },
  app:      { cat: 'input',  io: 'in',  device: true, vol: 'volume', glyph: 'app' },
  gate:     { cat: 'effect', glyph: 'effect',  param: 'gate' },
  comp:     { cat: 'effect', glyph: 'effect',  param: 'comp' },
  limiter:  { cat: 'effect', glyph: 'limiter', param: 'limiter' },
  eq:       { cat: 'effect', glyph: 'eq',      param: 'eq' },
  gain:     { cat: 'effect', glyph: 'gain',    vol: 'gain' },
  mixer:    { cat: 'route',  glyph: 'mixer',   perEdge: 'in', addIn: true },
  splitter: { cat: 'route',  glyph: 'splitter',perEdge: 'out', addOut: true },
  duck:     { cat: 'logic',  glyph: 'logic',   param: 'duck' },
  trigger:  { cat: 'logic',  glyph: 'trigger', trigger: true },
  output:   { cat: 'output', io: 'out', device: true, vol: 'volume', addIn: true, glyph: 'output' },
};

export function meta(type) { return TYPE_META[type] || TYPE_META.gate; }
export function isInput(n) { return meta(n.type).io === 'in'; }
export function isOutput(n) { return meta(n.type).io === 'out'; }

export function traceSources(nodes, edges, startId) {
  const found = [], seen = new Set(), stack = [startId];
  while (stack.length) {
    const id = stack.pop(); if (seen.has(id)) continue; seen.add(id);
    const node = nodes[id]; if (!node) continue;
    const ins = edges.filter(e => e.to.node === id && e.to.port !== 'ctrl');
    if (meta(node.type).io === 'in' || ins.length === 0) { if (meta(node.type).io === 'in') found.push(node.name); continue; }
    ins.forEach(e => stack.push(e.from.node));
  }
  return found;
}

export function traceSinks(nodes, edges, startId) {
  const found = [], seen = new Set(), stack = [startId];
  while (stack.length) {
    const id = stack.pop(); if (seen.has(id)) continue; seen.add(id);
    const node = nodes[id]; if (!node) continue;
    const outs = edges.filter(e => e.from.node === id);
    if (meta(node.type).io === 'out' || outs.length === 0) { if (meta(node.type).io === 'out') found.push(node.name); continue; }
    outs.forEach(e => stack.push(e.to.node));
  }
  return found;
}

export function summarizeNames(names) {
  const u = [...new Set(names)];
  if (!u.length) return null;
  if (u.length <= 2) return u.join(' + ');
  return u.slice(0, 2).join(' + ') + ' +' + (u.length - 2);
}

export function channelLabel(nodes, edges, neighborId, dir) {
  const o = nodes[neighborId];
  const busName = o ? o.name : '—';
  if (!o) return { name: busName, sub: null, title: busName };
  const m = meta(o.type);
  if (dir === 'src') {
    const traced = m.io !== 'in' ? summarizeNames(traceSources(nodes, edges, neighborId)) : null;
    return traced ? { name: traced, sub: 'via ' + busName, title: busName + ' → ' + traced } : { name: busName, sub: null, title: busName };
  }
  const traced = m.io !== 'out' ? summarizeNames(traceSinks(nodes, edges, neighborId)) : null;
  return traced ? { name: traced, sub: 'to ' + busName, title: busName + ' → ' + traced } : { name: busName, sub: null, title: busName };
}

export function outputContributions(nodes, edges, outputId) {
  const out = [], seenEdge = new Set();
  const walk = (nodeId, controlEdge) => {
    if (!nodeId || !controlEdge || seenEdge.has(controlEdge.id)) return;
    seenEdge.add(controlEdge.id);
    const node = nodes[nodeId]; if (!node) return;
    if (node.type === 'mixer') {
      edges.filter(e => e.to.node === nodeId && e.to.port !== 'ctrl').forEach(ie => walk(ie.from.node, ie));
    } else {
      out.push({ edgeId: controlEdge.id, ...channelLabel(nodes, edges, nodeId, 'src') });
    }
  };
  edges.filter(e => e.to.node === outputId).forEach(oe => walk(oe.from.node, oe));
  return out;
}

export function portsFor(type) {
  switch (type) {
    case 'source': case 'app':   return { in: [], out: [{ id: 'out', label: 'out' }] };
    case 'output':               return { in: [{ id: 'in1', label: '1' }], out: [] };
    case 'mixer':                return { in: [{ id: 'in1', label: '1' }, { id: 'in2', label: '2' }, { id: 'in3', label: '3' }], out: [{ id: 'out1', label: 'mix' }] };
    case 'splitter':             return { in: [{ id: 'in1', label: 'in' }], out: [{ id: 'out1', label: '1' }, { id: 'out2', label: '2' }, { id: 'out3', label: '3' }] };
    case 'duck':                 return { in: [{ id: 'in', label: 'in' }, { id: 'ctrl', label: 'ctrl' }], out: [{ id: 'out', label: 'out' }] };
    case 'trigger':              return { in: [], out: [{ id: 'ctrl', label: 'ctrl' }] };
    default:                     return { in: [{ id: 'in', label: 'in' }], out: [{ id: 'out', label: 'out' }] };
  }
}

const DEFAULT_PARAMS = {
  gate:     { threshold: -42 },
  comp:     { ratio: 4, threshold: -18 },
  limiter:  { ceiling: -1 },
  duck:     { duck: 30, attack: 12, release: 220 },
  trigger:  { key: 'V', mode: 'hold' },
};

export const DEFAULT_NAME = {
  source: 'Audio Source', app: 'Application', gate: 'Noise Gate', comp: 'Compressor',
  limiter: 'Limiter', eq: 'EQ', gain: 'Gain / Trim', mixer: 'Mixer', splitter: 'Splitter',
  duck: 'Ducking', trigger: 'Push-to-Talk', output: 'Output Device',
};

let _id = 0;
const nid = () => 'n' + (++_id);

export function mk(type, name, x, y, extra) {
  const m = meta(type);
  const p = portsFor(type);
  const node = {
    id: nid(), type, name: name || DEFAULT_NAME[type] || type, x, y,
    glyph: m.glyph, cat: m.cat,
    in: JSON.parse(JSON.stringify(p.in)),
    out: JSON.parse(JSON.stringify(p.out)),
    muted: false, solo: false,
  };
  if (m.vol) node.volume = (type === 'gain' || m.io === 'out' ? 100 : 80);
  if (DEFAULT_PARAMS[type]) node.params = { ...DEFAULT_PARAMS[type] };
  return Object.assign(node, extra);
}

export const VIRTUAL_TEMPLATES = [
  { io: 'out', name: 'Virtual Mic', device: 'NODUS Virtual Cable', kind: 'virtual' },
  { io: 'out', name: 'Virtual Output', device: 'NODUS Virtual Cable', kind: 'virtual' },
];

export const NODE_TEMPLATES = [
  { type: 'gate', cat: 'effect' }, { type: 'comp', cat: 'effect' }, { type: 'limiter', cat: 'effect' },
  { type: 'eq', cat: 'effect' }, { type: 'gain', cat: 'effect' },
  { type: 'mixer', cat: 'route' }, { type: 'splitter', cat: 'route' },
  { type: 'duck', cat: 'logic' }, { type: 'trigger', cat: 'logic' },
].map(t => ({ ...t, name: DEFAULT_NAME[t.type], glyph: meta(t.type).glyph }));

export const NODE_CATEGORIES = [
  { id: 'effect', label: 'Effects' },
  { id: 'route', label: 'Routing' },
  { id: 'logic', label: 'Logic' },
];

// Aliases used in app.jsx
export { mk as NODUS_mk, portsFor as NODUS_portsFor };
