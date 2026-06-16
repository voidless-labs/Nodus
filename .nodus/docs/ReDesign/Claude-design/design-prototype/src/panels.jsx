/* panels.jsx — Toolbar, Library, Inspector, StatusBar, BottomDock, rails (v2). */

const { useState: _uS, useRef: _uR, useEffect: _uE } = React;

const UI = {
  play: () => h('svg', { viewBox: '0 0 24 24', fill: 'currentColor' }, h('path', { d: 'M8 5v14l11-7z' })),
  stop: () => h('svg', { viewBox: '0 0 24 24', fill: 'currentColor' }, h('rect', { x: 7, y: 7, width: 10, height: 10, rx: 1 })),
  search: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }, [h('circle', { key: 'c', cx: 11, cy: 11, r: 7 }), h('path', { key: 'l', d: 'M21 21l-4-4' })]),
  chev: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2 }, h('path', { d: 'M6 9l6 6 6-6' })),
  plus: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round' }, h('path', { d: 'M12 5v14M5 12h14' })),
  minus: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }, h('path', { d: 'M5 12h14' })),
  x: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2.2, strokeLinecap: 'round' }, h('path', { d: 'M6 6l12 12M18 6L6 18' })),
  fit: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }, [h('path', { key: 'a', d: 'M4 9V5a1 1 0 011-1h4' }), h('path', { key: 'b', d: 'M20 9V5a1 1 0 00-1-1h-4' }), h('path', { key: 'c', d: 'M4 15v4a1 1 0 001 1h4' }), h('path', { key: 'd', d: 'M20 15v4a1 1 0 01-1 1h-4' })]),
  center: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }, [h('circle', { key: 'c', cx: 12, cy: 12, r: 3 }), h('path', { key: 'l', d: 'M12 3v3M12 18v3M3 12h3M18 12h3' })]),
  settings: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.6 }, [h('circle', { key: 'c', cx: 12, cy: 12, r: 3 }), h('path', { key: 'p', d: 'M12 2.5v3M12 18.5v3M21.5 12h-3M5.5 12h-3M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1M18.4 18.4l-2.1-2.1M7.7 7.7L5.6 5.6' })]),
  import: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round', strokeLinejoin: 'round' }, [h('path', { key: 'a', d: 'M12 3v12' }), h('path', { key: 'b', d: 'M8 11l4 4 4-4' }), h('path', { key: 'c', d: 'M4 19h16' })]),
  export: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round', strokeLinejoin: 'round' }, [h('path', { key: 'a', d: 'M12 15V3' }), h('path', { key: 'b', d: 'M8 7l4-4 4 4' }), h('path', { key: 'c', d: 'M4 19h16' })]),
  refresh: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round' }, [h('path', { key: 'a', d: 'M21 12a9 9 0 1 1-2.6-6.3' }), h('path', { key: 'b', d: 'M21 4v5h-5' })]),
  panelL: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7 }, [h('rect', { key: 'r', x: 3, y: 4, width: 18, height: 16, rx: 2 }), h('path', { key: 'l', d: 'M9 4v16' })]),
  pin: () => h('svg', { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.6, strokeLinecap: 'round', strokeLinejoin: 'round' }, [h('path', { key: 'a', d: 'M12 17v5' }), h('path', { key: 'b', d: 'M9 3h6l-1 6 3 3v1H7v-1l3-3-1-6z' })]),
  pinOn: () => h('svg', { viewBox: '0 0 24 24', fill: 'currentColor', stroke: 'currentColor', strokeWidth: 1.4, strokeLinecap: 'round', strokeLinejoin: 'round' }, [h('path', { key: 'a', d: 'M12 17v5', fill: 'none' }), h('path', { key: 'b', d: 'M9 3h6l-1 6 3 3v1H7v-1l3-3-1-6z' })]),
};

/* ============================ SETTINGS POPOVER ============================ */
const ACCENT_OPTS = ['#5B8FCB', '#4FB6C4', '#C99A4B', '#5FA47A'];
function SettingsPopover({ t, setTweak, onClose }) {
  return h('div', { className: 'pop-overlay', onMouseDown: onClose },
    h('div', { className: 'settings-pop', onMouseDown: (e) => e.stopPropagation() }, [
      h('div', { key: 'h', className: 'set-head' }, [
        h('span', { key: 't', className: 'sh-t' }, 'Settings'),
        h('span', { key: 'x', className: 'panel-collapse', onClick: onClose }, h(UI.x)),
      ]),
      h('div', { key: 'b', className: 'set-body' }, [
        h('div', { key: 'acc', className: 'set-row' }, [
          h('div', { key: 'l', className: 'set-label' }, 'Signal accent'),
          h('div', { key: 's', className: 'swatches' }, ACCENT_OPTS.map(c =>
            h('div', { key: c, className: 'swatch' + (t.accent === c ? ' on' : ''), style: { background: c }, onClick: () => setTweak('accent', c) }))),
        ]),
        h('div', { key: 'cur', className: 'set-row' }, [
          h('div', { key: 'l', className: 'set-label' }, 'Connection lines'),
          h('div', { key: 's', className: 'seg' }, ['bezier', 'ortho', 'straight'].map(o =>
            h('button', { key: o, className: t.curve === o ? 'on' : '', onClick: () => setTweak('curve', o) }, o))),
        ]),
        h('div', { key: 'grid', className: 'set-row' }, [
          h('div', { key: 'l', className: 'set-label' }, 'Canvas grid'),
          h('div', { key: 's', className: 'seg' }, ['lines', 'dots', 'off'].map(o =>
            h('button', { key: o, className: t.grid === o ? 'on' : '', onClick: () => setTweak('grid', o) }, o))),
        ]),
        ...[['showActivity', 'Signal flow animation'], ['grain', 'Film grain'], ['compactNodes', 'Compact nodes']].map(([k, label]) =>
          h('div', { key: k, className: 'set-row' },
            h('div', { className: 'set-toggle', onClick: () => setTweak(k, !t[k]) }, [
              h('span', { key: 'l', className: 'st-l' }, label),
              h('span', { key: 's', className: 'tg-switch' + (t[k] ? ' on' : '') }),
            ]))),
      ]),
    ]));
}

/* ============================ TOOLBAR ============================ */
function Toolbar({ live, onToggleLive, scenes, activeId, onSwitchScene, onNewScene, onCloseScene, onRenameScene, t, setTweak, onImport, onExport }) {
  const [setOpen, setSetOpen] = _uS(false);
  const [editing, setEditing] = _uS(null);

  return h('div', { className: 'toolbar' }, [
    h('div', { key: 'brand', className: 'brand' }, [
      h('span', { key: 'm', className: 'brand-mark' }),
      h('span', { key: 'n', className: 'brand-name' }, 'NODUS'),
    ]),
    h('div', { key: 'd1', className: 'tb-divider' }),

    // scene tabs
    h('div', { key: 'tabs', className: 'scene-tabs' }, scenes.map(s =>
      h('div', { key: s.id, className: 'scene-tab' + (s.id === activeId ? ' active' : ''), onMouseDown: (e) => { if (e.target.tagName !== 'INPUT') onSwitchScene(s.id); }, onDoubleClick: () => setEditing(s.id) }, [
        h('span', { key: 'd', className: 'st-dot' }),
        editing === s.id
          ? h('input', { key: 'i', autoFocus: true, defaultValue: s.name, onBlur: (e) => { onRenameScene(s.id, e.target.value || s.name); setEditing(null); }, onKeyDown: (e) => { if (e.key === 'Enter') e.target.blur(); } })
          : h('span', { key: 'n', className: 'st-name' }, s.name),
        scenes.length > 1 ? h('span', { key: 'x', className: 'scene-close', onMouseDown: (e) => { e.stopPropagation(); onCloseScene(s.id); } }, h(UI.x)) : null,
      ]))),
    h('div', { key: 'new', className: 'scene-new', onClick: onNewScene, title: 'New scene' }, h(UI.plus)),

    h('div', { key: 'spacer', className: 'tb-spacer' }),

    // settings / import / export
    h('div', { key: 'set', className: 'set-wrap' }, [
      h('div', { key: 'b', className: 'tb-btn' + (setOpen ? ' active' : ''), onClick: () => setSetOpen(o => !o) }, [h(UI.settings, { key: 'i' }), h('span', { key: 't' }, 'SETTINGS')]),
      setOpen ? h(SettingsPopover, { key: 'p', t, setTweak, onClose: () => setSetOpen(false) }) : null,
    ]),
    h('div', { key: 'imp', className: 'tb-btn', onClick: onImport }, [h(UI.import, { key: 'i' }), h('span', { key: 't' }, 'IMPORT')]),
    h('div', { key: 'exp', className: 'tb-btn', onClick: onExport }, [h(UI.export, { key: 'i' }), h('span', { key: 't' }, 'EXPORT')]),

    h('div', { key: 'd2', className: 'tb-divider' }),
    h('div', { key: 'live', className: 'tb-live' + (live ? ' on' : '') }, [
      h('span', { key: 'd', className: 'live-dot' }),
      h('span', { key: 't' }, live ? 'LIVE' : 'IDLE'),
    ]),
    h('div', { key: 'eng', className: 'tb-btn' + (live ? ' active' : ''), onClick: onToggleLive }, [h(live ? UI.stop : UI.play, { key: 'i' }), h('span', { key: 't' }, live ? 'STOP' : 'ENGAGE')]),
  ]);
}

/* ============================ LIBRARY ============================ */
function devType(io, dev) {
  if (io === 'in') return dev.kind === 'app' ? 'app' : 'source';
  return 'output';
}

function Library({ onCollapse }) {
  const [tab, setTab] = _uS('devices');
  const [q, setQ] = _uS('');
  const [collapsed, setCollapsed] = _uS({});
  const [scanning, setScanning] = _uS(false);
  const toggle = (id) => setCollapsed(c => ({ ...c, [id]: !c[id] }));
  const ql = q.trim().toLowerCase();

  const rescan = () => { setScanning(true); setTimeout(() => setScanning(false), 850); };

  const devItem = (io) => (d, i) => h('div', {
    key: d.name + i, className: 'lib-item', draggable: true,
    onDragStart: (e) => { e.dataTransfer.setData('application/nodus-device', JSON.stringify({ io, ...d })); e.dataTransfer.effectAllowed = 'copy'; },
  }, [
    h('div', { key: 'g', className: 'lib-glyph' }, h(Icons[d.kind === 'app' ? 'app' : (io === 'out' ? 'output' : 'source')])),
    h('div', { key: 't', className: 'lib-item-text' }, [
      h('div', { key: 'n', className: 'lib-item-name' }, d.name),
      h('div', { key: 'k', className: 'lib-item-kind' }, d.device),
    ]),
    d.kind === 'app' ? h('div', { key: 's', className: 'dev-status' + (d.running ? ' running' : '') }, [h('span', { key: 'd', className: 'ds-dot' }), d.running ? 'running' : 'idle']) : null,
  ]);

  const cat = (id, label, items) => {
    if (!items.length) return null;
    const isCol = collapsed[id];
    return h('div', { key: id, className: 'lib-cat' + (isCol ? ' collapsed' : '') }, [
      h('div', { key: 'h', className: 'lib-cat-head', onClick: () => toggle(id) }, [
        h('span', { key: 'c', className: 'chev' }, h(UI.chev)),
        h('span', { key: 'l' }, label),
        h('span', { key: 'n', style: { marginLeft: 'auto', color: 'var(--text-mute)' } }, items.length),
      ]),
      h('div', { key: 'i', className: 'lib-items' }, items),
    ]);
  };

  const inputs = INPUT_DEVICES.filter(d => !ql || d.name.toLowerCase().includes(ql));
  const outputs = OUTPUT_DEVICES.filter(d => !ql || d.name.toLowerCase().includes(ql));
  const virtuals = VIRTUAL_TEMPLATES.filter(d => !ql || d.name.toLowerCase().includes(ql));

  return h('div', { className: 'panel' }, [
    h('div', { key: 'head', className: 'panel-head' }, [
      h('span', { key: 't', className: 'panel-title' }, 'Library'),
      h('span', { key: 'c', className: 'panel-collapse', onClick: onCollapse, title: 'Collapse' }, h(UI.panelL)),
    ]),
    h('div', { key: 'tabs', className: 'lib-tabs' }, [
      h('div', { key: 'd', className: 'lib-tab' + (tab === 'devices' ? ' on' : ''), onClick: () => setTab('devices') }, 'Devices'),
      h('div', { key: 'n', className: 'lib-tab' + (tab === 'nodes' ? ' on' : ''), onClick: () => setTab('nodes') }, 'Nodes'),
    ]),
    h('div', { key: 'search', className: 'lib-search' }, [
      h(UI.search, { key: 'i' }),
      h('input', { key: 'in', placeholder: tab === 'devices' ? 'filter devices…' : 'filter nodes…', value: q, onChange: (e) => setQ(e.target.value) }),
    ]),
    h('div', { key: 'body', className: 'panel-body scroll' }, tab === 'devices' ? [
      h('div', { key: 'rescan', className: 'lib-rescan' + (scanning ? ' spin' : ''), onClick: rescan }, [h(UI.refresh, { key: 'i' }), scanning ? 'scanning devices…' : 'rescan devices']),
      cat('audio-in', 'Audio Input', inputs.map(devItem('in'))),
      cat('audio-out', 'Audio Output', outputs.map(devItem('out'))),
    ] : [
      ...NODE_CATEGORIES.map(c => cat(c.id, c.label, NODE_TEMPLATES.filter(t => t.cat === c.id && (!ql || t.name.toLowerCase().includes(ql) || t.type.includes(ql))).map((t, i) =>
        h('div', { key: t.type + i, className: 'lib-item', draggable: true, onDragStart: (e) => { e.dataTransfer.setData('application/nodus-node', t.type); e.dataTransfer.effectAllowed = 'copy'; } }, [
          h('div', { key: 'g', className: 'lib-glyph' }, h(Icons[t.glyph] || Icons.effect)),
          h('div', { key: 't', className: 'lib-item-text' }, [
            h('div', { key: 'n', className: 'lib-item-name' }, t.name),
            h('div', { key: 'k', className: 'lib-item-kind' }, t.cat === 'route' ? t.type : t.cat),
          ]),
        ])))),
      cat('virtual', 'Virtual Devices', virtuals.map(devItem('out'))),
    ]),
  ]);
}

/* ============================ INSPECTOR ============================ */
function Inspector({ node, edge, multi, nodes, edges, onCollapse, onRename, onVolume, onParam, onMute, onSolo, onDuplicate, onDelete, onDeleteEdge, onEdgeVol, onSelectNode, onAddPort, onRemovePort, onDeleteSelection, onMuteSelection, isPinned, onTogglePin }) {
  const head = (title, right) => h('div', { key: 'head', className: 'panel-head' }, [
    h('span', { key: 't', className: 'panel-title' }, title),
    h('span', { key: 'a', className: 'panel-head-actions' }, [
      right ? h('span', { key: 'r', className: 'panel-title', style: { letterSpacing: '0.1em', color: 'var(--text-mute)' } }, right) : null,
      h('span', { key: 'c', className: 'panel-collapse', onClick: onCollapse, title: 'Collapse' }, h(UI.panelL)),
    ]),
  ]);

  if (multi && multi.length > 1) {
    return h('div', { className: 'panel' }, [
      head('Selection', multi.length + ' NODES'),
      h('div', { key: 'b', className: 'panel-body scroll' }, [
        h('div', { key: 's', className: 'insp-sec' }, [
          h('div', { key: 't', className: 'insp-sec-title' }, multi.length + ' nodes selected'),
          ...multi.slice(0, 10).map(n => h('div', { key: n.id, className: 'conn-row', onClick: () => onSelectNode(n.id), style: { cursor: 'pointer' } }, [
            h('span', { key: 'd', className: 'cr-dir', style: { width: 'auto' } }, '·'),
            h('span', { key: 'n', className: 'cr-name' }, n.name),
          ])),
          multi.length > 10 ? h('div', { key: 'more', className: 'conn-row', style: { color: 'var(--text-mute)' } }, `+ ${multi.length - 10} more`) : null,
        ]),
        h('div', { key: 'hint', className: 'insp-sec', style: { borderBottom: 'none' } },
          h('div', { className: 'qc-empty', style: { lineHeight: 1.6 } }, 'Drag any selected node to move the group · press Delete to remove all.')),
        h('div', { key: 'm', className: 'insp-actions' }, [
          h('div', { key: 'mu', className: 'insp-btn', onClick: () => onMuteSelection(true) }, 'MUTE ALL'),
          h('div', { key: 'un', className: 'insp-btn', onClick: () => onMuteSelection(false) }, 'UNMUTE ALL'),
        ]),
        h('div', { key: 'd', className: 'insp-actions', style: { paddingTop: 0 } },
          h('div', { className: 'insp-btn danger', onClick: onDeleteSelection }, 'DELETE SELECTED')),
      ]),
    ]);
  }

  if (edge && !node) {
    const fn = nodes[edge.from.node], tn = nodes[edge.to.node];
    return h('div', { className: 'panel' }, [
      head('Connection'),
      h('div', { key: 'b', className: 'panel-body scroll' }, [
        h('div', { key: 's', className: 'insp-sec' }, [
          h('div', { key: 't', className: 'insp-sec-title' }, 'Route'),
          h('div', { key: 'f', className: 'conn-row' }, [h('span', { key: 'd', className: 'cr-name' }, fn ? fn.name : '?'), h('span', { key: 'a', className: 'cr-arrow' }, '→'), h('span', { key: 'n', className: 'cr-name' }, tn ? tn.name : '?')]),
        ]),
        h('div', { key: 'v', className: 'insp-sec' }, [
          h('div', { key: 't', className: 'insp-sec-title' }, 'Send level'),
          h('div', { key: 'sf', className: 'slider-field' }, [
            h('div', { key: 'top', className: 'sf-top' }, [h('span', { key: 'l', className: 'sf-label' }, 'volume'), h('span', { key: 'v', className: 'sf-val' }, (edge.vol ?? 100) + '%')]),
            h('input', { key: 'r', type: 'range', className: 'range', min: 0, max: 100, value: edge.vol ?? 100, onChange: (e) => onEdgeVol(edge.id, +e.target.value) }),
          ]),
        ]),
        h('div', { key: 'a', className: 'insp-actions' }, h('div', { className: 'insp-btn danger', onClick: () => onDeleteEdge(edge.id) }, 'DELETE ROUTE')),
      ]),
    ]);
  }

  if (!node) {
    return h('div', { className: 'panel' }, [
      head('Inspector'),
      h('div', { key: 'e', className: 'panel-body insp-empty' }, [
        h('div', { key: 'm', className: 'ie-mark' }),
        h('p', { key: 'p' }, 'No node selected.\nSelect a node to inspect routing, levels and parameters.'),
      ]),
    ]);
  }

  const m = meta(node.type);
  const Glyph = Icons[node.glyph] || Icons.source;
  const ins = edges.filter(e => e.to.node === node.id);
  const outs = edges.filter(e => e.from.node === node.id);

  const paramFields = [];
  if (node.type === 'gate') paramFields.push(['threshold', node.params.threshold, 'dB', -80, 0]);
  if (node.type === 'comp') paramFields.push(['ratio', node.params.ratio, ': 1', 1, 20], ['threshold', node.params.threshold, 'dB', -60, 0]);
  if (node.type === 'limiter') paramFields.push(['ceiling', node.params.ceiling, 'dB', -12, 0]);
  if (node.type === 'duck') paramFields.push(['duck', node.params.duck, '%', 0, 100], ['attack', node.params.attack, 'ms', 1, 80], ['release', node.params.release, 'ms', 20, 600]);

  return h('div', { className: 'panel' }, [
    head('Inspector', node.id.toUpperCase()),
    h('div', { key: 'b', className: 'panel-body scroll' }, [
      h('div', { key: 'hero', className: 'insp-hero' }, [
        h('div', { key: 'g', className: 'insp-hero-glyph' }, h(Glyph)),
        h('div', { key: 't', className: 'insp-hero-txt' }, [
          h('div', { key: 'n', className: 'insp-hero-name' }, h('input', { value: node.name, onChange: (e) => onRename(node.id, e.target.value) })),
          h('div', { key: 'k', className: 'insp-hero-kind' }, node.type + ' · ' + node.cat),
        ]),
      ]),

      // device — ONLY for input/output nodes
      m.device ? h('div', { key: 'meta', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, 'Device'),
        h('div', { key: 'd', className: 'kv' }, [h('span', { key: 'k', className: 'kv-k' }, 'binding'), h('span', { key: 'v', className: 'kv-v' }, node.device || '—')]),
        h('div', { key: 's', className: 'kv' }, [h('span', { key: 'k', className: 'kv-k' }, 'state'), h('span', { key: 'v', className: 'kv-v ' + (node.muted ? '' : 'acc') }, node.muted ? 'muted' : (node.solo ? 'solo' : 'active'))]),
      ]) : null,

      // volume / gain (input + gain node)
      m.vol ? h('div', { key: 'lvl', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, m.vol === 'gain' ? 'Gain' : 'Volume'),
        h('div', { key: 's', className: 'slider-field' }, [
          h('div', { key: 'top', className: 'sf-top' }, [h('span', { key: 'l', className: 'sf-label' }, m.vol === 'gain' ? 'gain' : 'volume'), h('span', { key: 'v', className: 'sf-val' }, (node.volume ?? 80) + '%')]),
          h('input', { key: 'r', type: 'range', className: 'range', min: 0, max: 100, value: node.volume ?? 80, onChange: (e) => onVolume(node.id, +e.target.value) }),
        ]),
      ]) : null,

      // per-incoming-input volumes (mixer + output)
      m.perEdge === 'in' ? h('div', { key: 'pe', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, `Input levels · ${ins.length}`),
        ins.length === 0 ? h('div', { key: 'e', className: 'conn-row', style: { color: 'var(--text-mute)' } }, 'no inputs connected') : null,
        ...ins.map(e => { const lbl = channelLabel(nodes, edges, e.from.node, 'src'); return h('div', { key: e.id, className: 'slider-field' }, [
          h('div', { key: 'top', className: 'sf-top' }, [h('span', { key: 'l', className: 'sf-label', title: lbl.title }, lbl.sub ? (lbl.name + ' · ' + lbl.sub) : lbl.name), h('span', { key: 'v', className: 'sf-val' }, (e.vol ?? 100) + '%')]),
          h('input', { key: 'r', type: 'range', className: 'range', min: 0, max: 100, value: e.vol ?? 100, onChange: (ev) => onEdgeVol(e.id, +ev.target.value) }),
        ]); }),
      ]) : null,

      // per-output volumes (splitter)
      m.perEdge === 'out' ? h('div', { key: 'po', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, `Output levels · ${outs.length}`),
        outs.length === 0 ? h('div', { key: 'e', className: 'conn-row', style: { color: 'var(--text-mute)' } }, 'no outputs connected') : null,
        ...outs.map(e => { const lbl = channelLabel(nodes, edges, e.to.node, 'sink'); return h('div', { key: e.id, className: 'slider-field' }, [
          h('div', { key: 'top', className: 'sf-top' }, [h('span', { key: 'l', className: 'sf-label', title: lbl.title }, lbl.sub ? (lbl.name + ' · ' + lbl.sub) : lbl.name), h('span', { key: 'v', className: 'sf-val' }, (e.vol ?? 100) + '%')]),
          h('input', { key: 'r', type: 'range', className: 'range', min: 0, max: 100, value: e.vol ?? 100, onChange: (ev) => onEdgeVol(e.id, +ev.target.value) }),
        ]); }),
      ]) : null,

      // params
      paramFields.length ? h('div', { key: 'prm', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, node.type === 'duck' ? 'Ducking' : 'Parameters'),
        ...paramFields.map(([k, v, unit, min, max]) => h('div', { key: k, className: 'slider-field' }, [
          h('div', { key: 'top', className: 'sf-top' }, [h('span', { key: 'l', className: 'sf-label' }, k), h('span', { key: 'v', className: 'sf-val' }, v + ' ' + unit)]),
          h('input', { key: 'r', type: 'range', className: 'range', min, max, value: v, onChange: (e) => onParam(node.id, k, +e.target.value) }),
        ])),
      ]) : null,

      // trigger params
      node.type === 'trigger' ? h('div', { key: 'tg', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, 'Trigger'),
        h('div', { key: 'k', className: 'kv' }, [h('span', { key: 'k', className: 'kv-k' }, 'hotkey'), h('span', { key: 'v', className: 'kv-v acc' }, '[ ' + node.params.key + ' ]')]),
        h('div', { key: 'm', className: 'kv' }, [h('span', { key: 'k', className: 'kv-k' }, 'mode'), h('span', { key: 'v', className: 'kv-v' }, node.params.mode)]),
      ]) : null,

      // ports management (mixer/splitter/output) — ports are CREATED only by
      // dragging a connection to the canvas "+"; here you can review / remove them
      (m.addIn || m.addOut) ? h('div', { key: 'ports', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, 'Ports'),
        m.addIn ? h('div', { key: 'in', style: { marginBottom: 8 } }, [
          h('div', { key: 'l', className: 'sf-label', style: { marginBottom: 6 } }, `inputs · ${node.in.length}`),
          h('div', { key: 'c', className: 'port-chips' },
            node.in.map(p => h('div', { key: p.id, className: 'port-chip' }, [h('span', { key: 'l' }, p.label), node.in.length > 1 ? h('span', { key: 'x', className: 'pc-x', title: 'Remove port', onClick: () => onRemovePort(node.id, 'in', p.id) }, h(UI.x)) : null]))),
        ]) : null,
        m.addOut ? h('div', { key: 'out' }, [
          h('div', { key: 'l', className: 'sf-label', style: { marginBottom: 6 } }, `outputs · ${node.out.length}`),
          h('div', { key: 'c', className: 'port-chips' },
            node.out.map(p => h('div', { key: p.id, className: 'port-chip' }, [h('span', { key: 'l' }, p.label), node.out.length > 1 ? h('span', { key: 'x', className: 'pc-x', title: 'Remove port', onClick: () => onRemovePort(node.id, 'out', p.id) }, h(UI.x)) : null]))),
        ]) : null,
      ]) : null,

      // routing list
      h('div', { key: 'route', className: 'insp-sec' }, [
        h('div', { key: 't', className: 'insp-sec-title' }, `Routing · ${ins.length} in / ${outs.length} out`),
        ins.length === 0 && outs.length === 0 ? h('div', { key: 'none', className: 'conn-row', style: { color: 'var(--text-mute)' } }, 'no connections') : null,
        ...ins.map(e => { const o = nodes[e.from.node]; return h('div', { key: 'i' + e.id, className: 'conn-row', onClick: () => o && onSelectNode(o.id), style: { cursor: 'pointer' } }, [h('span', { key: 'd', className: 'cr-dir' }, 'in'), h('span', { key: 'a', className: 'cr-arrow' }, '←'), h('span', { key: 'n', className: 'cr-name' }, o ? o.name : '?')]); }),
        ...outs.map(e => { const o = nodes[e.to.node]; return h('div', { key: 'o' + e.id, className: 'conn-row', onClick: () => o && onSelectNode(o.id), style: { cursor: 'pointer' } }, [h('span', { key: 'd', className: 'cr-dir' }, 'out'), h('span', { key: 'a', className: 'cr-arrow' }, '→'), h('span', { key: 'n', className: 'cr-name' }, o ? o.name : '?')]); }),
      ]),

      h('div', { key: 'act', className: 'insp-actions' }, [
        h('div', { key: 'm', className: 'insp-btn', onClick: () => onMute(node.id) }, node.muted ? 'UNMUTE' : 'MUTE'),
        h('div', { key: 'd', className: 'insp-btn', onClick: () => onDuplicate(node.id) }, 'DUPLICATE'),
      ]),
      h('div', { key: 'pin', className: 'insp-actions', style: { paddingTop: 0 } },
        h('div', { className: 'insp-btn' + (isPinned ? ' on' : ''), onClick: () => onTogglePin(node.id) }, [
          h(isPinned ? UI.pinOn : UI.pin, { key: 'i' }),
          h('span', { key: 't' }, isPinned ? 'PINNED TO QUICK CONTROLS' : 'PIN TO QUICK CONTROLS'),
        ])),
      h('div', { key: 'act2', className: 'insp-actions', style: { paddingTop: 0 } }, h('div', { className: 'insp-btn danger', onClick: () => onDelete(node.id) }, 'DELETE NODE')),
    ]),
  ]);
}

/* ============================ BOTTOM DOCK — QUICK CONTROLS ============================ */
/* ============================ BOTTOM DOCK — QUICK CONTROLS ============================ */
/* A fast, filterable channel mixer for operating ANY node in a large scene without
   hunting on the canvas: category quick-filters + search across the whole scene,
   horizontal level sliders, inline mute/solo, pin favourites, click name to reveal. */
function BottomDock({ open, onToggle, pinned, nodes, edges, allNodes, onUnpin, onPin, onFocus, onEdgeVol, onVolume, onMute, onSolo }) {
  const [q, setQ] = _uS('');
  const [cat, setCat] = _uS('pinned');
  const stopP = (e) => e.stopPropagation();
  const ql = q.trim().toLowerCase();
  const pinnedIds = new Set(pinned.map(p => p.id));

  const CATS = [
    { id: 'pinned', label: '★ Pinned' },
    { id: 'input', label: 'Sources' },
    { id: 'output', label: 'Outputs' },
    { id: 'route', label: 'Routing' },
    { id: 'effect', label: 'Effects' },
    { id: 'logic', label: 'Logic' },
  ];


  const chRow = (c, muted) => h('div', { key: c.key, className: 'qch' }, [
    h('div', { key: 'n', className: 'qch-label', title: c.title || c.name }, [
      h('span', { key: 'm', className: 'qch-name' }, c.name),
      c.sub ? h('span', { key: 's', className: 'qch-via' }, c.sub) : null,
    ]),
    h('input', { key: 's', type: 'range', min: 0, max: 100, value: c.value, className: 'qch-slider',
      style: { background: `linear-gradient(90deg, ${muted ? 'var(--text-mute)' : 'var(--accent)'} ${c.value}%, var(--border-2) ${c.value}%)` },
      onMouseDown: stopP, onChange: (e) => c.onChange(+e.target.value) }),
    h('span', { key: 'v', className: 'qch-val' }, c.value + '%'),
  ]);

  const strip = (n) => {
    const m = meta(n.type);
    let channels = [];
    if (m.vol) channels.push({ key: 'vol', name: m.vol === 'gain' ? 'Gain' : 'Volume', value: n.volume ?? 80, onChange: (v) => onVolume(n.id, v) });
    if (m.perEdge === 'in') edges.filter(e => e.to.node === n.id).forEach(e => {
      const lbl = channelLabel(nodes, edges, e.from.node, 'src');
      channels.push({ key: e.id, name: lbl.name, sub: lbl.sub, title: lbl.title, value: e.vol ?? 100, onChange: (v) => onEdgeVol(e.id, v) });
    });
    if (m.perEdge === 'out') edges.filter(e => e.from.node === n.id).forEach(e => {
      const lbl = channelLabel(nodes, edges, e.to.node, 'sink');
      channels.push({ key: e.id, name: lbl.name, sub: lbl.sub, title: lbl.title, value: e.vol ?? 100, onChange: (v) => onEdgeVol(e.id, v) });
    });

    const nodeMatch = !ql || n.name.toLowerCase().includes(ql) || n.type.includes(ql);
    if (ql && !nodeMatch) { channels = channels.filter(c => (c.name + ' ' + (c.sub || '')).toLowerCase().includes(ql)); if (!channels.length) return null; }

    const canMute = n.type !== 'trigger';
    const canSolo = n.type !== 'trigger' && m.io !== 'out';
    const isPinned = pinnedIds.has(n.id);

    return h('div', { key: n.id, className: 'qstrip' + (n.muted ? ' muted' : '') }, [
      h('div', { key: 'h', className: 'qstrip-head' }, [
        h('span', { key: 'g', className: 'qstrip-glyph' }, h(Icons[n.glyph] || Icons.source)),
        h('span', { key: 'n', className: 'qstrip-name', onClick: () => onFocus(n.id), title: 'Reveal on canvas' }, n.name),
        canMute ? h('span', { key: 'm', className: 'qstrip-btn mute' + (n.muted ? ' on' : ''), title: 'Mute', onClick: () => onMute(n.id) }, 'M') : null,
        canSolo ? h('span', { key: 's', className: 'qstrip-btn solo' + (n.solo ? ' on' : ''), title: 'Solo', onClick: () => onSolo(n.id) }, 'S') : null,
        h('span', { key: 'p', className: 'qstrip-pin' + (isPinned ? ' on' : ''), title: isPinned ? 'Unpin' : 'Pin', onClick: () => isPinned ? onUnpin(n.id) : onPin(n.id) }, h(isPinned ? UI.pinOn : UI.pin)),
      ]),
      channels.length
        ? h('div', { key: 'c', className: 'qstrip-chs' }, channels.map(c => chRow(c, n.muted)))
        : h('div', { key: 'e', className: 'qstrip-empty' }, n.type === 'trigger' ? ('hotkey · hold [ ' + n.params.key + ' ]') : (m.perEdge === 'in' ? 'no inputs connected' : (m.perEdge === 'out' ? 'no outputs connected' : 'mute / solo'))),
    ]);
  };

  // source list depends on active category (pinned favourites, or all nodes of a category)
  const base = cat === 'pinned' ? pinned : allNodes.filter(n => meta(n.type).cat === cat);
  const strips = base.map(strip).filter(Boolean);

  const emptyMsg = cat === 'pinned'
    ? (ql ? 'No pinned channels match “' + q + '”.' : 'No nodes pinned yet. Pick a category above to operate any node, or pin favourites with the ☆ on a strip.')
    : (ql ? 'No channels match “' + q + '”.' : 'No nodes in this category.');

  return h('div', { className: 'dock' + (open ? '' : ' collapsed') }, [
    h('div', { key: 'h', className: 'dock-head' }, [
      h('span', { key: 't', className: 'dh-title', onClick: onToggle, style: { cursor: 'pointer' } }, 'Quick Controls'),
      open ? h('div', { key: 'cats', className: 'qc-cats', onMouseDown: stopP }, CATS.map(c =>
        h('button', { key: c.id, className: 'qc-cat' + (cat === c.id ? ' on' : ''), onClick: () => setCat(c.id) }, [
          c.label, c.id === 'pinned' && pinned.length ? h('span', { key: 'n', className: 'qc-cat-n' }, pinned.length) : null,
        ]))) : null,
      open ? h('div', { key: 'f', className: 'qc-filter', onMouseDown: stopP }, [
        h(UI.search, { key: 'i' }),
        h('input', { key: 'in', placeholder: 'search scene…', value: q, onChange: (e) => setQ(e.target.value) }),
        q ? h('span', { key: 'x', className: 'qc-filter-x', onClick: () => setQ('') }, h(UI.x)) : null,
      ]) : null,
      h('span', { key: 'sp', style: { flex: 1 } }),
      h('span', { key: 'c', className: 'dh-chev', onClick: onToggle, style: { cursor: 'pointer' } }, h(UI.chev)),
    ]),
    open ? h('div', { key: 'b', className: 'qc-list scroll' },
      strips.length ? strips : h('div', { key: 'empty', className: 'qc-dock-empty' }, emptyMsg)) : null,
  ]);
}

/* ============================ STATUS BAR ============================ */
function StatusBar({ view, setView, onFit, nodeCount, edgeCount, live, activeCount, pttActive }) {
  const setZoom = (z) => setView(v => ({ ...v, zoom: Math.max(0.3, Math.min(2.2, z)) }));
  return h('div', { className: 'statusbar' }, [
    h('div', { key: 'p', className: 'sb-item' }, [h('span', { key: 'k', className: 'sb-k' }, 'PROJECT'), h('span', { key: 'v', className: 'sb-v' }, 'nodus')]),
    h('div', { key: 's1', className: 'sb-sep' }),
    h('div', { key: 'n', className: 'sb-item' }, [h('span', { key: 'k', className: 'sb-k' }, 'NODES'), h('span', { key: 'v', className: 'sb-v' }, nodeCount)]),
    h('div', { key: 'r', className: 'sb-item' }, [h('span', { key: 'k', className: 'sb-k' }, 'ROUTES'), h('span', { key: 'v', className: 'sb-v' }, edgeCount)]),
    h('div', { key: 's2', className: 'sb-sep' }),
    h('div', { key: 'sig', className: 'sb-item' }, [
      h('span', { key: 'd', className: 'sb-dot ' + (live ? 'ok' : 'idle') }),
      h('span', { key: 'v', className: 'sb-v' + (live ? ' acc' : '') }, live ? `${activeCount} active paths` : 'idle'),
    ]),
    pttActive ? h('div', { key: 'ptt', className: 'sb-item' }, [h('span', { key: 'k', className: 'sb-k' }, 'PTT'), h('span', { key: 'v', className: 'sb-v', style: { color: 'var(--solo)' } }, 'DUCKING')]) : null,
    h('div', { key: 'sp', className: 'sb-spacer' }),
    h('div', { key: 'k', className: 'sb-item' }, [h('span', { key: 'kk', className: 'sb-k' }, 'PTT KEY'), h('span', { key: 'v', className: 'sb-v' }, 'hold V')]),
    h('div', { key: 's3', className: 'sb-sep' }),
    h('div', { key: 'zoom', className: 'sb-item zoomctl' }, [
      h('button', { key: '-', onClick: () => setZoom(view.zoom - 0.15) }, h(UI.minus)),
      h('span', { key: 'l', className: 'zlabel', onClick: () => setView(v => ({ ...v, zoom: 1 })) }, Math.round(view.zoom * 100) + '%'),
      h('button', { key: '+', onClick: () => setZoom(view.zoom + 0.15) }, h(UI.plus)),
      h('button', { key: 'f', onClick: onFit, title: 'fit' }, h(UI.fit)),
    ]),
  ]);
}

/* ============================ COLLAPSE RAILS ============================ */
function CollapseRail({ side, label, onExpand }) {
  return h('div', { className: 'rail' }, [
    h('div', { key: 'b', className: 'rail-btn', onClick: onExpand, title: 'Expand' }, h(UI.panelL)),
    h('div', { key: 'l', className: 'rail-label' }, label),
  ]);
}

Object.assign(window, { Toolbar, Library, Inspector, BottomDock, StatusBar, CollapseRail, UI });
