/* app.jsx — Nodus orchestrator (v2): scenes, signal sim, dock, settings, I/O. */

const { useState, useEffect, useRef, useMemo, useCallback } = React;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "accent": "#5B8FCB",
  "curve": "bezier",
  "grid": "lines",
  "showActivity": true,
  "grain": true,
  "compactNodes": false
}/*EDITMODE-END*/;

function hexA(hex, a) { const n = parseInt(hex.slice(1), 16); return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`; }

let _gid = 5000;
const gid = (p) => p + (++_gid);

function freshStreamer() {
  const s = JSON.parse(JSON.stringify(STREAMER_SCENE));
  return { id: gid('s'), name: 'Streamer Setup', nodes: s.nodes, edges: s.edges, pinned: s.pinned || [], view: { x: 60, y: 40, zoom: 0.78 } };
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [scenes, setScenes] = useState(() => [freshStreamer()]);
  const [activeSceneId, setActiveSceneId] = useState(null);
  const [selection, setSelection] = useState({ nodeId: null, edgeId: null, ids: [] });
  const [live, setLive] = useState(true);
  const [ptt, setPtt] = useState(false);
  const [levels, setLevels] = useState({});
  const [ui, setUi] = useState({ lib: true, insp: false, dock: false, inspAuto: false });
  const canvasRegionRef = useRef(null);
  const fileRef = useRef(null);

  // resolve active scene
  const curId = activeSceneId || (scenes[0] && scenes[0].id);
  const scene = scenes.find(s => s.id === curId) || scenes[0];
  const nodes = scene.nodes, edges = scene.edges, view = scene.view;

  /* ---- scene mutators ---- */
  const patchScene = useCallback((fn) => setScenes(ss => ss.map(s => s.id === (activeSceneId || ss[0].id) ? { ...s, ...fn(s) } : s)), [activeSceneId]);
  const setNodes = useCallback((upd) => patchScene(s => ({ nodes: typeof upd === 'function' ? upd(s.nodes) : upd })), [patchScene]);
  const setEdges = useCallback((upd) => patchScene(s => ({ edges: typeof upd === 'function' ? upd(s.edges) : upd })), [patchScene]);
  const setView = useCallback((upd) => patchScene(s => ({ view: typeof upd === 'function' ? upd(s.view) : upd })), [patchScene]);

  /* ---- accent ---- */
  useEffect(() => {
    const r = document.documentElement;
    r.style.setProperty('--accent', t.accent);
    r.style.setProperty('--accent-soft', hexA(t.accent, 0.16));
    r.style.setProperty('--accent-line', hexA(t.accent, 0.55));
  }, [t.accent]);

  /* ---- signal propagation ---- */
  const anySolo = useMemo(() => Object.values(nodes).some(n => n.solo), [nodes]);
  const effMuted = useCallback((n) => n.muted || (anySolo && !n.solo && n.type !== 'trigger'), [anySolo]);

  const { activeNodes, activeEdges } = useMemo(() => {
    const aN = new Set(), aE = new Set();
    if (!live) return { activeNodes: aN, activeEdges: aE };
    Object.values(nodes).forEach(n => { if (n.in.length === 0 && n.type !== 'trigger' && !effMuted(n)) aN.add(n.id); });
    let changed = true, guard = 0;
    while (changed && guard++ < 300) {
      changed = false;
      edges.forEach(e => {
        const from = nodes[e.from.node], to = nodes[e.to.node];
        if (!from || !to) return;
        if (aN.has(from.id) && !effMuted(from)) {
          if (!aE.has(e.id)) { aE.add(e.id); changed = true; }
          if (!effMuted(to) && !aN.has(to.id)) { aN.add(to.id); changed = true; }
        }
      });
    }
    return { activeNodes: aN, activeEdges: aE };
  }, [nodes, edges, live, effMuted]);

  const duckingNodes = useMemo(() => {
    const s = new Set();
    if (!ptt) return s;
    edges.forEach(e => {
      const from = nodes[e.from.node], to = nodes[e.to.node];
      if (from && to && from.type === 'trigger' && to.type === 'duck' && e.to.port === 'ctrl') { s.add(to.id); s.add(from.id); }
    });
    return s;
  }, [ptt, edges, nodes]);

  useEffect(() => {
    if (!live) { setLevels({}); return; }
    const iv = setInterval(() => {
      setLevels(prev => {
        const next = {};
        activeNodes.forEach(id => { const target = 0.3 + Math.random() * 0.7; next[id] = (prev[id] ?? 0.5) + (target - (prev[id] ?? 0.5)) * 0.5; });
        return next;
      });
    }, 110);
    return () => clearInterval(iv);
  }, [live, activeNodes]);

  /* ---- node ops ---- */
  // open inspector on selection; remember if WE auto-opened it (so we can auto-close)
  const openInspForSelection = () => setUi(u => u.insp ? u : { ...u, insp: true, inspAuto: true });
  // collapse the inspector again only if it was auto-opened by a selection
  const collapseIfAuto = () => setUi(u => u.inspAuto ? { ...u, insp: false, inspAuto: false } : u);

  const selectNode = (id, additive) => { openInspForSelection(); setSelection(s => {
    if (additive) { const ids = (s.ids || []); const has = ids.includes(id); const next = has ? ids.filter(x => x !== id) : [...ids, id]; return { nodeId: next.length === 1 ? next[0] : null, edgeId: null, ids: next }; }
    return { nodeId: id, edgeId: null, ids: [id] };
  }); };
  const selectEdge = (id) => { openInspForSelection(); setSelection({ nodeId: null, edgeId: id, ids: [] }); };
  const clearSelection = () => { collapseIfAuto(); setSelection({ nodeId: null, edgeId: null, ids: [] }); };
  const marqueeSelect = (ids) => { if (ids.length) openInspForSelection(); else collapseIfAuto(); setSelection({ nodeId: ids.length === 1 ? ids[0] : null, edgeId: null, ids }); };

  const moveNode = useCallback((id, x, y) => setNodes(n => ({ ...n, [id]: { ...n[id], x, y } })), [setNodes]);
  const muteNode = (id) => setNodes(n => ({ ...n, [id]: { ...n[id], muted: !n[id].muted } }));
  const soloNode = (id) => setNodes(n => ({ ...n, [id]: { ...n[id], solo: !n[id].solo } }));
  const setVolume = (id, v) => setNodes(n => ({ ...n, [id]: { ...n[id], volume: v } }));
  const rename = (id, name) => setNodes(n => ({ ...n, [id]: { ...n[id], name } }));
  const setParam = (id, k, v) => setNodes(n => ({ ...n, [id]: { ...n[id], params: { ...n[id].params, [k]: v } } }));
  const setEdgeVol = (id, v) => setEdges(es => es.map(e => e.id === id ? { ...e, vol: v } : e));

  const deleteNode = (id) => { setNodes(n => { const c = { ...n }; delete c[id]; return c; }); setEdges(es => es.filter(e => e.from.node !== id && e.to.node !== id)); setPinnedArr(p => p.filter(x => x !== id)); collapseIfAuto(); setSelection({ nodeId: null, edgeId: null, ids: [] }); };
  const deleteEdge = (id) => { setEdges(es => es.filter(e => e.id !== id)); collapseIfAuto(); setSelection({ nodeId: null, edgeId: null, ids: [] }); };

  /* ---- bulk (marquee) ops ---- */
  const getSelectedPositions = () => { const m = {}; (selection.ids || []).forEach(id => { if (nodes[id]) m[id] = { x: nodes[id].x, y: nodes[id].y }; }); return m; };
  const moveMany = (snapshot, dx, dy) => setNodes(n => { const c = { ...n }; Object.keys(snapshot).forEach(id => { if (c[id]) c[id] = { ...c[id], x: snapshot[id].x + dx, y: snapshot[id].y + dy }; }); return c; });
  const deleteSelection = () => {
    const ids = new Set(selection.ids || []);
    if (!ids.size) return;
    setNodes(n => { const c = { ...n }; ids.forEach(id => delete c[id]); return c; });
    setEdges(es => es.filter(e => !ids.has(e.from.node) && !ids.has(e.to.node)));
    setPinnedArr(p => p.filter(x => !ids.has(x)));
    collapseIfAuto();
    setSelection({ nodeId: null, edgeId: null, ids: [] });
  };
  const muteSelection = (val) => { const ids = new Set(selection.ids || []); setNodes(n => { const c = { ...n }; ids.forEach(id => { if (c[id] && c[id].type !== 'trigger') c[id] = { ...c[id], muted: val }; }); return c; }); };

  const duplicateNode = (id) => setNodes(n => {
    const src = n[id]; if (!src) return n;
    const nid = gid('n');
    const copy = { ...JSON.parse(JSON.stringify(src)), id: nid, x: src.x + 36, y: src.y + 36, name: src.name + ' copy' };
    setSelection({ nodeId: nid, edgeId: null });
    return { ...n, [nid]: copy };
  });

  const createEdge = useCallback((from, to) => setEdges(es => {
    // one edge per input port AND one edge per output port — to fan out, use a Splitter
    const filtered = es.filter(e => !(e.to.node === to.node && e.to.port === to.port) && !(e.from.node === from.node && e.from.port === from.port));
    return [...filtered, { id: gid('e'), from, to, vol: 100 }];
  }), [setEdges]);

  /* ---- dynamic ports ---- */
  const addPort = (id, side) => {
    const node = nodes[id]; if (!node) return null;
    const arr = side === 'in' ? node.in : node.out;
    const nums = arr.map(p => parseInt((p.id.match(/\d+$/) || [arr.length])[0], 10)).filter(x => !isNaN(x));
    const k = (nums.length ? Math.max(...nums) : arr.length) + 1;
    const pid = side + k;
    setNodes(n => { const nd = n[id]; if (!nd) return n; const a = side === 'in' ? nd.in : nd.out; return { ...n, [id]: { ...nd, [side]: [...a, { id: pid, label: String(k) }] } }; });
    return pid;
  };
  const removePort = (id, side, portId) => {
    setNodes(n => { const node = n[id]; if (!node) return n; const arr = side === 'in' ? node.in : node.out; if (arr.length <= 1) return n; return { ...n, [id]: { ...node, [side]: arr.filter(p => p.id !== portId) } }; });
    setEdges(es => es.filter(e => !((side === 'in' && e.to.node === id && e.to.port === portId) || (side === 'out' && e.from.node === id && e.from.port === portId))));
  };

  /* ---- drops ---- */
  const dropTemplate = useCallback((type, x, y) => {
    const node = NODUS_mk(type, null, x, y); node.id = gid('n');
    setNodes(n => ({ ...n, [node.id]: node }));
    setSelection({ nodeId: node.id, edgeId: null });
  }, [setNodes]);

  const dropDevice = useCallback((d, x, y) => {
    const type = d.io === 'out' ? 'output' : (d.kind === 'app' ? 'app' : 'source');
    const node = NODUS_mk(type, d.name, x, y, { device: d.device });
    if (d.io === 'out' && /head/i.test(d.name)) node.glyph = 'headphone';
    node.id = gid('n');
    setNodes(n => ({ ...n, [node.id]: node }));
    setSelection({ nodeId: node.id, edgeId: null });
    return node.id;
  }, [setNodes]);

  /* ---- quick-control dock (pinned nodes) ---- */
  const setPinnedArr = useCallback((upd) => patchScene(s => ({ pinned: typeof upd === 'function' ? upd(s.pinned || []) : upd })), [patchScene]);
  const pinNode = (id) => setPinnedArr(p => p.includes(id) ? p : [...p, id]);
  const unpinNode = (id) => setPinnedArr(p => p.filter(x => x !== id));
  const togglePin = (id) => setPinnedArr(p => { const has = p.includes(id); if (!has) setUi(u => u.dock ? u : { ...u, dock: true }); return has ? p.filter(x => x !== id) : [...p, id]; });
  const pinnedNodes = (scene.pinned || []).map(id => nodes[id]).filter(Boolean);

  const focusNode = (id) => {
    const n = nodes[id]; if (!n) return;
    selectNode(id);
    const reg = canvasRegionRef.current; if (!reg) return;
    setView(v => ({ ...v, x: reg.clientWidth / 2 - (n.x + NODE_W / 2) * v.zoom, y: reg.clientHeight / 2 - (n.y + 60) * v.zoom }));
  };

  /* ---- scenes ---- */
  const switchScene = (id) => { setActiveSceneId(id); setSelection({ nodeId: null, edgeId: null }); };
  const newScene = () => { const s = { id: gid('s'), name: 'Scene ' + (scenes.length + 1), nodes: {}, edges: [], pinned: [], view: { x: 80, y: 60, zoom: 1 } }; setScenes(ss => [...ss, s]); setActiveSceneId(s.id); setSelection({ nodeId: null, edgeId: null }); };
  const closeScene = (id) => setScenes(ss => {
    if (ss.length <= 1) return ss;
    const idx = ss.findIndex(s => s.id === id);
    const next = ss.filter(s => s.id !== id);
    if ((activeSceneId || ss[0].id) === id) setActiveSceneId(next[Math.max(0, idx - 1)].id);
    return next;
  });
  const renameScene = (id, name) => setScenes(ss => ss.map(s => s.id === id ? { ...s, name } : s));

  /* ---- import / export ---- */
  const doExport = () => {
    const data = JSON.stringify({ app: 'nodus', version: 2, scenes }, null, 2);
    const blob = new Blob([data], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob); a.download = 'nodus-project.json';
    document.body.appendChild(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  };
  const doImport = () => fileRef.current && fileRef.current.click();
  const onFile = (e) => {
    const f = e.target.files[0]; if (!f) return;
    const r = new FileReader();
    r.onload = () => { try { const d = JSON.parse(r.result); if (d.scenes && d.scenes.length) { setScenes(d.scenes); setActiveSceneId(d.scenes[0].id); setSelection({ nodeId: null, edgeId: null }); } } catch (err) {} };
    r.readAsText(f); e.target.value = '';
  };

  /* ---- keyboard ---- */
  useEffect(() => {
    const pttKey = (Object.values(nodes).find(n => n.type === 'trigger')?.params.key || 'V').toLowerCase();
    const down = (e) => {
      const tag = (e.target.tagName || '').toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      if (e.key.toLowerCase() === pttKey && !e.repeat) setPtt(true);
      if (e.key === 'Delete' || e.key === 'Backspace') { if ((selection.ids || []).length > 1) deleteSelection(); else if (selection.edgeId) deleteEdge(selection.edgeId); else if (selection.nodeId) deleteNode(selection.nodeId); }
      if (e.key === ' ') { e.preventDefault(); setLive(l => !l); }
    };
    const up = (e) => { if (e.key.toLowerCase() === pttKey) setPtt(false); };
    window.addEventListener('keydown', down); window.addEventListener('keyup', up);
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up); };
  }, [nodes, selection]); // eslint-disable-line

  /* ---- fit view ---- */
  const fitView = useCallback(() => {
    const ns = Object.values(nodes); const reg = canvasRegionRef.current; if (!reg) return;
    if (!ns.length) { setView({ x: 80, y: 60, zoom: 1 }); return; }
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    ns.forEach(n => { minX = Math.min(minX, n.x); minY = Math.min(minY, n.y); maxX = Math.max(maxX, n.x + NODE_W); maxY = Math.max(maxY, n.y + 240); });
    const vw = reg.clientWidth, vh = reg.clientHeight, pad = 80;
    const zoom = Math.max(0.3, Math.min(1.3, Math.min((vw - pad * 2) / (maxX - minX), (vh - pad * 2) / (maxY - minY))));
    setView({ zoom, x: (vw - (maxX - minX) * zoom) / 2 - minX * zoom, y: (vh - (maxY - minY) * zoom) / 2 - minY * zoom });
  }, [nodes, setView]);

  useEffect(() => { const id = setTimeout(fitView, 80); return () => clearTimeout(id); }, [curId]); // eslint-disable-line

  const selNode = selection.nodeId ? nodes[selection.nodeId] : null;
  const selEdge = selection.edgeId ? edges.find(e => e.id === selection.edgeId) : null;
  const multiNodes = (selection.ids || []).length > 1 ? selection.ids.map(id => nodes[id]).filter(Boolean) : null;

  const appStyle = {
    '--lib-w': ui.lib ? '234px' : '40px',
    '--insp-w': ui.insp ? '314px' : '40px',
    '--dock-h': ui.dock ? '194px' : '30px',
  };

  return h('div', { className: 'app', style: appStyle }, [
    h('div', { key: 'top', className: 'region-top' },
      h(Toolbar, { live, onToggleLive: () => setLive(l => !l), scenes, activeId: curId, onSwitchScene: switchScene, onNewScene: newScene, onCloseScene: closeScene, onRenameScene: renameScene, t, setTweak, onImport: doImport, onExport: doExport })),

    h('div', { key: 'lib', className: 'region-lib' },
      ui.lib ? h(Library, { onCollapse: () => setUi(u => ({ ...u, lib: false })) })
             : h(CollapseRail, { side: 'left', label: 'Library', onExpand: () => setUi(u => ({ ...u, lib: true })) })),

    h('div', { key: 'canvas', className: 'region-canvas', ref: canvasRegionRef }, [
      h(NodusCanvas, {
        key: 'cv', nodes, edges, view, setView, selection,
        onSelectNode: selectNode, onSelectEdge: selectEdge, clearSelection,
        onMoveNode: moveNode, onCreateEdge: createEdge, onDropTemplate: dropTemplate, onDropDevice: dropDevice,
        live, activeNodes, activeEdges, duckingNodes, nodeLevels: levels, tweaks: t,
        onMuteNode: muteNode, onSoloNode: soloNode, onVolume: setVolume, onEdgeVol: setEdgeVol, onAddPort: addPort,
        onMarqueeSelect: marqueeSelect, getSelectedPositions, onMoveMany: moveMany,
      }),
      h('div', { key: 'fab', className: 'canvas-fab' },
        h('div', { className: 'fab-grp' }, [
          h('button', { key: 'f', onClick: fitView, title: 'Fit to view' }, h(UI.fit)),
          h('button', { key: 'c', onClick: () => setView(v => ({ ...v, zoom: 1 })), title: 'Reset zoom' }, h(UI.center)),
        ])),
    ]),

    h('div', { key: 'insp', className: 'region-insp' },
      ui.insp ? h(Inspector, {
        node: selNode, edge: selEdge, multi: multiNodes, nodes, edges, onCollapse: () => setUi(u => ({ ...u, insp: false, inspAuto: false })),
        onRename: rename, onVolume: setVolume, onParam: setParam, onMute: muteNode, onSolo: soloNode,
        onDuplicate: duplicateNode, onDelete: deleteNode, onDeleteEdge: deleteEdge, onEdgeVol: setEdgeVol,
        onSelectNode: selectNode, onAddPort: addPort, onRemovePort: removePort,
        onDeleteSelection: deleteSelection, onMuteSelection: muteSelection,
        isPinned: selNode ? (scene.pinned || []).includes(selNode.id) : false, onTogglePin: togglePin,
      }) : h(CollapseRail, { side: 'right', label: 'Inspector', onExpand: () => setUi(u => ({ ...u, insp: true, inspAuto: false })) })),

    h('div', { key: 'dock', className: 'region-dock' },
      h(BottomDock, {
        open: ui.dock, onToggle: () => setUi(u => ({ ...u, dock: !u.dock })),
        pinned: pinnedNodes, nodes, edges, allNodes: Object.values(nodes),
        onUnpin: unpinNode, onPin: pinNode, onFocus: focusNode,
        onEdgeVol: setEdgeVol, onVolume: setVolume, onMute: muteNode, onSolo: soloNode,
      })),

    h('div', { key: 'status', className: 'region-status' },
      h(StatusBar, { view, setView, onFit: fitView, nodeCount: Object.keys(nodes).length, edgeCount: edges.length, live, activeCount: activeEdges.size, pttActive: ptt })),

    h('input', { key: 'file', ref: fileRef, type: 'file', accept: 'application/json,.json', style: { display: 'none' }, onChange: onFile }),

    // host Tweaks panel mirrors Settings (same state)
    h(TweaksPanel, { key: 'tweaks' }, [
      h(TweakSection, { key: 's1', label: 'Routing' }),
      h(TweakColor, { key: 'accent', label: 'Signal accent', value: t.accent, options: ['#5B8FCB', '#4FB6C4', '#C99A4B', '#5FA47A'], onChange: (v) => setTweak('accent', v) }),
      h(TweakRadio, { key: 'curve', label: 'Connection lines', value: t.curve, options: ['bezier', 'ortho', 'straight'], onChange: (v) => setTweak('curve', v) }),
      h(TweakToggle, { key: 'act', label: 'Signal flow animation', value: t.showActivity, onChange: (v) => setTweak('showActivity', v) }),
      h(TweakSection, { key: 's2', label: 'Canvas' }),
      h(TweakRadio, { key: 'grid', label: 'Grid', value: t.grid, options: ['lines', 'dots', 'off'], onChange: (v) => setTweak('grid', v) }),
      h(TweakToggle, { key: 'grain', label: 'Film grain', value: t.grain, onChange: (v) => setTweak('grain', v) }),
      h(TweakToggle, { key: 'compact', label: 'Compact nodes', value: t.compactNodes, onChange: (v) => setTweak('compactNodes', v) }),
    ]),
  ]);
}

ReactDOM.createRoot(document.getElementById('root')).render(h(App));
