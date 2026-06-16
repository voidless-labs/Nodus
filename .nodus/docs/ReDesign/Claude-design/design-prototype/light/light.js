/* nodus light v2 — vanilla js (functional pass) */
(function () {
  'use strict';

  /* ---------- icons ---------- */
  const I = {
    mic: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="9" y="3.5" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3"/></svg>',
    phones: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M5 14v-2a7 7 0 0 1 14 0v2"/><rect x="4" y="13.5" width="3.6" height="6" rx="1.6" fill="currentColor" stroke="none"/><rect x="16.4" y="13.5" width="3.6" height="6" rx="1.6" fill="currentColor" stroke="none"/></svg>',
    cam: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="6.5" width="13" height="11" rx="2.5"/><path d="M16 10.5 21 8v8l-5-2.5"/></svg>',
    spark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v4M12 16v4M4 12h4M16 12h4"/><circle cx="12" cy="12" r="2.4" fill="currentColor" stroke="none"/></svg>',
    mix: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 7h6M14 7h6M4 12h10M18 12h2M4 17h4M12 17h8"/><circle cx="12" cy="7" r="2" fill="currentColor" stroke="none"/><circle cx="16" cy="12" r="2" fill="currentColor" stroke="none"/><circle cx="10" cy="17" r="2" fill="currentColor" stroke="none"/></svg>',
    muteOn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5H4z"/><path d="M16 9l5 6M21 9l-5 6"/></svg>',
    muteOff: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5H4z"/><path d="M15.5 9.5a4 4 0 0 1 0 5"/></svg>',
    cursor: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M5 3l15 8-6 1.6L11 19 5 3z"/></svg>',
  };

  /* ---------- option lists for the processor dropdowns ---------- */
  const PARAM_OPTS = {
    'sample rate': ['44.1 kHz', '48 kHz', '96 kHz', '192 kHz'],
    'limiter':     ['off', '−1 dB', '−3 dB', '−6 dB'],
    'mode':        ['stereo', 'mono', '5.1 surround'],
  };

  /* ---------- scene ---------- */
  const NODES = [
    { id: 'mic',     type: 'source',  label: 'mic',       name: 'Microphone', sub: 'Shure SM7B',     icon: I.mic,   x: 70,  y: 120, vol: 74, editor: 'paul' },
    { id: 'spotify', type: 'source',  label: 'source',    name: 'Spotify',    sub: 'playing · app',  glyph: 'S',    x: 50,  y: 360, vol: 55 },
    { id: 'game',    type: 'source',  label: 'source',    name: 'Arma 3',     sub: 'running · app',  glyph: 'A',    x: 70,  y: 600, vol: 88, editor: 'mario' },
    { id: 'mix',     type: 'processor', label: 'mixer',   name: 'Stream Mix', sub: 'routing engine', icon: I.mix,   x: 470, y: 285, wide: true, editor: 'kate',
      ports: [['mic','c-source'],['music','c-source'],['game','c-source']],
      params: [['sample rate','48 kHz'],['limiter','−1 dB'],['mode','stereo']] },
    { id: 'phones',  type: 'output',  label: 'output',    name: 'Headphones', sub: 'Galaxy Buds',    icon: I.phones, x: 890, y: 110, vol: 100 },
    { id: 'obs',     type: 'output',  label: 'output',    name: 'OBS',        sub: 'stream output',  icon: I.cam,   x: 915, y: 350, vol: 100 },
    { id: 'vmic',    type: 'virtual', label: 'virtual',   name: 'Nodus Mic',  sub: 'for Discord',    icon: I.spark, x: 890, y: 590, vol: 100 },
  ];
  let EDGES = [
    { from: 'mic',     fp: 'out', to: 'mix',    tp: 'mic' },
    { from: 'spotify', fp: 'out', to: 'mix',    tp: 'music' },
    { from: 'game',    fp: 'out', to: 'mix',    tp: 'game' },
    { from: 'mix',     fp: 'out', to: 'phones', tp: 'in' },
    { from: 'mix',     fp: 'out', to: 'obs',    tp: 'in' },
    { from: 'mix',     fp: 'out', to: 'vmic',   tp: 'in' },
  ];
  const CURSORS = [
    { user: 'paul',  name: 'Paul',  x: 250, y: 260, color: 'var(--u-paul)' },
    { user: 'mario', name: 'Mario', x: 250, y: 740, color: 'var(--u-mario)' },
    { user: 'kate',  name: 'Kate',  x: 700, y: 470, color: 'var(--u-kate)' },
  ];

  const stage = document.getElementById('stage');
  const world = document.getElementById('world');
  const svg = document.getElementById('edgesSvg');
  const app = document.getElementById('lapp');

  const reg = {};
  let live = true, zoom = 1, panX = 0, panY = 0, selected = null, uid = 100;

  const css = (v) => getComputedStyle(document.documentElement).getPropertyValue(v).trim();

  /* ============ toast + menu primitives ============ */
  let toastWrap = document.querySelector('.toast-wrap');
  if (!toastWrap) { toastWrap = document.createElement('div'); toastWrap.className = 'toast-wrap'; document.body.appendChild(toastWrap); }
  function toast(msg, ms) {
    const t = document.createElement('div'); t.className = 'toast'; t.textContent = msg;
    toastWrap.appendChild(t);
    setTimeout(() => { t.style.opacity = '0'; t.style.transform = 'translateY(8px)'; setTimeout(() => t.remove(), 220); }, ms || 1700);
  }
  let openEl = null;
  function closeMenu() { if (openEl) { openEl.remove(); openEl = null; document.removeEventListener('mousedown', onDocDown, true); } }
  function onDocDown(e) { if (openEl && !openEl.contains(e.target)) closeMenu(); }
  function openMenu(anchor, items, opts) {
    closeMenu();
    opts = opts || {};
    const m = document.createElement('div'); m.className = 'menu';
    items.forEach(it => {
      if (it.sep) { const s = document.createElement('div'); s.className = 'menu-sep'; m.appendChild(s); return; }
      const r = document.createElement('div'); r.className = 'menu-item' + (it.muted ? ' muted' : '');
      r.innerHTML = `<span>${it.label}</span>` + (it.checked ? '<span class="mi-check">✓</span>' : (it.hint ? `<span class="mi-hint">${it.hint}</span>` : ''));
      r.addEventListener('click', (e) => { e.stopPropagation(); closeMenu(); it.onClick && it.onClick(); });
      m.appendChild(r);
    });
    document.body.appendChild(m);
    const ar = anchor.getBoundingClientRect();
    const mw = m.offsetWidth, mh = m.offsetHeight;
    let left = opts.right ? ar.right - mw : ar.left;
    let top = opts.up ? ar.top - mh - 6 : ar.bottom + 6;
    left = Math.max(8, Math.min(left, window.innerWidth - mw - 8));
    top = Math.max(8, Math.min(top, window.innerHeight - mh - 8));
    m.style.left = left + 'px'; m.style.top = top + 'px';
    openEl = m;
    setTimeout(() => document.addEventListener('mousedown', onDocDown, true), 0);
  }

  /* ============ build a node ============ */
  function buildNode(n) {
    const el = document.createElement('div');
    el.className = 'lnode glow' + (n.wide ? ' wide' : '');
    el.dataset.type = n.type; el.dataset.id = n.id;
    if (n.editor) el.dataset.editor = n.editor;
    el.style.transform = `translate(${n.x}px, ${n.y}px)`;

    let body = '', portsHtml = '';
    if (n.type === 'processor') {
      const inRows = n.ports.map(p => `<div class="prow"><span class="pdot" style="background:var(--${p[1]})"></span><span class="pk">${p[0]}</span></div>`).join('');
      const params = n.params.map(p => `<div class="param"><span class="pname">${p[0]}</span><button class="pctl" data-param="${p[0]}">${p[1]}<span class="chev">▾</span></button></div>`).join('');
      body =
        `<div class="ninner">${inRows}<div class="prow out"><span class="pk">mix</span><span class="pdot" style="background:var(--accent)"></span></div></div>` +
        `<div class="params">${params}</div>` +
        `<div class="vu"><div class="vu-fill"></div></div>`;
      portsHtml = n.ports.map(p => `<span class="port in" data-port="${p[0]}"></span>`).join('') + `<span class="port out" data-port="out"></span>`;
    } else {
      const hasOut = n.type === 'source', hasIn = n.type !== 'source';
      body =
        `<div class="ninner"><div class="ncap">${n.type === 'source' ? 'input level' : 'output level'}</div><div class="vu"><div class="vu-fill"></div></div></div>` +
        `<div class="volrow"><button class="mute-btn" title="mute / unmute">${I.muteOff}</button><input class="vslider" type="range" min="0" max="100" value="${n.vol}" title="volume" /><span class="vval">${n.vol}%</span></div>`;
      portsHtml = (hasIn ? '<span class="port in" data-port="in"></span>' : '') + (hasOut ? '<span class="port out" data-port="out"></span>' : '');
    }

    el.innerHTML =
      `<div class="nlabel"><span class="ld"></span>${n.label}</div>` +
      `<div class="ncard">${portsHtml}` +
        `<div class="nhead"><div class="nicon">${n.icon || n.glyph}</div><div class="nhtxt"><div class="nname">${n.name}</div><div class="nsub">${n.sub}</div></div></div>` +
        body +
      `</div>`;

    world.appendChild(el);
    const ports = {}; el.querySelectorAll('.port').forEach(p => ports[p.dataset.port] = p);
    reg[n.id] = { data: n, el, muted: false, level: 0.5, ports };
    requestAnimationFrame(() => { alignPorts(n.id); drawEdges(); });

    // volume
    const slider = el.querySelector('.vslider');
    if (slider) {
      const vval = el.querySelector('.vval');
      slider.addEventListener('input', () => { n.vol = +slider.value; vval.textContent = n.vol + '%'; });
      slider.addEventListener('mousedown', e => e.stopPropagation());
    }
    // mute
    const mbtn = el.querySelector('.mute-btn');
    if (mbtn) {
      mbtn.addEventListener('mousedown', e => e.stopPropagation());
      mbtn.addEventListener('click', () => {
        const s = reg[n.id]; s.muted = !s.muted;
        el.classList.toggle('is-muted', s.muted);
        mbtn.classList.toggle('on', s.muted);
        mbtn.innerHTML = s.muted ? I.muteOn : I.muteOff;
        drawEdges(); toast(n.name + (s.muted ? ' muted' : ' unmuted'));
      });
    }
    // param dropdowns
    el.querySelectorAll('.pctl').forEach(btn => {
      btn.addEventListener('mousedown', e => e.stopPropagation());
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const pname = btn.dataset.param, opts = PARAM_OPTS[pname] || [];
        const cur = btn.firstChild.textContent.trim();
        openMenu(btn, opts.map(o => ({ label: o, checked: o === cur, onClick: () => {
          btn.firstChild.textContent = o;
          const pr = n.params.find(p => p[0] === pname); if (pr) pr[1] = o;
          toast(pname + ' → ' + o);
        } })));
      });
    });

    // select + drag
    el.querySelector('.ncard').addEventListener('mousedown', (e) => {
      if (e.button !== 0 || e.target.closest('.pctl') || e.target.closest('.mute-btn') || e.target.closest('.vslider')) return;
      e.stopPropagation();
      select(n.id);
      const sx = e.clientX, sy = e.clientY, ox = n.x, oy = n.y; let moved = false;
      el.classList.add('dragging');
      const move = (ev) => { moved = true; n.x = ox + (ev.clientX - sx) / zoom; n.y = oy + (ev.clientY - sy) / zoom; el.style.transform = `translate(${n.x}px, ${n.y}px)`; drawEdges(); };
      const up = () => { el.classList.remove('dragging'); window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
      window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
    });
    return el;
  }

  NODES.forEach(buildNode);

  function select(id) { selected = id; Object.values(reg).forEach(s => s.el.classList.toggle('selected', s.data.id === id)); updateSelMeta(); }
  function updateSelMeta() {
    const meta = document.getElementById('selMeta');
    if (meta) meta.textContent = selected ? reg[selected].data.name : 'nothing selected';
  }

  /* ---------- node actions ---------- */
  function deleteNode(id) {
    if (!reg[id]) return;
    const name = reg[id].data.name;
    reg[id].el.remove(); delete reg[id];
    EDGES = EDGES.filter(e => e.from !== id && e.to !== id);
    const i = NODES.findIndex(n => n.id === id); if (i >= 0) NODES.splice(i, 1);
    if (selected === id) selected = null;
    drawEdges(); updateSelMeta(); updateStatus(); toast(name + ' deleted');
  }
  function duplicateNode(id) {
    if (!reg[id]) return;
    const src = reg[id].data;
    const copy = JSON.parse(JSON.stringify(src));
    copy.id = 'n' + (++uid); copy.x = src.x + 40; copy.y = src.y + 40; copy.name = src.name + ' copy'; copy.editor = null;
    NODES.push(copy); buildNode(copy);
    requestAnimationFrame(() => { alignPorts(copy.id); select(copy.id); drawEdges(); });
    updateStatus(); toast(src.name + ' duplicated');
  }
  function muteAll(v) { Object.keys(reg).forEach(id => { const s = reg[id]; if (s.data.type === 'processor') return; s.muted = v; s.el.classList.toggle('is-muted', v); const mb = s.el.querySelector('.mute-btn'); if (mb) { mb.classList.toggle('on', v); mb.innerHTML = v ? I.muteOn : I.muteOff; } }); drawEdges(); toast(v ? 'all muted' : 'all unmuted'); }

  /* ---------- cursors ---------- */
  CURSORS.forEach(c => {
    const el = document.createElement('div'); el.className = 'cursor';
    el.style.setProperty('--cx', c.x + 'px'); el.style.setProperty('--cy', c.y + 'px');
    el.style.animationDelay = (Math.random() * -5) + 's';
    el.innerHTML = `<div style="color:${c.color}">${I.cursor}</div><span class="ctag" style="background:${c.color}">${c.name}</span>`;
    world.appendChild(el);
  });

  /* ---------- edges ---------- */
  function topWithin(node, ancestor) { let y = 0, el = node; while (el && el !== ancestor) { y += el.offsetTop; el = el.offsetParent; } return y; }
  function alignPorts(id) {
    const s = reg[id]; if (!s) return;
    const n = s.data, el = s.el, ports = s.ports, card = el.querySelector('.ncard');
    const setY = (port, rowEl) => { if (!port || !rowEl) return; const y = topWithin(rowEl, card) + rowEl.offsetHeight / 2; port.style.top = y + 'px'; port.style.transform = 'translateY(-50%)'; };
    if (n.type === 'processor') { const rows = el.querySelectorAll('.ninner .prow'); n.ports.forEach((p, i) => setY(ports[p[0]], rows[i])); setY(ports['out'], el.querySelector('.prow.out')); }
    else { const head = el.querySelector('.nhead'); Object.values(ports).forEach(p => setY(p, head)); }
  }
  function alignAll() { Object.keys(reg).forEach(alignPorts); }

  function portWorld(el) { const wr = world.getBoundingClientRect(), r = el.getBoundingClientRect(); return { x: (r.left + r.width / 2 - wr.left) / zoom, y: (r.top + r.height / 2 - wr.top) / zoom }; }
  function drawEdges() {
    let html = '';
    EDGES.forEach(e => {
      if (!reg[e.from] || !reg[e.to]) return;
      const fp = reg[e.from].ports[e.fp], tp = reg[e.to].ports[e.tp]; if (!fp || !tp) return;
      const a = portWorld(fp), b = portWorld(tp);
      const dx = Math.max(45, Math.min(170, Math.abs(b.x - a.x) * 0.5));
      const d = `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
      const muted = reg[e.from].muted || reg[e.to].muted;
      const col = css('--' + (reg[e.from].data.type === 'processor' ? 'accent' : 'c-' + reg[e.from].data.type));
      if (!muted) html += `<path class="l-edge-glow on" d="${d}" stroke="${col}"></path>`;
      html += `<path class="l-edge ${muted ? 'is-muted' : 'on'}" d="${d}"></path>`;
    });
    svg.innerHTML = html;
  }

  /* ---------- vu ---------- */
  let vuTimer = null;
  function startVu() {
    if (vuTimer) return;
    vuTimer = setInterval(() => {
      Object.values(reg).forEach(s => {
        const target = s.muted ? 0 : (0.25 + Math.random() * 0.65);
        s.level += (target - s.level) * 0.45;
        const f = s.el.querySelector('.vu-fill'); if (!f) return;
        const vol = s.data.vol != null ? s.data.vol : 90;
        f.style.width = (live && !s.muted ? Math.round(s.level * (vol / 100) * 100) : 0) + '%';
      });
    }, 70);
  }
  function stopVu() { clearInterval(vuTimer); vuTimer = null; document.querySelectorAll('.vu-fill').forEach(f => f.style.width = '0%'); }

  /* ---------- engine ---------- */
  const genPill = document.getElementById('genPill');
  function setLive(v) {
    live = v; app.classList.toggle('live', live);
    if (genPill) genPill.querySelector('.gp-text').textContent = live ? 'engine live' : 'engine paused';
    if (genPill) genPill.classList.toggle('paused', !live);
    if (live) startVu(); else stopVu();
  }

  /* ---------- status ---------- */
  function updateStatus() {
    const n = Object.keys(reg).length;
    const sN = document.getElementById('stN'); if (sN) sN.textContent = n + ' (' + n + ')';
  }

  /* ---------- pan & zoom ---------- */
  function applyView() { world.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`; }
  stage.addEventListener('mousedown', (e) => {
    if (e.target.closest('.lnode') || e.target.closest('button') || e.target.closest('.rail') || e.target.closest('.cmdbar') || e.target.closest('.menu')) return;
    select(null);
    const sx = e.clientX, sy = e.clientY, ox = panX, oy = panY;
    const move = (ev) => { panX = ox + ev.clientX - sx; panY = oy + ev.clientY - sy; applyView(); };
    const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
    window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
  });
  stage.addEventListener('wheel', (e) => {
    e.preventDefault();
    const r = stage.getBoundingClientRect(), cx = e.clientX - r.left, cy = e.clientY - r.top;
    const z = Math.max(0.4, Math.min(2, zoom * Math.exp(-e.deltaY * 0.0014))), k = z / zoom;
    panX = cx - (cx - panX) * k; panY = cy - (cy - panY) * k; zoom = z; applyView();
  }, { passive: false });
  function setZoom(z, cx, cy) { cx = cx == null ? stage.clientWidth / 2 : cx; cy = cy == null ? stage.clientHeight / 2 : cy; z = Math.max(0.4, Math.min(2, z)); const k = z / zoom; panX = cx - (cx - panX) * k; panY = cy - (cy - panY) * k; zoom = z; applyView(); }
  function fitView() {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    NODES.forEach(n => { const el = reg[n.id].el; minX = Math.min(minX, n.x); minY = Math.min(minY, n.y); maxX = Math.max(maxX, n.x + el.offsetWidth); maxY = Math.max(maxY, n.y + el.offsetHeight); });
    const pad = 90, vw = stage.clientWidth, vh = stage.clientHeight;
    zoom = Math.max(0.4, Math.min(1.1, Math.min((vw - pad * 2) / (maxX - minX), (vh - pad * 2) / (maxY - minY))));
    panX = (vw - (maxX - minX) * zoom) / 2 - minX * zoom; panY = (vh - (maxY - minY) * zoom) / 2 - minY * zoom;
    applyView();
  }

  /* ============ wire the chrome ============ */
  const $ = (id) => document.getElementById(id);
  $('zin').addEventListener('click', () => setZoom(zoom + 0.15));
  $('zout').addEventListener('click', () => setZoom(zoom - 0.15));
  $('zfit').addEventListener('click', fitView);
  const eyeBtn = $('eyeBtn');
  eyeBtn.addEventListener('click', () => { setLive(!live); eyeBtn.classList.toggle('on', live); toast(live ? 'activity shown' : 'activity hidden'); });
  $('linkBtn').addEventListener('click', () => toast('connection mode — drag port to port'));

  // engine pill toggles live
  if (genPill) genPill.addEventListener('click', () => { setLive(!live); toast(live ? 'engine running' : 'engine paused'); });

  // queue: run once
  $('queueBtn').addEventListener('click', () => {
    const mixEl = reg.mix && reg.mix.el; if (mixEl) { mixEl.classList.add('pulse'); setTimeout(() => mixEl.classList.remove('pulse'), 700); }
    toast('queued · processing graph');
  });

  // toolbar menus
  $('mWorkflow').addEventListener('click', (e) => openMenu(e.currentTarget, [
    { label: 'New scene', onClick: () => toast('new scene created') },
    { label: 'Duplicate scene', onClick: () => toast('scene duplicated') },
    { sep: true },
    { label: 'Import…', onClick: () => toast('import workflow') },
    { label: 'Export…', onClick: () => toast('export workflow') },
  ]));
  $('mEdit').addEventListener('click', (e) => openMenu(e.currentTarget, [
    { label: 'Duplicate node', hint: '⌘D', onClick: () => selected ? duplicateNode(selected) : toast('select a node first') },
    { label: 'Delete node', hint: '⌫', onClick: () => selected ? deleteNode(selected) : toast('select a node first') },
    { sep: true },
    { label: 'Mute all', onClick: () => muteAll(true) },
    { label: 'Unmute all', onClick: () => muteAll(false) },
    { sep: true },
    { label: 'Fit to screen', hint: 'F', onClick: fitView },
  ]));
  $('mHelp').addEventListener('click', (e) => openMenu(e.currentTarget, [
    { label: 'Drag node — move', muted: true },
    { label: 'Click param — change value', muted: true },
    { label: 'Del — delete selected', muted: true },
    { label: 'F — fit · Space drag — pan', muted: true },
    { sep: true },
    { label: 'About Nodus', onClick: () => toast('Nodus · virtual audio router') },
  ]));
  $('moreBtn').addEventListener('click', (e) => openMenu(e.currentTarget, [
    { label: selected ? ('Duplicate ' + reg[selected].data.name) : 'Duplicate node', onClick: () => selected ? duplicateNode(selected) : toast('select a node first') },
    { label: 'Delete node', onClick: () => selected ? deleteNode(selected) : toast('select a node first') },
    { sep: true },
    { label: 'Reset view', onClick: fitView },
  ], { right: true }));
  $('menuBtn').addEventListener('click', (e) => openMenu(e.currentTarget, [
    { label: 'Fit to screen', onClick: fitView },
    { label: 'Reset zoom', onClick: () => setZoom(1) },
    { label: live ? 'Pause engine' : 'Start engine', onClick: () => { setLive(!live); } },
  ], { right: true }));

  // breadcrumb + stepper + clear/delete
  $('crumbPrev').addEventListener('click', () => toast('previous scene'));
  $('crumbNext').addEventListener('click', () => toast('next scene'));
  $('crumbX').addEventListener('click', () => toast('close scene'));
  $('stepUp').addEventListener('click', () => setZoom(zoom + 0.1));
  $('stepDown').addEventListener('click', () => setZoom(zoom - 0.1));
  $('clearBtn').addEventListener('click', () => { if (selected) { select(null); toast('deselected'); } else toast('nothing selected'); });
  $('delBtn').addEventListener('click', () => selected ? deleteNode(selected) : toast('select a node first'));

  // share / make public
  $('shareBtn').addEventListener('click', () => toast('share link copied to clipboard'));
  $('publicBtn').addEventListener('click', (e) => { e.currentTarget.classList.toggle('on'); toast(e.currentTarget.classList.contains('on') ? 'workflow is now public' : 'workflow set to private'); });

  // command-bar search — dims non-matching nodes
  const search = $('cmdSearch');
  if (search) search.addEventListener('input', () => {
    const q = search.value.trim().toLowerCase();
    Object.values(reg).forEach(s => { const hit = !q || s.data.name.toLowerCase().includes(q); s.el.classList.toggle('dim', !!q && !hit); s.el.classList.toggle('hit', !!q && hit); });
  });
  document.querySelectorAll('.cmd-tool').forEach(t => t.addEventListener('click', () => {
    document.querySelectorAll('.cmd-tool').forEach(x => x.classList.remove('active')); t.classList.add('active');
    toast(t.dataset.label || 'panel');
  }));

  // keyboard
  window.addEventListener('keydown', (e) => {
    const tag = (e.target.tagName || '').toLowerCase(); if (tag === 'input') return;
    if (e.key === 'Delete' || e.key === 'Backspace') { if (selected) deleteNode(selected); }
    if (e.key === 'f' || e.key === 'F') fitView();
    if (e.key === 'Escape') { closeMenu(); select(null); }
    if ((e.key === 'd' || e.key === 'D') && (e.metaKey || e.ctrlKey)) { e.preventDefault(); if (selected) duplicateNode(selected); }
  });

  /* ---------- boot ---------- */
  function boot() { alignAll(); fitView(); drawEdges(); setLive(true); eyeBtn.classList.add('on'); updateSelMeta(); updateStatus(); }
  if (document.readyState === 'complete') requestAnimationFrame(boot);
  else window.addEventListener('load', () => requestAnimationFrame(boot));
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => { alignAll(); fitView(); drawEdges(); });
  window.addEventListener('resize', () => { alignAll(); fitView(); drawEdges(); });
})();
