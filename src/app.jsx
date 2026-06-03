/* app.jsx — Nodus orchestrator: scenes, signal sim, Tauri integration. */
import React from 'react';
import { NODUS_mk } from './graph-data.jsx';
import { NodusCanvas, NODE_W } from './canvas.jsx';
import { Toolbar, Library, Inspector, BottomDock, StatusBar, CollapseRail, UI, VirtualDeviceSetupDialog } from './panels.jsx';
import { useTweaks, TweaksPanel, TweakSection, TweakColor, TweakRadio, TweakToggle } from './tweaks-panel.jsx';
import * as Bridge from './tauri-bridge.js';

const { useState, useEffect, useRef, useMemo, useCallback } = React;
const h = React.createElement;

const TWEAK_DEFAULTS = {
  accent: '#5B8FCB',
  curve: 'bezier',
  grid: 'lines',
  showActivity: true,
  grain: true,
  compactNodes: false,
};

function hexA(hex, a) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

let _gid = 5000;
const gid = (p) => p + (++_gid);

function freshScene(name = 'Scene 1') {
  return { id: gid('s'), name, nodes: {}, edges: [], pinned: [], view: { x: 80, y: 60, zoom: 1 } };
}

// Map UI node types to Rust routing graph node types
const RUST_NODE_TYPE = {
  source: 'source', app: 'source',
  output: 'output',
  mixer: 'mixer',
  splitter: 'splitter',
  // Effects as passthrough mixers (MVP — no DSP yet)
  gate: 'mixer', comp: 'mixer', limiter: 'mixer', eq: 'mixer', gain: 'mixer', duck: 'mixer',
  virtual: 'virtual',
  // trigger: skip (control-only, no audio)
};

function buildRoutingGraph(nodes, edges) {
  const rustNodes = [];
  const rustRoutes = [];

  // Effective mute = own mute OR (some node soloed AND this one isn't).
  // Mirrors the visual simulation (App.effMuted) so Solo actually mutes routes in the engine.
  const anySolo = Object.values(nodes).some(n => n.solo);
  const effMuted = (n) => !!(n.muted || (anySolo && !n.solo && n.type !== 'trigger'));

  Object.values(nodes).forEach(n => {
    const nodeType = RUST_NODE_TYPE[n.type];
    if (!nodeType) return;
    rustNodes.push({ id: n.id, node_type: nodeType, label: n.name, device_id: n._deviceId || '', exe_name: n._exeName || null });
  });

  const includedIds = new Set(rustNodes.map(n => n.id));

  edges.forEach(e => {
    if (e.to.port === 'ctrl') return; // skip ducking control wires
    if (!includedIds.has(e.from.node) || !includedIds.has(e.to.node)) return;
    const fn = nodes[e.from.node], tn = nodes[e.to.node];
    if (!fn || !tn) return;
    rustRoutes.push({
      id: e.id,
      from_node: e.from.node,
      to_node: e.to.node,
      volume: (e.vol ?? 100) / 100,
      muted: effMuted(fn) || effMuted(tn),
      pan: (e.pan ?? 0) / 100,
    });
  });

  return { nodes: rustNodes, routes: rustRoutes };
}

function debounce(fn, ms) {
  let timer;
  return (...args) => { clearTimeout(timer); timer = setTimeout(() => fn(...args), ms); };
}

// Known audio apps — always shown in Library with running status from Rust detection
const KNOWN_APPS = [
  { name: 'Discord', exe: 'discord.exe' },
  { name: 'Spotify', exe: 'spotify.exe' },
  { name: 'Arma 3', exe: 'arma3_x64.exe' },
  { name: 'Chrome', exe: 'chrome.exe' },
  { name: 'Firefox', exe: 'firefox.exe' },
  { name: 'Edge', exe: 'msedge.exe' },
  { name: 'TeamSpeak', exe: 'ts3client_win64.exe' },
  { name: 'Steam', exe: 'steam.exe' },
  { name: 'OBS', exe: 'obs64.exe' },
];

export default function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [scenes, setScenes] = useState(() => [freshScene()]);
  const [activeSceneId, setActiveSceneId] = useState(null);
  const [selection, setSelection] = useState({ nodeId: null, edgeId: null, ids: [] });
  const [live, setLive] = useState(false);
  const [ptt, setPtt] = useState(false);
  const [levels, setLevels] = useState({});
  const [ui, setUi] = useState({ lib: true, insp: false, dock: false, inspAuto: false });
  const canvasRegionRef = useRef(null);
  const fileRef = useRef(null);

  // Tauri device/process state
  const [physInputDevices, setPhysInputDevices] = useState([]);
  const [physOutputDevices, setPhysOutputDevices] = useState([]);
  const [physVirtualDevices, setPhysVirtualDevices] = useState([]);
  const [runningExes, setRunningExes] = useState(new Set());

  // Virtual device setup state — drives the onboarding dialog
  // kind: 'not_found' | 'vb_audio' | 'nodus_driver'
  const [virtualSetup, setVirtualSetup] = useState(null);       // null = checking
  const [showSetupDialog, setShowSetupDialog] = useState(false);
  const [setupInstalling, setSetupInstalling] = useState(false);
  const [setupError, setSetupError] = useState(null);

  // Refs for latest state — used by debounced graph application
  const nodesRef = useRef(null);
  const edgesRef = useRef(null);
  const liveRef = useRef(false);

  // Resolve active scene
  const curId = activeSceneId || (scenes[0] && scenes[0].id);
  const scene = scenes.find(s => s.id === curId) || scenes[0];
  const nodes = scene.nodes, edges = scene.edges, view = scene.view;

  // Update refs every render so debounced fn always sees latest state
  nodesRef.current = nodes;
  edgesRef.current = edges;
  liveRef.current = live;

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

  /* ---- Tauri init: fetch devices + processes, subscribe to events ---- */
  const updateDevices = useCallback((devices) => {
    setPhysInputDevices(devices
      .filter(d => d.device_type === 'input')
      .map(d => ({ name: d.name, device: d.name, kind: 'device', _deviceId: d.id })));
    setPhysOutputDevices(devices
      .filter(d => d.device_type === 'output')
      .map(d => ({ name: d.name, device: d.name, kind: 'device', _deviceId: d.id })));
    setPhysVirtualDevices(devices
      .filter(d => d.device_type === 'virtual')
      .map(d => ({ io: 'out', name: d.name, device: d.original_name || d.name, kind: 'virtual', _deviceId: d.id })));
  }, []);

  const updateProcesses = useCallback((processes) => {
    setRunningExes(new Set(processes.map(p => p.exe_name.toLowerCase())));
  }, []);

  useEffect(() => {
    let unlistenDevices, unlistenProcesses;
    async function init() {
      const [devices, processes] = await Promise.all([
        Bridge.getAudioDevices().catch(() => []),
        Bridge.getRunningAudioProcesses().catch(() => []),
      ]);
      updateDevices(devices);
      updateProcesses(processes);
      unlistenDevices = await Bridge.listenToEvent('audio-devices-changed', updateDevices);
      unlistenProcesses = await Bridge.listenToEvent('process-changed', updateProcesses);
    }
    init();
    return () => {
      if (unlistenDevices) unlistenDevices();
      if (unlistenProcesses) unlistenProcesses();
    };
  }, [updateDevices, updateProcesses]);

  /* ---- Virtual device setup check (runs once on startup) ---- */
  useEffect(() => {
    async function checkVirtualSetup() {
      const status = await Bridge.getVirtualSetupStatus().catch(() => null);
      if (!status) return;
      setVirtualSetup(status);
      if (status.kind === 'not_found') setShowSetupDialog(true);
    }
    checkVirtualSetup();
  }, []);

  // Called by the setup dialog "Install VB-Audio" button
  const handleInstallVbcable = useCallback(async () => {
    setSetupInstalling(true);
    setSetupError(null);
    try {
      await Bridge.installVbcable();
      // Re-check after install (user may have cancelled the UAC prompt)
      const [status, devices] = await Promise.all([
        Bridge.getVirtualSetupStatus(),
        Bridge.getAudioDevices(),
      ]);
      setVirtualSetup(status);
      updateDevices(devices);
      if (status.kind !== 'not_found') setShowSetupDialog(false);
    } catch (e) {
      setSetupError(String(e));
    } finally {
      setSetupInstalling(false);
    }
  }, [updateDevices]);

  // Called by the setup dialog "Skip" button
  const handleSkipSetup = useCallback(() => setShowSetupDialog(false), []);

  /* ---- Library device lists ---- */
  const libraryInputDevices = useMemo(() => [
    ...physInputDevices,
    ...KNOWN_APPS
      .filter(app => runningExes.has(app.exe))
      .map(app => ({
        name: app.name, device: 'app capture', kind: 'app',
        running: true,
        exe: app.exe,
      })),
  ], [physInputDevices, runningExes]);

  const libraryOutputDevices = physOutputDevices;

  const handleRescan = useCallback(async () => {
    const [devices, processes] = await Promise.all([
      Bridge.getAudioDevices().catch(() => []),
      Bridge.getRunningAudioProcesses().catch(() => []),
    ]);
    updateDevices(devices);
    updateProcesses(processes);
  }, [updateDevices, updateProcesses]);

  /* ---- Debounced routing graph application ---- */
  const applyGraphLater = useMemo(() => debounce(async () => {
    if (!liveRef.current) return;
    const graph = buildRoutingGraph(nodesRef.current, edgesRef.current);
    await Bridge.applyRoutingGraph(graph).catch(e => console.error('apply_routing_graph:', e));
  }, 300), []);

  /* ---- Signal propagation (visual simulation) ---- */
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

  // VU meter levels — real data from Tauri volume-levels event, mock fallback in browser.
  useEffect(() => {
    if (!live) { setLevels({}); return; }

    let unlisten;
    // Real levels: Rust emits {device_id|exe_name: 0.0-1.0} at 30fps.
    // Device sources keyed by WASAPI device_id; app sources keyed by exe_name.
    Bridge.listenToEvent('volume-levels', (deviceLevels) => {
      const nodesNow = nodesRef.current;
      const next = {};
      for (const [key, level] of Object.entries(deviceLevels)) {
        for (const [nodeId, node] of Object.entries(nodesNow)) {
          const match = (node._deviceId && node._deviceId === key) ||
                        (node._exeName && node._exeName.toLowerCase() === key.toLowerCase());
          if (match) next[nodeId] = Math.max(next[nodeId] || 0, level);
        }
      }
      setLevels(next);
    }).then(fn => { unlisten = fn; });

    // Browser-mode fallback mock (no Tauri IPC available)
    const isTauri = typeof window !== 'undefined' && !!window.__TAURI_IPC__;
    const iv = !isTauri ? setInterval(() => {
      setLevels(prev => {
        const next = {};
        activeNodes.forEach(id => { const target = 0.3 + Math.random() * 0.7; next[id] = (prev[id] ?? 0.5) + (target - (prev[id] ?? 0.5)) * 0.5; });
        return next;
      });
    }, 110) : null;

    return () => {
      if (unlisten) unlisten();
      if (iv) clearInterval(iv);
    };
  }, [live, activeNodes]); // eslint-disable-line

  /* ---- Live toggle with engine start/stop ---- */
  const handleToggleLive = useCallback(async () => {
    const newLive = !liveRef.current;
    setLive(newLive);
    if (newLive) {
      await Bridge.startEngine().catch(e => console.error('start_engine:', e));
      const graph = buildRoutingGraph(nodesRef.current, edgesRef.current);
      await Bridge.applyRoutingGraph(graph).catch(e => console.error('apply_routing_graph:', e));
    } else {
      await Bridge.stopEngine().catch(e => console.error('stop_engine:', e));
    }
  }, []);

  /* ---- Inspector auto-open/close ---- */
  const openInspForSelection = () => setUi(u => u.insp ? u : { ...u, insp: true, inspAuto: true });
  const collapseIfAuto = () => setUi(u => u.inspAuto ? { ...u, insp: false, inspAuto: false } : u);

  /* ---- Node selection ---- */
  const selectNode = (id, additive) => {
    openInspForSelection();
    setSelection(s => {
      if (additive) {
        const ids = (s.ids || []); const has = ids.includes(id);
        const next = has ? ids.filter(x => x !== id) : [...ids, id];
        return { nodeId: next.length === 1 ? next[0] : null, edgeId: null, ids: next };
      }
      return { nodeId: id, edgeId: null, ids: [id] };
    });
  };
  const selectEdge = (id) => { openInspForSelection(); setSelection({ nodeId: null, edgeId: id, ids: [] }); };
  const clearSelection = () => { collapseIfAuto(); setSelection({ nodeId: null, edgeId: null, ids: [] }); };
  const marqueeSelect = (ids) => { if (ids.length) openInspForSelection(); else collapseIfAuto(); setSelection({ nodeId: ids.length === 1 ? ids[0] : null, edgeId: null, ids }); };

  /* ---- Node ops ---- */
  const moveNode = useCallback((id, x, y) => setNodes(n => ({ ...n, [id]: { ...n[id], x, y } })), [setNodes]);

  const muteNode = (id) => {
    setNodes(n => ({ ...n, [id]: { ...n[id], muted: !n[id].muted } }));
    applyGraphLater();
  };

  const soloNode = (id) => {
    setNodes(n => ({ ...n, [id]: { ...n[id], solo: !n[id].solo } }));
    applyGraphLater(); // solo changes effective mute of other routes → push to engine
  };
  const setVolume = (id, v) => setNodes(n => ({ ...n, [id]: { ...n[id], volume: v } }));
  const rename = (id, name) => setNodes(n => ({ ...n, [id]: { ...n[id], name } }));
  const setParam = (id, k, v) => setNodes(n => ({ ...n, [id]: { ...n[id], params: { ...n[id].params, [k]: v } } }));

  const setEdgeVol = (id, v) => {
    setEdges(es => es.map(e => e.id === id ? { ...e, vol: v } : e));
    if (liveRef.current) Bridge.setRouteVolume(id, v / 100).catch(console.error);
  };

  // Stereo balance per route. UI stores -100..100 (center 0); engine takes -1..1.
  const setEdgePan = (id, p) => {
    setEdges(es => es.map(e => e.id === id ? { ...e, pan: p } : e));
    if (liveRef.current) Bridge.setRoutePan(id, p / 100).catch(console.error);
  };

  const deleteNode = (id) => {
    setNodes(n => { const c = { ...n }; delete c[id]; return c; });
    setEdges(es => es.filter(e => e.from.node !== id && e.to.node !== id));
    setPinnedArr(p => p.filter(x => x !== id));
    collapseIfAuto();
    setSelection({ nodeId: null, edgeId: null, ids: [] });
    applyGraphLater();
  };

  const deleteEdge = (id) => {
    setEdges(es => es.filter(e => e.id !== id));
    collapseIfAuto();
    setSelection({ nodeId: null, edgeId: null, ids: [] });
    applyGraphLater();
  };

  /* ---- Bulk (marquee) ops ---- */
  const getSelectedPositions = () => {
    const m = {};
    (selection.ids || []).forEach(id => { if (nodes[id]) m[id] = { x: nodes[id].x, y: nodes[id].y }; });
    return m;
  };
  const moveMany = (snapshot, dx, dy) => setNodes(n => {
    const c = { ...n };
    Object.keys(snapshot).forEach(id => { if (c[id]) c[id] = { ...c[id], x: snapshot[id].x + dx, y: snapshot[id].y + dy }; });
    return c;
  });
  const deleteSelection = () => {
    const ids = new Set(selection.ids || []);
    if (!ids.size) return;
    setNodes(n => { const c = { ...n }; ids.forEach(id => delete c[id]); return c; });
    setEdges(es => es.filter(e => !ids.has(e.from.node) && !ids.has(e.to.node)));
    setPinnedArr(p => p.filter(x => !ids.has(x)));
    collapseIfAuto();
    setSelection({ nodeId: null, edgeId: null, ids: [] });
    applyGraphLater();
  };
  const muteSelection = (val) => {
    const ids = new Set(selection.ids || []);
    setNodes(n => { const c = { ...n }; ids.forEach(id => { if (c[id] && c[id].type !== 'trigger') c[id] = { ...c[id], muted: val }; }); return c; });
    applyGraphLater();
  };

  const duplicateNode = (id) => setNodes(n => {
    const src = n[id]; if (!src) return n;
    const nid = gid('n');
    const copy = { ...JSON.parse(JSON.stringify(src)), id: nid, x: src.x + 36, y: src.y + 36, name: src.name + ' copy' };
    setSelection({ nodeId: nid, edgeId: null });
    return { ...n, [nid]: copy };
  });

  const createEdge = useCallback((from, to) => setEdges(es => {
    const filtered = es.filter(e => !(e.to.node === to.node && e.to.port === to.port) && !(e.from.node === from.node && e.from.port === from.port));
    const newEdges = [...filtered, { id: gid('e'), from, to, vol: 100 }];
    if (liveRef.current) applyGraphLater();
    return newEdges;
  }), [setEdges, applyGraphLater]);

  /* ---- Dynamic ports ---- */
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
    applyGraphLater();
  };

  /* ---- Drops ---- */
  const dropTemplate = useCallback((type, x, y) => {
    const node = NODUS_mk(type, null, x, y); node.id = gid('n');
    setNodes(n => ({ ...n, [node.id]: node }));
    setSelection({ nodeId: node.id, edgeId: null });
  }, [setNodes]);

  const dropDevice = useCallback((d, x, y) => {
    const type = d.io === 'out' ? 'output' : (d.kind === 'app' ? 'app' : 'source');
    const node = NODUS_mk(type, d.name, x, y, { device: d.device, _deviceId: d._deviceId || '', _exeName: d.exe || null });
    if (d.io === 'out' && /head/i.test(d.name)) node.glyph = 'headphone';
    node.id = gid('n');
    setNodes(n => ({ ...n, [node.id]: node }));
    setSelection({ nodeId: node.id, edgeId: null });
    return node.id;
  }, [setNodes]);

  /* ---- Quick-control dock (pinned nodes) ---- */
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

  /* ---- Scenes ---- */
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

  /* ---- Import / export ---- */
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

  /* ---- Keyboard ---- */
  useEffect(() => {
    const pttKey = (Object.values(nodes).find(n => n.type === 'trigger')?.params.key || 'V').toLowerCase();
    const down = (e) => {
      const tag = (e.target.tagName || '').toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      if (e.key.toLowerCase() === pttKey && !e.repeat) setPtt(true);
      if (e.key === 'Delete' || e.key === 'Backspace') {
        if ((selection.ids || []).length > 1) deleteSelection();
        else if (selection.edgeId) deleteEdge(selection.edgeId);
        else if (selection.nodeId) deleteNode(selection.nodeId);
      }
      if (e.key === ' ') { e.preventDefault(); handleToggleLive(); }
    };
    const up = (e) => { if (e.key.toLowerCase() === pttKey) setPtt(false); };
    window.addEventListener('keydown', down); window.addEventListener('keyup', up);
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up); };
  }, [nodes, selection, handleToggleLive]); // eslint-disable-line

  /* ---- Fit view ---- */
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
      h(Toolbar, {
        live, onToggleLive: handleToggleLive,
        scenes, activeId: curId,
        onSwitchScene: switchScene, onNewScene: newScene, onCloseScene: closeScene, onRenameScene: renameScene,
        t, setTweak, onImport: doImport, onExport: doExport,
      })),

    h('div', { key: 'lib', className: 'region-lib' },
      ui.lib
        ? h(Library, {
            onCollapse: () => setUi(u => ({ ...u, lib: false })),
            inputDevices: libraryInputDevices,
            outputDevices: libraryOutputDevices,
            virtualDevices: physVirtualDevices,
            onRescan: handleRescan,
          })
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
      ui.insp
        ? h(Inspector, {
            node: selNode, edge: selEdge, multi: multiNodes, nodes, edges,
            onCollapse: () => setUi(u => ({ ...u, insp: false, inspAuto: false })),
            onRename: rename, onVolume: setVolume, onParam: setParam, onMute: muteNode, onSolo: soloNode,
            onDuplicate: duplicateNode, onDelete: deleteNode, onDeleteEdge: deleteEdge, onEdgeVol: setEdgeVol, onEdgePan: setEdgePan,
            onSelectNode: selectNode, onAddPort: addPort, onRemovePort: removePort,
            onDeleteSelection: deleteSelection, onMuteSelection: muteSelection,
            isPinned: selNode ? (scene.pinned || []).includes(selNode.id) : false, onTogglePin: togglePin,
          })
        : h(CollapseRail, { side: 'right', label: 'Inspector', onExpand: () => setUi(u => ({ ...u, insp: true, inspAuto: false })) })),

    h('div', { key: 'dock', className: 'region-dock' },
      h(BottomDock, {
        open: ui.dock, onToggle: () => setUi(u => ({ ...u, dock: !u.dock })),
        pinned: pinnedNodes, nodes, edges, allNodes: Object.values(nodes),
        onUnpin: unpinNode, onPin: pinNode, onFocus: focusNode,
        onEdgeVol: setEdgeVol, onVolume: setVolume, onMute: muteNode, onSolo: soloNode,
      })),

    h('div', { key: 'status', className: 'region-status' },
      h(StatusBar, {
        view, setView, onFit: fitView,
        nodeCount: Object.keys(nodes).length, edgeCount: edges.length,
        live, activeCount: activeEdges.size, pttActive: ptt,
      })),

    h('input', { key: 'file', ref: fileRef, type: 'file', accept: 'application/json,.json', style: { display: 'none' }, onChange: onFile }),

    h(VirtualDeviceSetupDialog, {
      key: 'vdsetup',
      show: showSetupDialog,
      status: virtualSetup,
      installing: setupInstalling,
      error: setupError,
      onInstallVbcable: handleInstallVbcable,
      onSkip: handleSkipSetup,
    }),

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
