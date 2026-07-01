import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  emitEvent,
  listenAny,
  winShow,
  listVirtualDevices,
  createVirtualDevice as bridgeCreateVirtualDevice,
  removeVirtualDevice as bridgeRemoveVirtualDevice,
  type AudioDevice,
  type VirtualDeviceInfo,
} from './bridge';
import { Canvas } from './ui/Canvas';
import { Graph } from './ui/Graph';
import { Topbar } from './ui/Topbar';
import { EngineButton } from './ui/EngineButton';
import { BottomBar } from './ui/BottomBar';
import { ZoomControls } from './ui/ZoomControls';
import { AddPanel } from './ui/AddPanel';
import { EmptyCanvas } from './ui/EmptyCanvas';
import { VirtualDeviceModal } from './ui/VirtualDeviceModal';
import { SettingsModal } from './ui/SettingsModal';
import { SelectionBar } from './ui/SelectionBar';
import { QuickPanel, type QuickItem } from './ui/QuickPanel';
import { useBackend } from './useBackend';
import { useScene } from './useScene';
import { useSettings } from './useSettings';
import { useView } from './useView';
import { usePlaceDrag, type PlacePayload } from './usePlaceDrag';
import { bindScene, buildPreset, type PresetId } from './scenes';

/** Nodus's own virtual device (created here, or Nodus-branded) vs third-party. */
const isOwnVirtual = (d: AudioDevice) =>
  /nodus/i.test(d.name) || /nodus/i.test(d.original_name ?? '');

/**
 * NodusApp — root of the redesigned Nodus UI.
 *
 * The scene starts empty (first-run), so the EmptyCanvas onboarding shows;
 * preset cards build a ready graph. The scene store (useScene) holds the live
 * canvas and pushes a RoutingGraph to the engine; real devices, meters and the
 * engine come from useBackend (R3/R18).
 */
export default function NodusApp() {
  const backend = useBackend();
  const store = useScene(backend.live);
  const { scene } = store;
  const settingsCtl = useSettings();
  const [setupOpen, setSetupOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [search, setSearch] = useState('');
  const canvasAreaRef = useRef<HTMLDivElement>(null);
  const viewCtl = useView(canvasAreaRef);
  const [selection, setSelection] = useState<Set<string>>(new Set());

  // Virtual devices (t5 step 3 S3.5). The dynamic ones are REAL OS devices managed
  // through the kernel control channel; the driver table is the source of truth.
  // (Routing a dynamic device through the engine — mapping its WASAPI endpoint to
  // its ring — completes with t8 name correlation; here we manage create/delete.)
  const [managed, setManaged] = useState<VirtualDeviceInfo[]>([]);
  const [pendingVirtualEdit, setPendingVirtualEdit] = useState<string | null>(null);
  const refreshManaged = useCallback(() => {
    void listVirtualDevices().then(setManaged).catch((e) => console.error('list_virtual_devices:', e));
  }, []);
  useEffect(() => {
    if (backend.ready) refreshManaged();
  }, [backend.ready, refreshManaged]);

  // Static Nodus endpoints from the OS enumeration (real, routable — keep as-is).
  const staticOwn = useMemo(
    () => backend.devices.filter((d) => d.is_virtual && isOwnVirtual(d)),
    [backend.devices],
  );
  // Dynamic devices from the driver table, mapped to the card shape. Synthetic id
  // `nodus:<driverId>` so delete/rename can recover the driver id.
  const dynamicOwn = useMemo<AudioDevice[]>(
    () =>
      managed
        .filter((m) => !m.is_static)
        .map((m) => ({
          id: `nodus:${m.id}`,
          name: m.name,
          device_type: m.kind === 'capture' ? 'input' : 'virtual',
          is_default: false,
          // Provenance marker: WE created these via the driver, so they are always
          // "ours" regardless of the user's custom name. deviceNode()'s isOwn check
          // matches /nodus/i on name+original_name — without this, a custom-named mic
          // ("Микрофон для Дискорд") fails isOwn and is misclassified as a THIRD-PARTY
          // capture source (port-out) instead of our sink (port-in). (t5 fix)
          original_name: 'Nodus Virtual Audio',
          is_virtual: true,
        })),
    [managed],
  );
  const virtualOwn = useMemo(() => [...staticOwn, ...dynamicOwn], [staticOwn, dynamicOwn]);
  const createdIds = useMemo(() => new Set(dynamicOwn.map((d) => d.id)), [dynamicOwn]);
  const virtualOther = useMemo(
    () => backend.devices.filter((d) => d.is_virtual && !isOwnVirtual(d)),
    [backend.devices],
  );
  const physicalDevices = useMemo(
    () => backend.devices.filter((d) => !d.is_virtual),
    [backend.devices],
  );

  const driverIdOf = (synthetic: string): number | null => {
    const m = /^nodus:(\d+)$/.exec(synthetic);
    return m ? Number(m[1]) : null;
  };

  const createVirtualDevice = useCallback(() => {
    const n = managed.filter((m) => !m.is_static).length + 1;
    void bridgeCreateVirtualDevice('capture', `Nodus Mic ${n}`)
      .then((info) => {
        refreshManaged();
        if (info) setPendingVirtualEdit(`nodus:${info.id}`); // open its name field
      })
      .catch((e) => console.error('create_virtual_device:', e));
  }, [managed, refreshManaged]);

  // No SET_NAME IOCTL yet (ADR §6.2): rename = destroy + recreate with the new name.
  const renameVirtual = useCallback(
    (id: string, name: string) => {
      const driverId = driverIdOf(id);
      const dev = managed.find((m) => m.id === driverId && !m.is_static);
      if (driverId == null || !dev) return;
      void bridgeRemoveVirtualDevice(driverId)
        .then(() => bridgeCreateVirtualDevice(dev.kind, name))
        .then(() => refreshManaged())
        .catch((e) => console.error('rename_virtual_device:', e));
    },
    [managed, refreshManaged],
  );

  const deleteVirtual = useCallback(
    (id: string) => {
      const driverId = driverIdOf(id);
      if (driverId == null) return;
      void bridgeRemoveVirtualDevice(driverId)
        .then(() => refreshManaged())
        .catch((e) => console.error('remove_virtual_device:', e));
    },
    [refreshManaged],
  );

  // Load a preset, bound to the user's real devices/processes so it routes (R18).
  const loadPreset = useCallback(
    (id: PresetId) => {
      const out =
        backend.devices.find((d) => d.device_type === 'output' && d.is_default) ??
        backend.devices.find((d) => d.device_type === 'output');
      const inp =
        backend.devices.find((d) => d.device_type === 'input' && d.is_default) ??
        backend.devices.find((d) => d.device_type === 'input');
      store.replaceScene(
        bindScene(buildPreset(id), {
          output: out,
          // Bind a routable Nodus mic (static endpoint); dynamic-device routing
          // lands with t8 correlation.
          input: inp,
          virtualMic: staticOwn.find((d) => d.device_type === 'input') ?? staticOwn[0],
          processes: backend.processes,
        }),
      );
    },
    [backend.devices, backend.processes, staticOwn, store],
  );

  // Turning the engine on starts it, then pushes the current graph (proven order).
  const toggleLive = () => backend.setLive(!backend.live, store.applyNow);
  const fitAll = () => viewCtl.fit([...scene.nodes, ...scene.hubs]);

  // Start the engine on launch if the setting asks for it (t14). Once, after both
  // settings and backend are ready, and only when the engine isn't already live.
  const autostartedRef = useRef(false);
  useEffect(() => {
    if (autostartedRef.current) return;
    if (!settingsCtl.ready || !backend.ready) return;
    autostartedRef.current = true; // decide once
    if (settingsCtl.settings.start_engine_on_launch && !backend.live) {
      backend.setLive(true, store.applyNow);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settingsCtl.ready, backend.ready]);

  // ── Quick-controls popup (t13, Phase A) ───────────────────────────────────
  // Pinned set drives the pin button on each node; the popup lists pinned nodes.
  const pinnedSet = useMemo(() => new Set(store.pinned), [store.pinned]);
  const [quickOpen, setQuickOpen] = useState(false);
  const quickItems = useMemo<QuickItem[]>(
    () =>
      store.pinned
        .map((id): QuickItem | null => {
          const node = scene.nodes.find((n) => n.id === id);
          if (node) return { kind: 'node', node };
          const hub = scene.hubs.find((h) => h.id === id);
          if (hub) return { kind: 'hub', hub };
          return null;
        })
        .filter((x): x is QuickItem => x !== null),
    [store.pinned, scene.nodes, scene.hubs],
  );
  // Temporary trigger until the system tray (Phase B): press "q" to toggle.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'q' && e.key !== 'Q') return;
      const tag = (e.target as HTMLElement)?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      setQuickOpen((v) => !v);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // ── Bridge to the tray flyout window (t13 Phase B2) ────────────────────────
  // Mirror the pinned snapshot out, and apply control commands coming back.
  // The flyout is a separate webview with no shared store; the main window stays
  // the single source of truth. All no-ops in the browser (emit/listen stubs).
  // When the engine started (epoch ms), for the flyout's uptime clock; null when off.
  const [liveSince, setLiveSince] = useState<number | null>(null);
  useEffect(() => {
    setLiveSince(backend.live ? Date.now() : null);
  }, [backend.live]);

  const snapshotRef = useRef({
    live: backend.live,
    items: quickItems,
    scenes: store.scenes,
    activeId: store.activeSceneId,
    liveSince,
  });
  snapshotRef.current = {
    live: backend.live,
    items: quickItems,
    scenes: store.scenes,
    activeId: store.activeSceneId,
    liveSince,
  };
  useEffect(() => {
    void emitEvent('quick:snapshot', snapshotRef.current);
  }, [quickItems, backend.live, store.scenes, store.activeSceneId, liveSince]);

  const storeRef = useRef(store);
  storeRef.current = store;
  const toggleLiveRef = useRef(toggleLive);
  toggleLiveRef.current = toggleLive;
  useEffect(() => {
    let unReq = () => {};
    let unCmd = () => {};
    let alive = true;
    const bind = (fn: () => void, set: (f: () => void) => void) => (alive ? set(fn) : fn());
    listenAny('quick:request', () => void emitEvent('quick:snapshot', snapshotRef.current)).then(
      (f) => bind(f, (x) => (unReq = x)),
    );
    listenAny<{ type: string; id?: string; inputId?: string; value?: number; name?: string }>(
      'quick:cmd',
      (c) => {
        const s = storeRef.current;
        switch (c.type) {
          case 'nodeVolume':
            if (c.id) s.setNodeVolume(c.id, c.value ?? 0);
            break;
          case 'nodeMute':
            if (c.id) s.toggleNodeMute(c.id);
            break;
          case 'nodeSolo':
            if (c.id) s.toggleNodeSolo(c.id);
            break;
          case 'hubInputVolume':
            if (c.id && c.inputId) s.setHubInputVolume(c.id, c.inputId, c.value ?? 0);
            break;
          case 'unpin':
            if (c.id) s.togglePin(c.id);
            break;
          case 'toggleLive':
            toggleLiveRef.current();
            break;
          case 'sceneSwitch':
            if (c.id) s.switchScene(c.id);
            break;
          case 'sceneAdd':
            s.newScene();
            break;
          case 'sceneClose':
            if (c.id) s.closeScene(c.id);
            break;
          case 'sceneRename':
            if (c.id && c.name) s.renameScene(c.id, c.name);
            break;
          case 'showMain':
            void winShow();
            break;
        }
      },
    ).then((f) => bind(f, (x) => (unCmd = x)));
    return () => {
      alive = false;
      unReq();
      unCmd();
    };
  }, []);

  // ── Multi-select group actions (R23) ──────────────────────────────────────
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const clearSelection = useCallback(() => setSelection(new Set()), []);
  const deleteSelection = useCallback(() => {
    store.removeNodes([...selectionRef.current]);
    setSelection(new Set());
  }, [store]);
  const muteSelection = useCallback(
    (muted: boolean) => store.setNodesMuted([...selectionRef.current], muted),
    [store],
  );

  // Delete / Backspace removes the selection (ignored while typing in a field).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Delete' && e.key !== 'Backspace') return;
      const tag = (e.target as HTMLElement)?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      if (selectionRef.current.size > 0) {
        e.preventDefault();
        deleteSelection();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [deleteSelection]);

  // Esc clears the selection.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && selectionRef.current.size > 0) clearSelection();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [clearSelection]);

  // Undo (Ctrl+Z) / Redo (Ctrl+X) for scene edits. Use e.code (physical key) so it
  // works on non-Latin keyboard layouts too — on a Cyrillic layout e.key is 'я'/'ч',
  // not 'z'/'x'. Ignored while typing in a field so native text undo/cut still works.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const tag = (e.target as HTMLElement)?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea') return;
      if (e.code === 'KeyZ') {
        e.preventDefault();
        storeRef.current.undo();
      } else if (e.code === 'KeyX') {
        e.preventDefault();
        storeRef.current.redo();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Switching scenes resets the transient view + selection.
  useEffect(() => {
    setSelection(new Set());
    viewCtl.setView({ x: 0, y: 0, zoom: 1 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store.activeSceneId]);

  // Place a palette item (device/process/catalog type) at a world position.
  // `world` already cancels pan/zoom; offset so the drop lands near the head.
  const placeNode = useCallback(
    (payload: PlacePayload, world: { x: number; y: number }) => {
      const pos = { x: world.x - 30, y: world.y - 24 };
      if (payload.kind === 'device') {
        const d = [...backend.devices, ...dynamicOwn].find((x) => x.id === payload.id);
        if (d) store.addDevice(d, pos);
      } else if (payload.kind === 'process') {
        const p = backend.processes.find((x) => x.exe_name === payload.id);
        if (p) store.addProcess(p, pos);
      } else if (payload.kind === 'type') {
        store.addNodeType(payload.id, pos);
      }
    },
    [backend.devices, backend.processes, dynamicOwn, store],
  );

  // Pointer-based drag from the palettes onto the canvas (WebView2-safe).
  const viewRef = useRef(viewCtl.view);
  viewRef.current = viewCtl.view;
  const place = usePlaceDrag({ canvasRef: canvasAreaRef, viewRef, onPlace: placeNode });

  return (
    <div className="app-shell">
      <Topbar
        scenes={store.scenes}
        activeId={store.activeSceneId}
        onSwitch={store.switchScene}
        onAdd={store.newScene}
        onClose={store.closeScene}
        onRename={store.renameScene}
        onOpenSettings={() => setSettingsOpen(true)}
        onUndo={store.undo}
        onRedo={store.redo}
        canUndo={store.canUndo}
        canRedo={store.canRedo}
      />
      <div className="canvas-area" ref={canvasAreaRef}>
        <Canvas view={viewCtl.view}>
          {store.isEmpty ? (
            <EmptyCanvas onPreset={loadPreset} />
          ) : (
            <Graph
              nodes={scene.nodes}
              edges={scene.edges}
              hubs={scene.hubs}
              search={search}
              levels={backend.levels}
              view={viewCtl.view}
              setView={viewCtl.setView}
              selection={selection}
              setSelection={setSelection}
              onNodeVolume={store.setNodeVolume}
              onNodeMute={store.toggleNodeMute}
              onNodesMove={store.moveNodes}
              onNodeSolo={store.toggleNodeSolo}
              onNodeDuplicate={store.duplicateNode}
              onNodeDelete={(id) => {
                store.removeNode(id);
                setSelection(new Set());
              }}
              onNodeRename={store.renameNode}
              onConnect={store.connect}
              onEdgeVolume={store.setEdgeVolume}
              onEdgeMute={store.setEdgeMute}
              onEdgePan={store.setEdgePan}
              onRemoveEdge={store.removeEdge}
              onRemoveHubInput={store.removeHubInput}
              onHubInputVolume={store.setHubInputVolume}
              onConnectNewInput={store.connectNewInput}
              onConnectNewOutput={store.connectNewOutput}
              onConnectNewBoth={store.connectNewBoth}
              pinned={pinnedSet}
              onPin={store.togglePin}
            />
          )}
        </Canvas>
        {selection.size >= 2 && (
          <SelectionBar
            count={selection.size}
            onMuteAll={() => muteSelection(true)}
            onUnmuteAll={() => muteSelection(false)}
            onDelete={deleteSelection}
            onClear={clearSelection}
          />
        )}
        <EngineButton live={backend.live} onToggleLive={toggleLive} />
        <ZoomControls
          zoom={viewCtl.view.zoom}
          onZoomIn={viewCtl.zoomIn}
          onZoomOut={viewCtl.zoomOut}
          onReset={viewCtl.resetZoom}
          onFit={fitAll}
        />
        <BottomBar
          onSearch={setSearch}
          virtualOwn={virtualOwn}
          virtualOther={virtualOther}
          createdIds={createdIds}
          editingId={pendingVirtualEdit}
          onCreateVirtual={createVirtualDevice}
          onRenameVirtual={(id, name) => {
            renameVirtual(id, name);
            setPendingVirtualEdit(null);
          }}
          onDeleteVirtual={deleteVirtual}
          onBeginPlace={place.begin}
        />
        <AddPanel
          nodes={store.nodeCount}
          routes={store.routeCount}
          devices={physicalDevices}
          processes={backend.processes}
          onAddDevice={store.addDevice}
          onAddProcess={store.addProcess}
          onBeginPlace={place.begin}
        />
        {setupOpen && <VirtualDeviceModal onClose={() => setSetupOpen(false)} />}
        {settingsOpen && (
          <SettingsModal
            settings={settingsCtl.settings}
            onUpdate={settingsCtl.update}
            onClose={() => setSettingsOpen(false)}
          />
        )}
      </div>
      {quickOpen && (
        <QuickPanel
          items={quickItems}
          live={backend.live}
          onToggleLive={toggleLive}
          onNodeVolume={store.setNodeVolume}
          onNodeMute={store.toggleNodeMute}
          onNodeSolo={store.toggleNodeSolo}
          onHubInputVolume={store.setHubInputVolume}
          onUnpin={store.togglePin}
          onClose={() => setQuickOpen(false)}
        />
      )}
      {place.ghost && (
        <div
          className="place-ghost"
          style={{ left: place.ghost.x + 14, top: place.ghost.y + 14 }}
        >
          {place.ghost.label}
        </div>
      )}
    </div>
  );
}
