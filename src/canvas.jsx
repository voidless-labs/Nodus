/* canvas.jsx — infinite canvas, edges, ports, node component. Converted to ES module. */
import React from 'react';
import { meta, channelLabel, Icons } from './graph-data.jsx';

const h = React.createElement;
const { useRef, useState, useEffect, useCallback } = React;

export const NODE_W = 200, NODE_W_COMPACT = 178;
const PORT_Y0 = 41.5, PORT_DY = 22;

function portAnchor(node, side, index, compact) {
  const w = compact ? NODE_W_COMPACT : NODE_W;
  const x = side === 'in' ? node.x - 1.5 : node.x + w + 1.5;
  const y = node.y + PORT_Y0 + index * PORT_DY;
  return { x, y };
}

function edgePath(x1, y1, x2, y2, style) {
  if (style === 'straight') return `M ${x1} ${y1} L ${x2} ${y2}`;
  if (style === 'ortho') {
    const mx = (x1 + x2) / 2, r = 8;
    const dir = x2 >= x1 ? 1 : -1, sy = y2 >= y1 ? 1 : -1;
    if (Math.abs(y2 - y1) < 2 || Math.abs(x2 - x1) < 24) return `M ${x1} ${y1} L ${x2} ${y2}`;
    return `M ${x1} ${y1} H ${mx - r * dir} Q ${mx} ${y1} ${mx} ${y1 + r * sy} V ${y2 - r * sy} Q ${mx} ${y2} ${mx + r * dir} ${y2} H ${x2}`;
  }
  const dx = Math.max(40, Math.min(170, Math.abs(x2 - x1) * 0.55));
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

const xIcon = () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2.2, strokeLinecap: 'round' }, h('path', { d: 'M6 6l12 12M18 6L6 18' }));
const plusIcon = () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round' }, h('path', { d: 'M12 5v14M5 12h14' }));
const plusMini = () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 3, strokeLinecap: 'round' }, h('path', { d: 'M12 6v12M6 12h12' }));

/* ============================ PORT ============================ */
function Port({ node, side, port, index, compact, connectedActive, connected, compatible, onDown, onEnter, onLeave }) {
  return h('div', {
    className: 'port' + (compatible ? ' compatible' : ''),
    onMouseDown: (e) => { e.stopPropagation(); onDown(node.id, side, port.id, index); },
    onMouseEnter: () => onEnter(node.id, side, port.id, index),
    onMouseLeave: () => onLeave(),
  }, [
    h('span', { key: 'd', className: 'port-dot' + (connected ? ' connected' : '') + (connectedActive ? ' active' : '') }),
    h('span', { key: 'l', className: 'port-label' }, port.label),
  ]);
}

const stop = (e) => e.stopPropagation();

/* ============================ NODE ============================ */
function NodusNode(props) {
  const { node, selected, active, live, level, ducking, compact, view, nodes, edges,
          onSelect, onMove, onMute, onSolo, onVolume, onEdgeVol, onAddPort, portProps, addSlotProps,
          isMultiSelected, getSelectedPositions, onMoveMany } = props;
  const dragRef = useRef(null);
  const m = meta(node.type);

  const onHeadDown = (e) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    if (e.shiftKey) { onSelect(node.id, true); return; }
    const wasMulti = isMultiSelected;
    if (!wasMulti) onSelect(node.id, false);
    const startX = e.clientX, startY = e.clientY, ox = node.x, oy = node.y;
    if (wasMulti) {
      const snap = getSelectedPositions();
      const move = (ev) => onMoveMany(snap, (ev.clientX - startX) / view.zoom, (ev.clientY - startY) / view.zoom);
      const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    } else {
      const move = (ev) => onMove(node.id, ox + (ev.clientX - startX) / view.zoom, oy + (ev.clientY - startY) / view.zoom);
      const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    }
  };

  const isOut = m.io === 'out', isIn = m.io === 'in';
  const ledClass = node.muted ? '' : (ducking ? 'duck' : (live && active ? 'live' : ''));

  const volMeter = m.vol ? (() => {
    const v = node.volume ?? 80;
    const pct = live && !node.muted && active ? Math.round(level * 100) : 0;
    return h('div', { key: 'vol', className: 'field' }, [
      h('div', { key: 'l', className: 'field-label' }, m.vol === 'gain' ? 'gain' : 'volume'),
      h('div', { key: 'r', className: 'meter-row' }, [
        h('div', { key: 'm', className: 'meter' }, h('div', { className: 'meter-fill', style: { width: pct + '%' } })),
        h('div', { key: 'v', className: 'meter-val' }, node.muted ? 'MUTE' : v + '%'),
      ]),
      h('input', { key: 's', type: 'range', className: 'range-mini', min: 0, max: 100, value: v,
        onMouseDown: stop, onChange: (e) => onVolume(node.id, +e.target.value) }),
    ]);
  })() : null;

  const perEdge = m.perEdge ? (() => {
    const dir = m.perEdge === 'in' ? 'src' : 'sink';
    const rows = m.perEdge === 'in'
      ? edges.filter(e => e.to.node === node.id).map(e => ({ e, nb: e.from.node }))
      : edges.filter(e => e.from.node === node.id).map(e => ({ e, nb: e.to.node }));
    return h('div', { key: 'pe', className: 'vol-list' }, [
      h('div', { key: 'h', className: 'vol-head' }, m.perEdge === 'in' ? `inputs · ${rows.length}` : `outputs · ${rows.length}`),
      rows.length === 0 ? h('div', { key: 'e', className: 'vol-empty' }, m.perEdge === 'in' ? 'no inputs connected' : 'no outputs connected') : null,
      ...rows.map(({ e, nb }) => { const lbl = channelLabel(nodes, edges, nb, dir); return h('div', { key: e.id, className: 'vol-row' }, [
        h('div', { key: 'n', className: 'vol-label', title: lbl.title }, [
          h('span', { key: 'm', className: 'vol-name' }, lbl.name),
          lbl.sub ? h('span', { key: 's', className: 'vol-via' }, lbl.sub) : null,
        ]),
        h('input', { key: 's', type: 'range', className: 'range-mini', min: 0, max: 100, value: e.vol ?? 100,
          onMouseDown: stop, onChange: (ev) => onEdgeVol(e.id, +ev.target.value) }),
        h('div', { key: 'v', className: 'vol-val' }, (e.vol ?? 100) + '%'),
      ]); }),
    ]);
  })() : null;

  const noteFor = () => {
    const p = node.params || {};
    if (node.type === 'gate') return `gate · thr ${p.threshold} dB`;
    if (node.type === 'comp') return `comp · ${p.ratio}:1 · thr ${p.threshold} dB`;
    if (node.type === 'limiter') return `limiter · ceil ${p.ceiling} dB`;
    if (node.type === 'eq') return `parametric eq · 3-band`;
    if (node.type === 'duck') return `duck → ${p.duck}% on ctrl${ducking ? ' · ACTIVE' : ''}`;
    return null;
  };
  const note = noteFor();

  const addSlot = (side) => {
    const { className, ...handlers } = addSlotProps(side, node.id);
    return h('div', { key: 'add-' + side, className: 'port port-add-slot' + (className ? ' ' + className : ''), ...handlers,
      title: side === 'in' ? 'Drag a connection here to add an input' : 'Drag from here to add an output' },
      h('span', { className: 'port-dot add' }, h(plusMini)));
  };

  const trigBody = m.trigger ? h('div', { key: 'trg', className: 'node-trig' + (ducking ? ' live' : '') }, [
    h('span', { key: 's', className: 'nt-state' }, ducking ? 'firing' : 'hold key'),
    h('span', { key: 'k', className: 'nt-key' }, node.params.key),
  ]) : null;

  const showMuteRow = !m.trigger;
  const inCount = node.in.length + (m.addIn ? 1 : 0);
  const outCount = node.out.length + (m.addOut ? 1 : 0);
  const maxPorts = Math.max(inCount, outCount);
  const portsMinHeight = maxPorts > 0 ? PORT_Y0 + (maxPorts - 1) * PORT_DY + 16 : 0;

  return h('div', {
    className: 'node' + (selected ? ' selected' : '') + (node.muted ? ' muted' : '') + (compact ? ' compact' : ''),
    'data-node-id': node.id,
    style: { transform: `translate(${node.x}px, ${node.y}px)`, minHeight: portsMinHeight ? portsMinHeight + 'px' : undefined },
  }, [
    (node.in.length || m.addIn) ? h('div', { key: 'pin', className: 'port-col in' }, [
      ...node.in.map((p, i) => h(Port, { key: p.id, node, side: 'in', port: p, index: i, compact, ...portProps('in', node.id, p.id) })),
      m.addIn ? addSlot('in') : null,
    ]) : null,
    (node.out.length || m.addOut) ? h('div', { key: 'pout', className: 'port-col out' }, [
      ...node.out.map((p, i) => h(Port, { key: p.id, node, side: 'out', port: p, index: i, compact, ...portProps('out', node.id, p.id) })),
      m.addOut ? addSlot('out') : null,
    ]) : null,
    h('div', { key: 'head', className: 'node-head', onMouseDown: onHeadDown }, [
      h('div', { key: 'g', className: 'node-glyph' }, h(Icons[node.glyph] || Icons.source)),
      h('div', { key: 't', className: 'node-titles' }, [
        h('div', { key: 'n', className: 'node-name' }, node.name),
        h('div', { key: 'k', className: 'node-kind' }, node.cat === 'route' ? node.type : node.cat),
      ]),
      h('div', { key: 's', className: 'node-stat' }, h('span', { className: 'node-led ' + ledClass })),
    ]),
    h('div', { key: 'body', className: 'node-body' }, [
      volMeter,
      perEdge,
      trigBody,
      node.device ? h('div', { key: 'dev', className: 'node-note' }, node.device) : null,
      note ? h('div', { key: 'p', className: 'node-note' }, note) : null,
      showMuteRow ? h('div', { key: 'tg', className: 'node-toggles' }, [
        h('div', { key: 'm', className: 'ntog mute' + (node.muted ? ' on' : ''), title: 'Mute', onMouseDown: (e) => { e.stopPropagation(); onMute(node.id); } }, 'M'),
        !isOut ? h('div', { key: 's', className: 'ntog solo' + (node.solo ? ' on' : ''), title: 'Solo', onMouseDown: (e) => { e.stopPropagation(); onSolo(node.id); } }, 'S') : null,
      ]) : null,
    ]),
  ]);
}

/* ============================ CANVAS ============================ */
export function NodusCanvas(props) {
  const { nodes, edges, view, setView, selection, onSelectNode, onSelectEdge,
          clearSelection, onMoveNode, onCreateEdge, live, activeNodes, activeEdges,
          duckingNodes, nodeLevels, tweaks, onMuteNode, onSoloNode, onVolume, onEdgeVol,
          onAddPort, onDropTemplate, onDropDevice, onMarqueeSelect, getSelectedPositions, onMoveMany } = props;

  const vpRef = useRef(null);
  const [pending, setPending] = useState(null);
  const [hoverPort, setHoverPort] = useState(null);
  const [marquee, setMarquee] = useState(null);
  const spaceRef = useRef(false);
  const compact = !!tweaks.compactNodes;
  const selectedIds = selection.ids || [];

  useEffect(() => {
    const kd = (e) => { if (e.code === 'Space') { const tag = (e.target.tagName || '').toLowerCase(); if (tag !== 'input' && tag !== 'textarea') spaceRef.current = true; } };
    const ku = (e) => { if (e.code === 'Space') spaceRef.current = false; };
    window.addEventListener('keydown', kd); window.addEventListener('keyup', ku);
    return () => { window.removeEventListener('keydown', kd); window.removeEventListener('keyup', ku); };
  }, []);

  const toWorld = useCallback((cx, cy) => {
    const r = vpRef.current.getBoundingClientRect();
    return { x: (cx - r.left - view.x) / view.zoom, y: (cy - r.top - view.y) / view.zoom };
  }, [view]);

  const onVpDown = (e) => {
    const t = e.target;
    if (t.closest && t.closest('.node')) return;
    if (e.button === 0 && t.closest && t.closest('.edge-hit')) return;
    const r = vpRef.current.getBoundingClientRect();

    if (e.button === 1 || (e.button === 0 && spaceRef.current)) {
      const sx = e.clientX, sy = e.clientY, ox = view.x, oy = view.y;
      vpRef.current.classList.add('panning');
      const move = (ev) => setView(v => ({ ...v, x: ox + (ev.clientX - sx), y: oy + (ev.clientY - sy) }));
      const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); vpRef.current && vpRef.current.classList.remove('panning'); };
      window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
      return;
    }
    if (e.button !== 0) return;

    const x0 = e.clientX - r.left, y0 = e.clientY - r.top;
    if (!e.shiftKey) clearSelection();
    setMarquee({ x0, y0, x1: x0, y1: y0 });
    const move = (ev) => setMarquee(m => m ? { ...m, x1: ev.clientX - r.left, y1: ev.clientY - r.top } : m);
    const up = (ev) => {
      window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up);
      const rr = vpRef.current.getBoundingClientRect();
      const x1 = ev.clientX - rr.left, y1 = ev.clientY - rr.top;
      const minX = Math.min(x0, x1), maxX = Math.max(x0, x1), minY = Math.min(y0, y1), maxY = Math.max(y0, y1);
      if (maxX - minX < 4 && maxY - minY < 4) { setMarquee(null); return; }
      const ids = [];
      vpRef.current.querySelectorAll('.node[data-node-id]').forEach(el => {
        const b = el.getBoundingClientRect();
        const nx0 = b.left - rr.left, ny0 = b.top - rr.top, nx1 = b.right - rr.left, ny1 = b.bottom - rr.top;
        if (nx0 < maxX && nx1 > minX && ny0 < maxY && ny1 > minY) ids.push(el.getAttribute('data-node-id'));
      });
      onMarqueeSelect(ids);
      setMarquee(null);
    };
    window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
  };

  const onWheel = (e) => {
    e.preventDefault();
    const r = vpRef.current.getBoundingClientRect();
    const cx = e.clientX - r.left, cy = e.clientY - r.top;
    setView(v => {
      const z = Math.max(0.3, Math.min(2.2, v.zoom * Math.exp(-e.deltaY * 0.0015)));
      const k = z / v.zoom;
      return { zoom: z, x: cx - (cx - v.x) * k, y: cy - (cy - v.y) * k };
    });
  };

  const onPortDown = (nodeId, side, portId, index) => {
    if (side === 'in') return;
    const a = portAnchor(nodes[nodeId], 'out', index, compact);
    setPending({ node: nodeId, port: portId, index, sx: a.x, sy: a.y, x: a.x, y: a.y });
  };
  useEffect(() => {
    if (!pending) return;
    const move = (ev) => { const w = toWorld(ev.clientX, ev.clientY); setPending(p => p ? { ...p, x: w.x, y: w.y } : p); };
    const up = () => setPending(p => {
      if (p && hoverPort && hoverPort.side === 'in' && hoverPort.node !== p.node) {
        const fromPort = p.addOut ? onAddPort(p.node, 'out') : p.port;
        const toPort = hoverPort.addPort ? onAddPort(hoverPort.node, 'in') : hoverPort.port;
        if (fromPort && toPort) onCreateEdge({ node: p.node, port: fromPort }, { node: hoverPort.node, port: toPort });
      }
      return null;
    });
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
    return () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
  }, [pending, hoverPort, toWorld, onCreateEdge, onAddPort]);

  const startAddOut = (nodeId) => {
    const node = nodes[nodeId]; if (!node) return;
    const idx = node.out.length;
    const a = portAnchor(node, 'out', idx, compact);
    setPending({ node: nodeId, addOut: true, index: idx, sx: a.x, sy: a.y, x: a.x, y: a.y });
  };
  const addSlotProps = (side, nodeId) => side === 'in'
    ? { className: (pending && pending.node !== nodeId) ? 'compatible' : '', onMouseEnter: () => setHoverPort({ node: nodeId, side: 'in', addPort: true }), onMouseLeave: () => setHoverPort(null) }
    : { className: '', onMouseDown: (e) => { e.stopPropagation(); startAddOut(nodeId); } };

  const portProps = (side, nodeId, portId) => {
    const connected = edges.some(e => (side === 'in' && e.to.node === nodeId && e.to.port === portId) || (side === 'out' && e.from.node === nodeId && e.from.port === portId));
    const connectedActive = live && connected && edges.some(e => activeEdges.has(e.id) && ((side === 'in' && e.to.node === nodeId && e.to.port === portId) || (side === 'out' && e.from.node === nodeId && e.from.port === portId)));
    const compatible = !!pending && side === 'in' && pending.node !== nodeId;
    return { connected, connectedActive, compatible, onDown: onPortDown,
      onEnter: (nId, s, pId, idx) => setHoverPort({ node: nId, side: s, port: pId, index: idx }),
      onLeave: () => setHoverPort(null) };
  };

  const gridStyle = (() => {
    if (tweaks.grid === 'off') return { background: 'transparent' };
    const sz = 40 * view.zoom;
    const ox = ((view.x % sz) + sz) % sz, oy = ((view.y % sz) + sz) % sz;
    const bpos = `${ox}px ${oy}px`, bsize = `${sz}px ${sz}px`;
    if (tweaks.grid === 'dots') return {
      inset: 0,
      backgroundImage: `radial-gradient(var(--grid) 1px, transparent 1px)`,
      backgroundSize: bsize, backgroundPosition: bpos,
    };
    return {
      inset: 0,
      backgroundImage: `linear-gradient(var(--grid) 1px, transparent 1px), linear-gradient(90deg, var(--grid) 1px, transparent 1px)`,
      backgroundSize: bsize, backgroundPosition: bpos,
    };
  })();
  const majorStyle = (() => {
    if (tweaks.grid === 'off') return { display: 'none' };
    const sz = 200 * view.zoom;
    const ox = ((view.x % sz) + sz) % sz, oy = ((view.y % sz) + sz) % sz;
    return {
      inset: 0,
      backgroundImage: `linear-gradient(rgba(255,255,255,0.04) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.04) 1px, transparent 1px)`,
      backgroundSize: `${sz}px ${sz}px`, backgroundPosition: `${ox}px ${oy}px`,
    };
  })();

  const onDragOver = (e) => {
    if (e.dataTransfer.types.includes('application/nodus-node') || e.dataTransfer.types.includes('application/nodus-device')) {
      e.preventDefault(); e.dataTransfer.dropEffect = 'copy';
    }
  };
  const onDrop = (e) => {
    const dev = e.dataTransfer.getData('application/nodus-device');
    const type = e.dataTransfer.getData('application/nodus-node');
    const w = toWorld(e.clientX, e.clientY);
    if (dev) { e.preventDefault(); const d = JSON.parse(dev); onDropDevice(d, w.x - NODE_W / 2, w.y - 30); return; }
    if (type) { e.preventDefault(); onDropTemplate(type, w.x - NODE_W / 2, w.y - 30); }
  };

  const edgeEls = edges.map(edge => {
    const fn = nodes[edge.from.node], tn = nodes[edge.to.node];
    if (!fn || !tn) return null;
    const fi = fn.out.findIndex(p => p.id === edge.from.port);
    const ti = tn.in.findIndex(p => p.id === edge.to.port);
    if (fi < 0 || ti < 0) return null;
    const a = portAnchor(fn, 'out', fi, compact), b = portAnchor(tn, 'in', ti, compact);
    const d = edgePath(a.x, a.y, b.x, b.y, tweaks.curve);
    const isActive = live && activeEdges.has(edge.id) && tweaks.showActivity;
    const muted = fn.muted || tn.muted;
    const sel = selection.edgeId === edge.id;
    return h('g', { key: edge.id, className: 'edge' + (isActive ? ' active' : '') + (sel ? ' selected' : '') + (muted ? ' muted' : ''), onClick: (e) => { e.stopPropagation(); onSelectEdge(edge.id); } }, [
      h('path', { key: 'hit', className: 'edge-hit', d }),
      h('path', { key: 'p', className: 'edge-path', d }),
      isActive ? h('path', { key: 'f', className: 'edge-flow', d }) : null,
    ]);
  });

  let tempEdge = null;
  if (pending) tempEdge = h('path', { className: 'edge-temp', d: edgePath(pending.sx, pending.sy, pending.x, pending.y, tweaks.curve) });

  return h('div', { ref: vpRef, className: 'canvas-vp' + (pending ? ' connecting' : ''), onMouseDown: onVpDown, onWheel, onDragOver, onDrop }, [
    h('div', { key: 'grid', className: 'canvas-grid', style: gridStyle }),
    h('div', { key: 'major', className: 'canvas-grid', style: majorStyle }),
    // Two-layer zoom: outer div handles translate (screen-space), inner handles CSS zoom.
    // CSS zoom re-renders content at proper DPI instead of scaling a composited bitmap.
    // The coordinate system is identical — portAnchor and toWorld math unchanged.
    h('div', { key: 'world', className: 'canvas-world', style: { transform: `translate(${view.x}px, ${view.y}px)` } }, [
      h('div', { key: 'zoom', style: { zoom: view.zoom, position: 'absolute', top: 0, left: 0 } }, [
        h('svg', { key: 'edges', className: 'edges-svg', width: 1, height: 1 }, [...edgeEls, tempEdge]),
        ...Object.values(nodes).map(n => h(NodusNode, {
          key: n.id, node: n, compact, view, nodes, edges,
          selected: selection.nodeId === n.id || selectedIds.includes(n.id),
          active: activeNodes.has(n.id),
          live, level: nodeLevels[n.id] || 0, ducking: duckingNodes.has(n.id),
          onSelect: onSelectNode, onMove: onMoveNode, onMute: onMuteNode, onSolo: onSoloNode,
          onVolume, onEdgeVol, onAddPort, portProps, addSlotProps,
          isMultiSelected: selectedIds.length > 1 && selectedIds.includes(n.id),
          getSelectedPositions, onMoveMany,
        })),
      ]),
    ]),
    marquee ? h('div', { key: 'marq', className: 'marquee', style: { left: Math.min(marquee.x0, marquee.x1), top: Math.min(marquee.y0, marquee.y1), width: Math.abs(marquee.x1 - marquee.x0), height: Math.abs(marquee.y1 - marquee.y0) } }) : null,
    tweaks.grain ? h('div', { key: 'grain', className: 'canvas-grain' }) : null,
  ]);
}
