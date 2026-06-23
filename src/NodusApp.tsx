import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AudioDevice } from './bridge';
import { Canvas } from './ui/Canvas';
import { Graph } from './ui/Graph';
import { Topbar } from './ui/Topbar';
import { EngineButton } from './ui/EngineButton';
import { BottomBar } from './ui/BottomBar';
import { ZoomControls } from './ui/ZoomControls';
import { AddPanel } from './ui/AddPanel';
import { EmptyCanvas } from './ui/EmptyCanvas';
import { VirtualDeviceModal } from './ui/VirtualDeviceModal';
import { SelectionBar } from './ui/SelectionBar';
import { useBackend } from './useBackend';
import { useScene } from './useScene';
import { useView } from './useView';

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
  const [setupOpen, setSetupOpen] = useState(true);
  const [search, setSearch] = useState('');
  const canvasAreaRef = useRef<HTMLDivElement>(null);
  const viewCtl = useView(canvasAreaRef);
  const [selection, setSelection] = useState<Set<string>>(new Set());

  // Virtual devices: real ones (backend) + user-created (the "virtual" tab).
  // Creating a real OS device needs the kernel driver (t5); the UX shell is here.
  const [createdVirtuals, setCreatedVirtuals] = useState<AudioDevice[]>([]);
  // The just-created device id — its card opens in name-edit mode (R5 follow-up).
  const [pendingVirtualEdit, setPendingVirtualEdit] = useState<string | null>(null);
  const createdIds = useMemo(() => new Set(createdVirtuals.map((d) => d.id)), [createdVirtuals]);
  const virtualOwn = useMemo(
    () => [
      ...backend.devices.filter((d) => d.device_type === 'virtual' && isOwnVirtual(d)),
      ...createdVirtuals,
    ],
    [backend.devices, createdVirtuals],
  );
  const virtualOther = useMemo(
    () => backend.devices.filter((d) => d.device_type === 'virtual' && !isOwnVirtual(d)),
    [backend.devices],
  );
  const physicalDevices = useMemo(
    () => backend.devices.filter((d) => d.device_type !== 'virtual'),
    [backend.devices],
  );
  const createVirtualDevice = useCallback(() => {
    const id = `nodus-virtual-${Date.now().toString(36)}`;
    setCreatedVirtuals((v) => {
      // Number by OUR virtual devices only (Nodus-branded backend + created).
      const ownBackend = backend.devices.filter(
        (d) => d.device_type === 'virtual' && isOwnVirtual(d),
      ).length;
      const n = ownBackend + v.length + 1;
      return [
        ...v,
        { id, name: `Nodus Mic ${n}`, device_type: 'virtual', is_default: false, original_name: null },
      ];
    });
    setPendingVirtualEdit(id); // open its name field for the user to set a name
    // TODO(t5): also create the real OS device + apply the (renamed) name to it.
  }, [backend.devices]);
  const renameVirtual = useCallback((id: string, name: string) => {
    setCreatedVirtuals((v) => v.map((d) => (d.id === id ? { ...d, name } : d)));
    // TODO(t5): rename the real OS device to match.
  }, []);
  const deleteVirtual = useCallback((id: string) => {
    setCreatedVirtuals((v) => v.filter((d) => d.id !== id));
    // TODO(t5): remove the real OS device.
  }, []);

  // Turning the engine on starts it, then pushes the current graph (proven order).
  const toggleLive = () => backend.setLive(!backend.live, store.applyNow);
  const fitAll = () => viewCtl.fit([...scene.nodes, ...scene.hubs]);

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

  // Switching scenes resets the transient view + selection.
  useEffect(() => {
    setSelection(new Set());
    viewCtl.setView({ x: 0, y: 0, zoom: 1 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store.activeSceneId]);

  // Drop a dragged AddPanel row onto the canvas → create the node where dropped.
  const onCanvasDragOver = (e: React.DragEvent) => {
    if (e.dataTransfer.types.includes('application/nodus-add')) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
    }
  };
  const onCanvasDrop = (e: React.DragEvent) => {
    const raw = e.dataTransfer.getData('application/nodus-add');
    if (!raw) return;
    e.preventDefault();
    let payload: { kind: string; id: string };
    try {
      payload = JSON.parse(raw);
    } catch {
      return;
    }
    const rect = canvasAreaRef.current?.getBoundingClientRect();
    const v = viewCtl.view;
    const wx = (e.clientX - (rect?.left ?? 0) - v.x) / v.zoom;
    const wy = (e.clientY - (rect?.top ?? 0) - v.y) / v.zoom;
    const pos = { x: wx - 30, y: wy - 24 }; // drop point lands near the node head
    if (payload.kind === 'device') {
      const d = [...backend.devices, ...createdVirtuals].find((x) => x.id === payload.id);
      if (d) store.addDevice(d, pos);
    } else if (payload.kind === 'process') {
      const p = backend.processes.find((x) => x.exe_name === payload.id);
      if (p) store.addProcess(p, pos);
    } else if (payload.kind === 'type') {
      store.addNodeType(payload.id, pos);
    }
  };

  return (
    <div className="app-shell">
      <Topbar
        scenes={store.scenes}
        activeId={store.activeSceneId}
        onSwitch={store.switchScene}
        onAdd={store.newScene}
        onClose={store.closeScene}
        onRename={store.renameScene}
      />
      <div
        className="canvas-area"
        ref={canvasAreaRef}
        onDragOver={onCanvasDragOver}
        onDrop={onCanvasDrop}
      >
        <Canvas view={viewCtl.view}>
          {store.isEmpty ? (
            <EmptyCanvas onPreset={store.loadPreset} />
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
              onConnectNewInput={store.connectNewInput}
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
        />
        <AddPanel
          nodes={store.nodeCount}
          routes={store.routeCount}
          devices={physicalDevices}
          processes={backend.processes}
          onAddDevice={store.addDevice}
          onAddProcess={store.addProcess}
        />
        {setupOpen && <VirtualDeviceModal onClose={() => setSetupOpen(false)} />}
      </div>
    </div>
  );
}
