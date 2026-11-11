/* Shell extras: background service settings, status chips, backend event surfaces and the
   newer device controls. Loaded after app.js; everything here degrades to nothing when the
   backend (or a matching capability) is missing. */
(() => {
  'use strict';
  const app = window.controllerStudio;
  if (!app) return;
  const { state, native, invoke, toast } = app;
  const $ = (s, root = document) => root.querySelector(s);
  const $$ = (s, root = document) => [...root.querySelectorAll(s)];
  const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, n));
  const send = (command, quiet) => app.send(command, false, null, quiet);
  const EXPERIMENTAL = 'Experimental — unverified on hardware';

  const load = (key, fallback) => { try { const v = JSON.parse(localStorage.getItem(key)); return v ?? fallback; } catch { return fallback; } };
  const save = (key, value) => { try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* storage may be blocked */ } };

  function h(tag, props = {}, kids = []) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (value == null || value === false) continue;
      if (key === 'text') node.textContent = value;
      else if (key === 'class') node.className = value;
      else if (key in node && key !== 'list') node[key] = value;
      else node.setAttribute(key, value === true ? '' : value);
    }
    for (const kid of [].concat(kids)) if (kid != null) node.append(kid);
    return node;
  }
  let uid = 0;
  const nextId = prefix => `${prefix}-${++uid}`;
  function field(label, input, hint) {
    if (!input.id) input.id = nextId('extra');
    const row = h('div', { class: 'extra-field' });
    row.append(h('label', { class: 'control-label', htmlFor: input.id, text: label }), input);
    if (hint) row.append(h('small', { text: hint }));
    return row;
  }
  function range(min, max, step, value, onInput, format) {
    const input = h('input', { type: 'range', min, max, step, value });
    const out = h('b', { class: 'extra-value' });
    const show = () => { out.textContent = format ? format(+input.value) : input.value; };
    input.addEventListener('input', () => { show(); input.style.setProperty('--fill', `${((+input.value - min) / (max - min)) * 100}%`); });
    input.addEventListener('change', () => onInput(+input.value));
    show(); input.style.setProperty('--fill', `${((+value - min) / (max - min)) * 100}%`);
    return { input, out };
  }
  function rangeField(label, min, max, step, value, onChange, format) {
    const r = range(min, max, step, value, onChange, format);
    const wrap = h('div', { class: 'extra-field' });
    r.input.id = nextId('extra');
    wrap.append(h('label', { class: 'control-label', htmlFor: r.input.id }, [h('span', { text: label }), r.out]), r.input);
    wrap.range = r;
    return wrap;
  }
  function named(wrap, id) { const i = wrap.querySelector('input'); i.id = id; wrap.querySelector('label').htmlFor = id; return wrap; }
  function select(options, value, onChange) {
    const node = h('select');
    for (const [v, label] of options) node.add(new Option(label, String(v)));
    node.value = String(value);
    node.addEventListener('change', () => onChange(node.value));
    return node;
  }
  function checkbox(label, checked, onChange, id) {
    const input = h('input', { type: 'checkbox', checked: !!checked });
    if (id) input.id = id;
    input.addEventListener('change', () => onChange(input.checked));
    return { row: h('label', { class: 'check-row' }, [input, document.createTextNode(` ${label}`)]), input };
  }
  function numberInput(min, max, step, value, onChange) {
    const input = h('input', { type: 'number', min, max, step, value });
    input.addEventListener('change', () => {
      const v = clamp(Number(input.value) || 0, min, max);
      input.value = String(v);
      onChange(v);
    });
    return input;
  }
  function panel(title, tag, id) {
    const body = h('div', { class: 'controls-inner extra-body' });
    const box = h('div', { class: 'work-panel framed extras-panel', id }, [h('div', { class: 'panel-cap' }, [h('span', { text: title }), h('span', { text: tag })]), body]);
    return { box, body };
  }
  const profile = () => app.profile();

  /* ---------- status chips ---------- */
  const chips = new Map();
  function setChip(key, text, tone = 'info', title = '') {
    if (!text) chips.delete(key); else chips.set(key, { text, tone, title });
    const host = $('#status-chips');
    if (!host) return;
    host.replaceChildren(...[...chips].map(([k, c]) => {
      const chip = h('span', { class: `status-chip is-${c.tone}`, text: c.text });
      chip.dataset.chip = k;
      if (c.title) chip.title = c.title;
      return chip;
    }));
  }
  const humanize = text => String(text || '').replace(/[-_]+/g, ' ').replace(/^./, c => c.toUpperCase());
  const STATE_LABEL = {
    disconnected: 'Disconnected', handoff: 'Released to a game', 'edge-busy': 'Saving Edge profile',
  };
  const REASON_LABEL = {
    'native-bridge': 'a native bridge has the controller', 'native-feedback': 'a game is driving native feedback',
    'edge-write': 'an Edge profile is being written', 'hid-disabled': 'HID is disabled for this session',
    'selection-not-found': 'the selected controller was not found',
  };
  function doubleInputChip() {
    const risk = state.xinput && (state.mouse || state.gyroAim || state.remapArmed);
    setChip('double', risk ? 'Double input risk' : '', 'warn',
      'XInput is on while mouse, gyro or remap is active. Hide the DualSense (for example with HidHide) or turn one off.');
  }

  /* ---------- notifications ---------- */
  async function notify(title, body, tone) {
    const n = window.__TAURI__?.notification;
    if (n?.sendNotification) {
      try {
        let ok = n.isPermissionGranted ? await n.isPermissionGranted() : true;
        if (!ok && n.requestPermission) ok = (await n.requestPermission()) === 'granted';
        if (ok) { n.sendNotification({ title, body }); return; }
      } catch { /* fall back to the in-app toast */ }
    }
    toast(`${title}: ${body}`, { error: tone !== 'info' });
  }
  document.addEventListener('controller-low-battery', e => {
    const d = e.detail || {};
    notify('Low battery', `${d.name || 'Controller'} is at ${d.level}%. Plug it in soon.`, 'warn');
  });

  /* ---------- background service settings ---------- */
  let settings = null, restarting = false;
  function renderService(coreStatus) {
    const chip = $('#core-service-chip'), detail = $('#core-service-detail'), hint = $('#background-core-hint');
    if (!chip) return;
    const s = settings || {};
    const failed = s.coreState === 'failed' || coreStatus === 'failed';
    let label = 'Starting', st = 'starting';
    if (!native) { label = 'Preview'; st = 'off'; }
    else if (failed) { label = 'Core failed'; st = 'failed'; }
    else if (restarting || coreStatus === 'restarting') { label = 'Restarting'; st = 'starting'; }
    else if (s.backgroundActive) { label = 'Service running'; st = 'running'; }
    else if (s.coreState === 'running' || coreStatus === 'ready') { label = 'Built-in core'; st = 'builtin'; }
    chip.textContent = label; chip.dataset.state = st;
    if (hint) hint.hidden = !(settings && !!settings.backgroundCore !== !!settings.backgroundActive);
    if (detail) {
      detail.textContent = !native ? 'Background service settings work in the installed app.'
        : failed ? `The core could not start${s.coreError ? `: ${s.coreError}` : ''}. Try Restart core.`
        : s.backgroundActive ? 'The controller core runs as a background service. It keeps profiles, remaps and effects active with the window closed.'
        : s.backgroundCore ? 'Background service is on for the next start. Restart the core to switch now.'
        : 'The core runs inside this app and stops when you quit it.';
    }
    $('#restart-core').disabled = !native || restarting;
    $('#background-core').disabled = !native; $('#start-minimized').disabled = !native;
  }
  async function refreshSettings(coreStatus) {
    if (!native) { renderService(coreStatus); return; }
    try {
      const s = await invoke('get_app_settings');
      if (s && typeof s === 'object') {
        settings = s;
        $('#background-core').checked = !!s.backgroundCore;
        $('#start-minimized').checked = s.startMinimized !== false;
      }
      try {
        const status = await invoke('get_core_status');
        if (status && settings) { settings.coreState = status.state || settings.coreState; settings.coreError = status.error ?? settings.coreError; settings.backgroundActive = status.backgroundActive ?? settings.backgroundActive; }
      } catch { /* optional */ }
    } catch { /* settings stay as they were */ }
    renderService(coreStatus);
  }
  async function restartCore() {
    if (!native || restarting) return;
    restarting = true; renderService('restarting');
    $('#core-banner').hidden = true;
    try { await invoke('restart_core'); toast('Core restarted.'); }
    catch (error) { showBanner(`Core restart failed: ${error}`); }
    finally { restarting = false; await refreshSettings(); }
  }
  for (const [id, command] of [['background-core', 'set_background_core'], ['start-minimized', 'set_start_minimized']]) {
    $(`#${id}`).addEventListener('change', async event => {
      const enabled = event.target.checked; event.target.disabled = true;
      try {
        await invoke(command, { enabled });
        $('#app-settings-status').textContent = id === 'background-core' ? 'Saved. Restart the core to apply.' : 'Saved.';
      } catch (error) { event.target.checked = !enabled; $('#app-settings-status').textContent = `Could not save: ${error}`; }
      finally { event.target.disabled = false; await refreshSettings(); }
    });
  }
  $('#restart-core').addEventListener('click', restartCore);
  $('#core-banner-restart').addEventListener('click', restartCore);
  function showBanner(text) { $('#core-banner-text').textContent = text; $('#core-banner').hidden = false; }

  /* ---------- tray profile ---------- */
  let trayName = null, lightsSignature = '';
  function profileChanged() {
    let p; try { p = profile(); } catch { return; }
    if (p && p.name && p.name !== trayName) {
      trayName = p.name;
      if (native) Promise.resolve(invoke('set_tray_profile', { name: p.name })).catch(() => {});
    }
    const sig = `${p?.id}|${JSON.stringify(p?.lightbar || null)}|${JSON.stringify(p?.audioPreset || null)}`;
    if (sig !== lightsSignature) { lightsSignature = sig; renderLightsProfile?.(); }
  }

  /* ---------- profile load recovery ---------- */
  async function recoveryStatus(failed) {
    const button = $('#load-failed-recover'), note = $('#load-failed-recovery');
    if (!failed) { button.hidden = true; note.hidden = true; return; }
    if (!native) return;
    try {
      const r = await invoke('profile_recovery_status');
      if (!r || r.ok) { button.hidden = true; note.hidden = true; return; }
      button.hidden = !r.recoverable;
      note.hidden = false;
      const backup = r.backup;
      const when = backup?.createdMs ? new Date(backup.createdMs).toLocaleString() : '';
      note.textContent = r.recoverable
        ? `A valid backup was found${when ? ` from ${when}` : ''}. Recovering keeps the unreadable file aside.`
        : 'No readable backup exists yet. Use Start fresh to continue.';
    } catch { /* the Restore and Start fresh buttons still work */ }
  }

  /* ---------- re-send stored preferences ---------- */
  // Only values the user explicitly saved are pushed, so a fresh install sends nothing.
  let pushedFor = null;
  function resendStoredPrefs() {
    if (!state.connected) return;
    const stored = (key) => { try { return localStorage.getItem(key) != null; } catch { return false; } };
    try {
      if (stored(PC_KEY)) {
        const raw = load(PC_KEY, {});
        if (raw.holdMs > 0 && raw.gapMs > 0) send({ action: 'setRemapTiming', holdMs: clamp(raw.holdMs, 15, 500), gapMs: clamp(raw.gapMs, 5, 500) }, true);
        if (raw.touchpadClick || raw.mic) send({ action: 'setPadExtras', touchpadClick: raw.touchpadClick || null, mic: raw.mic || null }, true);
      }
      if (stored('controller.autoBias.v1')) send({ action: 'setAutoBias', enabled: !!load('controller.autoBias.v1', false) }, true);
      const bt = load(BATTERY_KEY, null);
      if (bt && Number.isInteger(bt.warn) && Number.isInteger(bt.critical) && bt.critical < bt.warn) send({ action: 'setLowBatteryThresholds', warn: bt.warn, critical: bt.critical }, true);
    } catch { /* best effort */ }
  }

  /* ---------- events ---------- */
  const handlers = {
    battery(e) {
      const faults = Array.isArray(e.faults) ? e.faults : [];
      const line = $('#battery-faults');
      if (e.available === false) {
        const why = e.reason === 'short-report' ? 'This Bluetooth report has no battery data. Use USB or the full input report.' : 'The controller did not report a battery level.';
        setChip('battery', 'Battery unavailable', 'warn', why);
        line.hidden = false; line.textContent = why;
        return;
      }
      setChip('battery', '');
      if (faults.length) {
        const text = `Battery fault: ${faults.map(humanize).join(', ')}`;
        setChip('bfault', text, 'error'); line.hidden = false; line.textContent = text;
      } else { setChip('bfault', ''); line.hidden = true; line.textContent = ''; }
    },
    device(e) {
      if (e.connected) {
        setChip('state', ''); setChip('conn', '');
        const fw = e.firmwareVersion ? `Firmware ${e.firmwareVersion}${e.firmwareBuild ? ` · ${e.firmwareBuild}` : ''}` : '';
        setChip('fw', fw, 'info');
        if (fw) { const d = $('#diag-firmware'); if (d) d.textContent = fw; }
        setChip('legacy', e.legacyRumble ? 'Legacy rumble' : '', 'warn', 'This firmware drives the grips as plain rumble motors.');
      } else {
        for (const k of ['fw', 'legacy', 'jack', 'battery', 'bfault', 'suspend', 'double', 'pinned']) setChip(k, '');
        $('#battery-faults').hidden = true;
        const label = STATE_LABEL[e.state] || (e.state ? humanize(e.state) : '');
        const why = REASON_LABEL[e.reason] || (e.reason ? humanize(e.reason).toLowerCase() : '');
        const busy = e.state && e.state !== 'disconnected';
        setChip('state', label ? `${label}${why ? ` · ${why}` : ''}` : '', busy ? 'warn' : 'info', e.message || '');
        renderSuspended({});
      }
      refreshBatteryControls();
      renderMonitorState();
      if (e.connected) refreshLightsButtons();
      if (!e.connected) pushedFor = null;
      else {
        const key = `${e.controllerId || e.name || ''}|${e.path || ''}|${e.transport || ''}`;
        if (key !== pushedFor) { pushedFor = key; resendStoredPrefs(); }
      }
    },
    audioJacks(e) {
      const text = e.headphones && e.microphone ? 'Headset connected' : e.headphones ? 'Headphones connected' : e.microphone ? 'Headset mic connected' : '';
      setChip('jack', text, 'info');
    },
    outputRestored(e) {
      const names = { lowBatteryAlert: 'low-battery alert', lightbar: 'lightbar', playerLeds: 'player lights', triggers: 'triggers', rumble: 'rumble' };
      const list = (Array.isArray(e.restored) ? e.restored : []).map(k => names[k] || humanize(k).toLowerCase());
      toast(list.length ? `Restored after reconnect: ${list.join(', ')}.` : 'Controller outputs restored.', { error: false });
    },
    suspended(e) { renderSuspended(e); },
    queueOverflow(e) { toast(e.message || 'A macro was dropped because the key queue was full.', { error: true }); },
    desktopInputStatus(e) {
      const warn = $('#pointer-precision-warning');
      if (warn) warn.hidden = e.pointerPrecision !== true;
    },
    hapticAlertStatus(e) {
      if (Number.isInteger(e.warn) && Number.isInteger(e.critical)) setThresholdSliders(e.warn, e.critical);
    },
    core(e) {
      if (e.status === 'failed') showBanner(e.message || 'The controller core failed to start.');
      else if (e.status === 'ready' || e.status === 'restarting') $('#core-banner').hidden = true;
      refreshSettings(e.status);
      // A restarted built-in core forgot everything; the background service keeps its own copy.
      if (e.status === 'ready' || e.status === 'restarting') pushedFor = null;
      if (e.status === 'ready' && state.connected) { pushedFor = 'core'; resendStoredPrefs(); }
    },
    notice(e) {
      if (e.code === 'pinned-missing') setChip('pinned', 'Using another controller', 'warn', e.message);
    },
    xinput: doubleInputChip, mouseMode: doubleInputChip, gyroAim: doubleInputChip, remap: doubleInputChip,
  };
  function renderSuspended(e) {
    const text = reason => reason ? `Suspended because ${String(reason).replace(/^The /, 'the ').replace(/^./, c => c.toLowerCase())}.` : '';
    const set = (id, reason) => { const n = $(id); if (n) { n.hidden = !reason; n.textContent = text(reason); } };
    set('#suspend-badge-pc', e.pc); set('#suspend-badge-gyro', e.gyro); set('#suspend-badge-remap', e.pc);
    setChip('suspend', e.pc || e.gyro ? 'Input suspended' : '', 'warn', [e.pc, e.gyro].filter(Boolean).join(' / '));
  }

  /* ---------- battery thresholds ---------- */
  function setThresholdSliders(warn, critical) {
    const w = $('#battery-warn'), c = $('#battery-critical');
    if (!w || document.activeElement === w || document.activeElement === c) return;
    w.value = String(warn); c.value = String(critical);
    $('#battery-warn-value').textContent = `${warn}%`; $('#battery-critical-value').textContent = `${critical}%`;
  }
  function refreshBatteryControls() {
    const apply = $('#battery-thresholds-apply');
    apply.disabled = !state.connected;
    $('#battery-thresholds-status').textContent = state.connected ? 'Levels apply to the connected controller.' : 'Connect a controller to apply.';
  }
  const BATTERY_KEY = 'controller.batteryThresholds.v1';
  for (const id of ['battery-warn', 'battery-critical']) {
    $(`#${id}`).addEventListener('input', () => { $(`#${id}-value`).textContent = `${$(`#${id}`).value}%`; });
  }
  $('#battery-thresholds-apply').addEventListener('click', async () => {
    const warn = +$('#battery-warn').value, critical = +$('#battery-critical').value;
    if (critical >= warn) { toast('The critical level must be lower than the warning level.', { error: true }); return; }
    if (await send({ action: 'setLowBatteryThresholds', warn, critical })) {
      save(BATTERY_KEY, { warn, critical });
      $('#battery-thresholds-status').textContent = `Warn at ${warn}%, critical at ${critical}%.`;
      app.prepareLowBatteryWarning?.({ threshold: warn });
    }
  });

  /* ---------- persistent (background service) opt-ins ---------- */
  const persistPrefs = load('controller.persistent.v1', {});
  function persistent(action) { return !!persistPrefs[action]; }
  function persistRow(action, label, anchor, position, after) {
    const box = checkbox(label, persistPrefs[action], on => {
      persistPrefs[action] = on; save('controller.persistent.v1', persistPrefs);
      toast(on ? 'Will keep running in the background service after you close the window.' : 'Will stop when the window closes.', { error: false });
      after?.();
    });
    box.row.classList.add('persistent-row');
    box.input.dataset.persistent = action;
    box.row.append(h('small', { class: 'persistent-hint', text: 'Needs the background service (Settings).' }));
    anchor.insertAdjacentElement(position, box.row);
  }
  persistRow('setScript', 'Keep this rule active in the background service', $('#script-toggle').parentElement, 'beforebegin');
  const gameStatus = $('#game-action-status');
  if (gameStatus) persistRow('setGameActions', 'Keep game actions active in the background service', gameStatus, 'beforebegin', () => app.applyGameRules?.());
  persistRow('setPcBindings', 'Keep PC shortcuts active in the background service', $('#pc-bindings-status'), 'beforebegin', () => app.savePcBindings());

  /* ---------- binding extras (remap and PC shortcuts) ---------- */
  const LAYER_BUTTONS = [[4, 'L1'], [5, 'R1'], [8, 'Create'], [9, 'Options'], [10, 'L3'], [11, 'R3'], [16, 'PS'], [17, 'Touchpad']];
  const PROGRAM_MODES = [['once', 'Run once, press again to restart'], ['cancelOnRepress', 'Run once, press again to cancel'], ['toggle', 'Loop until pressed again'], ['whileHeld', 'Loop while held']];
  function bindingExtras(binding, opts = {}) {
    const details = h('details', { class: 'binding-extras' });
    const summary = h('summary', { text: 'More options' });
    summary.setAttribute('aria-label', `${opts.label || 'Binding'} more options`);
    const changed = () => { opts.onChange?.(); updateSummary(); };
    const set = (key, value) => {
      if (value === false || value === 0 || value == null || (Array.isArray(value) && !value.length)) delete binding[key]; else binding[key] = value;
      changed();
    };
    const body = h('div', { class: 'binding-extras-body' });
    const layer = h('div', { class: 'extra-field' }, [h('span', { class: 'control-label', text: 'Also hold (layer buttons)' })]);
    const pills = h('div', { class: 'pc-modifiers' });
    for (const [index, name] of LAYER_BUTTONS) {
      if (index === binding.button || index === binding.modifier) continue;
      const b = h('button', { type: 'button', class: 'quiet-action', text: name });
      b.setAttribute('aria-label', `${opts.label || 'Binding'} also hold ${name}`);
      b.setAttribute('aria-pressed', String((binding.modifiers || []).includes(index)));
      b.addEventListener('click', () => {
        const set_ = new Set(binding.modifiers || []);
        set_.has(index) ? set_.delete(index) : set_.add(index);
        b.setAttribute('aria-pressed', String(set_.has(index)));
        set('modifiers', [...set_].sort((x, y) => x - y));
      });
      pills.append(b);
    }
    layer.append(pills);
    const layerOnly = checkbox('Layer only (silence the layer button on its own)', binding.layerOnly, on => set('layerOnly', on));
    const toggleLayer = checkbox('Toggle layer (press once to latch)', binding.toggleLayer, on => set('toggleLayer', on));
    const long = numberInput(0, 2000, 50, binding.longPressMs || 0, v => { set('longPressMs', v); if (v) { const d = double; d.value = '0'; set('doubleTapMs', 0); } });
    const double = numberInput(0, 1000, 25, binding.doubleTapMs || 0, v => { set('doubleTapMs', v); if (v) { long.value = '0'; set('longPressMs', 0); } });
    const turbo = numberInput(0, 500, 10, binding.turboMs || 0, v => set('turboMs', v));
    const consume = checkbox('Hide this button from the virtual gamepad while active', binding.consume, on => set('consume', on));
    body.append(layer, layerOnly.row, toggleLayer.row,
      field('Long press (ms, 0 = off)', long, 'A consuming long-press binding hides the button from the virtual gamepad from the moment it is pressed. With a layer modifier held, a deeper long-press suppresses the shallower binding until it resolves, so a short tap fires nothing.'), field('Double tap (ms, 0 = off)', double),
      field('Turbo / rapid fire (ms, 0 = off)', turbo, 'Hold bindings only.'), consume.row);
    if (opts.pc && binding.behavior === 'program') {
      const mode = select(PROGRAM_MODES, binding.programMode || 'once', v => set('programMode', v === 'once' ? null : v));
      body.append(field('When pressed while the macro runs', mode));
    }
    function updateSummary() {
      const on = ['modifiers', 'layerOnly', 'toggleLayer', 'longPressMs', 'doubleTapMs', 'turboMs', 'consume', 'programMode'].filter(k => binding[k]);
      summary.textContent = on.length ? `More options (${on.length})` : 'More options';
    }
    updateSummary();
    details.append(summary, body);
    return details;
  }

  /* ---------- gyro extras, auto bias, touch mouse ---------- */
  const GYRO_DEFAULTS = { gate: 'hold', stickOffThreshold: 0, smoothThreshold: 0, acceleration: 0, accelThreshold: 0, flickWithoutGyro: false };
  const GYRO_LIMITS = { stickOffThreshold: 1, smoothThreshold: 1000, acceleration: 2, accelThreshold: 1000 };
  function gyroExtras(g) {
    const out = {};
    for (const [key, def] of Object.entries(GYRO_DEFAULTS)) if (g && g[key] != null && g[key] !== def) out[key] = g[key];
    for (const [key, max] of Object.entries(GYRO_LIMITS)) if (key in out) out[key] = clamp(Number(out[key]) || 0, 0, max);
    if (!['hold', 'disable', 'toggle'].includes(out.gate ?? 'hold')) delete out.gate;
    return out;
  }
  function renderGyroExtras() {
    const block = $('#touch .gyro-aim-block');
    if (!block || $('#gyro-extras')) return;
    const g = () => profile().gyroAim;
    const commit = (key, value) => { g()[key] = value; app.saveProfiles(); if (state.connected) send({ action: 'setGyroAim', enabled: state.gyroAim, sensitivity: g().sensitivity, yawScale: g().yawScale ?? 1, pitchScale: g().pitchScale ?? 1, invertY: !!g().invertY, activation: g().activation, bias: g().bias || [0, 0, 0], deadzone: g().deadzone, smoothing: g().smoothing, ...gyroExtras(g()) }); };
    const details = h('details', { class: 'binding-extras gyro-extras', id: 'gyro-extras' }, [h('summary', { text: 'More gyro options' })]);
    Object.assign(details.dataset, { uiSection: 'touch.gyro-more', level: 'advanced', title: 'More gyro options', hint: 'Noise floor, smoothing, acceleration', keywords: 'gyro noise floor smoothing acceleration flick stick pause auto bias drift' });
    const body = h('div', { class: 'binding-extras-body' });
    const gate = select([['hold', 'Hold the button to aim'], ['disable', 'Hold the button to pause aim'], ['toggle', 'Press the button to toggle aim']], 'hold', v => commit('gate', v));
    gate.id = 'gyro-gate';
    const stick = rangeField('Pause while right stick past', 0, 100, 5, 0, v => commit('stickOffThreshold', v / 100), v => (v ? `${v}%` : 'never'));
    named(stick, 'gyro-stick-off');
    const smooth = rangeField('Smooth only below', 0, 200, 5, 0, v => commit('smoothThreshold', v), v => (v ? `${v} °/s` : 'always'));
    named(smooth, 'gyro-smooth-threshold');
    const accel = rangeField('Acceleration on fast turns', 0, 2, 0.1, 0, v => commit('acceleration', v), v => (v ? v.toFixed(1) : 'linear'));
    named(accel, 'gyro-acceleration');
    const accelAt = rangeField('Acceleration starts at', 0, 300, 5, 0, v => commit('accelThreshold', v), v => (v ? `${v} °/s` : 'default 75 °/s'));
    named(accelAt, 'gyro-accel-threshold');
    const flick = checkbox('Flick stick works even while gyro aim is off', false, on => commit('flickWithoutGyro', on), 'gyro-flick-without');
    const autoBias = checkbox('Slowly correct drift while the controller is still (auto bias)', load('controller.autoBias.v1', false), on => {
      save('controller.autoBias.v1', on);
      if (state.connected) send({ action: 'setAutoBias', enabled: on });
    }, 'gyro-auto-bias');
    body.append(field('Aim button behaviour', gate), stick, smooth, accel, accelAt, flick.row, autoBias.row);
    details.append(body);
    details.render = () => {
      const p = g();
      gate.value = p.gate || 'hold';
      for (const [wrap, key, scale] of [[stick, 'stickOffThreshold', 100], [smooth, 'smoothThreshold', 1], [accel, 'acceleration', 1], [accelAt, 'accelThreshold', 1]]) {
        const input = wrap.querySelector('input'); input.value = String((p[key] || 0) * scale);
        input.dispatchEvent(new Event('input'));
      }
      flick.input.checked = !!p.flickWithoutGyro;
    };
    const anchor = $('.gyro-calibration', block);
    anchor.insertAdjacentElement('beforebegin', h('p', { id: 'suspend-badge-gyro', class: 'suspend-badge', role: 'status', hidden: true }));
    anchor.insertAdjacentElement('beforebegin', details);
    details.render();
  }

  const TOUCH_KEY = 'controller.touchMouse.v1';
  let touchPrefs = load(TOUCH_KEY, {});
  function touchOptions() {
    const out = {};
    if (Number.isFinite(touchPrefs.sensitivity) && touchPrefs.sensitivity !== 1) out.sensitivity = touchPrefs.sensitivity;
    if (Number.isFinite(touchPrefs.acceleration) && touchPrefs.acceleration !== 0) out.acceleration = touchPrefs.acceleration;
    return out;
  }
  function renderTouchOptions() {
    const host = $('.touch-pointer-settings .controls-inner');
    if (!host || $('#touch-pointer-extras')) return;
    const push = () => { save(TOUCH_KEY, touchPrefs); if (state.connected && state.mouse) send({ action: 'setTouchMouse', enabled: true, sensitivity: touchPrefs.sensitivity ?? 1, acceleration: touchPrefs.acceleration ?? 0 }); };
    const sens = rangeField('Pointer speed', 0.2, 3, 0.1, touchPrefs.sensitivity ?? 1, v => { touchPrefs.sensitivity = v; push(); }, v => `${v.toFixed(1)}×`);
    named(sens, 'touch-sensitivity');
    const acc = rangeField('Pointer acceleration', 0, 100, 5, Math.round((touchPrefs.acceleration ?? 0) * 100), v => { touchPrefs.acceleration = v / 100; push(); }, v => `${v}%`);
    named(acc, 'touch-acceleration');
    host.append(h('div', { class: 'touch-mode', id: 'touch-pointer-extras' }, [sens, acc, h('small', { text: 'Applies while Touchpad mouse is on.' })]));
  }

  /* ---------- PC options ---------- */
  const PC_KEY = 'controller.pcOptions.v1';
  const pcPrefs = Object.assign({ pointerAcceleration: 0, leftStick: 'off', holdMs: 0, gapMs: 0, touchpadClick: '', mic: '' }, load(PC_KEY, {}));
  function pcOptions() {
    const out = {};
    if (pcPrefs.pointerAcceleration > 0) out.pointer_acceleration = pcPrefs.pointerAcceleration;
    if (pcPrefs.leftStick && pcPrefs.leftStick !== 'off') out.left_stick = pcPrefs.leftStick;
    return out;
  }
  const PAD_BUTTONS = [['', 'Unmapped'], ['a', 'A'], ['b', 'B'], ['x', 'X'], ['y', 'Y'], ['lb', 'LB'], ['rb', 'RB'], ['back', 'Back'], ['start', 'Start'], ['leftStick', 'Left stick click'], ['rightStick', 'Right stick click'], ['guide', 'Guide']];
  function renderPcExtras() {
    const anchor = $('#pc-controls .pc-overview');
    if (!anchor || $('#pc-extras')) return;
    const box = h('div', { class: 'work-panel framed extras-panel pc-work', id: 'pc-extras' }, [h('div', { class: 'panel-cap' }, [h('span', { text: 'More desktop options' }), h('span', { text: 'ADVANCED' })])]);
    const fineBody = h('div', { class: 'controls-inner extra-body' }), timingBody = h('div', { class: 'controls-inner extra-body' });
    const fine = h('div', { class: 'extras-section' }, [fineBody]), timing_ = h('div', { class: 'extras-section' }, [timingBody]);
    Object.assign(fine.dataset, { uiSection: 'pc.fine-tuning', level: 'advanced', title: 'Pointer fine-tuning', hint: 'Click point, acceleration, left stick', keywords: 'pointer acceleration left stick wasd mouse fine tuning macro shortcut keyboard mouse scroll' });
    Object.assign(timing_.dataset, { uiSection: 'pc.timing-extras', level: 'advanced', title: 'Key timing and Xbox extras', hint: 'Key hold and gap, touchpad and mic as Xbox buttons', keywords: 'key timing hold gap repeat xbox touchpad mic virtual gamepad macro shortcut keyboard' });
    box.append(fine, timing_);
    const savePrefs = () => save(PC_KEY, pcPrefs);
    const accel = rangeField('Pointer acceleration', 0, 100, 5, Math.round(pcPrefs.pointerAcceleration * 100), v => { pcPrefs.pointerAcceleration = v / 100; savePrefs(); app.savePcBindings(); }, v => `${v}%`);
    named(accel, 'pc-pointer-acceleration');
    const stick = select([['off', 'Off'], ['mouse', 'Moves the mouse'], ['wasd', 'Sends W A S D']], pcPrefs.leftStick, v => { pcPrefs.leftStick = v; savePrefs(); app.savePcBindings(); });
    stick.id = 'pc-left-stick';
    const hold = numberInput(15, 500, 5, pcPrefs.holdMs || 50, v => { pcPrefs.holdMs = v; });
    const gap = numberInput(5, 500, 5, pcPrefs.gapMs || 30, v => { pcPrefs.gapMs = v; });
    hold.id = 'remap-hold-ms'; gap.id = 'remap-gap-ms';
    const timing = h('button', { type: 'button', class: 'quiet-action', id: 'remap-timing-apply', text: 'Apply key timing' });
    timing.addEventListener('click', async () => {
      pcPrefs.holdMs = +hold.value; pcPrefs.gapMs = +gap.value; savePrefs();
      if (!state.connected) { toast('Connect a controller to apply key timing.'); return; }
      if (await send({ action: 'setRemapTiming', holdMs: pcPrefs.holdMs, gapMs: pcPrefs.gapMs })) toast('Key timing applied.', { error: false });
    });
    const extrasNow = () => ({ touchpadClick: pcPrefs.touchpadClick || null, mic: pcPrefs.mic || null });
    const pushPad = async () => { savePrefs(); if (state.connected) await send({ action: 'setPadExtras', ...extrasNow() }); };
    const tp = select(PAD_BUTTONS, pcPrefs.touchpadClick, v => { pcPrefs.touchpadClick = v; pushPad(); });
    const mic = select(PAD_BUTTONS, pcPrefs.mic, v => { pcPrefs.mic = v; pushPad(); });
    tp.id = 'pad-extra-touchpad'; mic.id = 'pad-extra-mic';
    fineBody.append(accel, field('Left stick in Desktop mode', stick));
    timingBody.append(h('div', { class: 'extra-row' }, [field('Key hold (ms)', hold), field('Gap between keys (ms)', gap)]), timing,
      h('strong', { text: 'Virtual gamepad extras' }),
      h('small', { text: 'Map the touchpad click and mic button onto Xbox buttons while the Xbox bridge is on.' }),
      field('Touchpad click acts as', tp), field('Mic button acts as', mic));
    (($('#pc-bindings') && $('#pc-bindings').closest('.work-panel')) || anchor).insertAdjacentElement('afterend', box);
    timingBody.append(h('p', { id: 'suspend-badge-remap', class: 'suspend-badge', role: 'status', hidden: true }));
  }

  /* ---------- trigger experimental modes ---------- */
  const TRIGGER_MODES = {
    simpleFeedback: { label: 'Simple feedback', params: [['position', 'Start zone', 0, 9, 3], ['strength', 'Strength', 1, 6, 5]] },
    simpleWeapon: { label: 'Simple weapon', params: [['start', 'Start zone', 2, 7, 3], ['end', 'End zone', 3, 8, 6], ['strength', 'Strength', 1, 6, 6]] },
    simpleVibration: { label: 'Simple vibration', params: [['position', 'Start zone', 0, 9, 2], ['amplitude', 'Amplitude', 1, 6, 5], ['frequency', 'Frequency (Hz)', 1, 255, 30]] },
    galloping: { label: 'Galloping', params: [['start', 'Start zone', 0, 8, 2], ['end', 'End zone', 1, 9, 8], ['firstFoot', 'First foot', 0, 6, 2], ['secondFoot', 'Second foot', 1, 7, 5], ['frequency', 'Frequency (Hz)', 1, 255, 20]] },
    machine: { label: 'Machine', params: [['start', 'Start zone', 0, 8, 2], ['end', 'End zone', 1, 9, 8], ['amplitudeA', 'Amplitude A', 0, 5, 5], ['amplitudeB', 'Amplitude B', 0, 5, 2], ['frequency', 'Frequency (Hz)', 1, 255, 25], ['period', 'Period', 0, 255, 10]] },
  };
  function renderExperimentalTriggers() {
    const modes = $('#trigger-modes');
    if (!modes || $('#experimental-triggers')) return;
    const details = h('details', { class: 'binding-extras experimental-triggers', id: 'experimental-triggers' });
    details.append(h('summary', { text: 'More trigger modes' }));
    Object.assign(details.dataset, { uiSection: 'triggers.experimental-modes', level: 'experimental', title: 'More trigger modes', hint: 'Unverified firmware effects', keywords: 'galloping machine weapon feedback vibration rumble bow experimental trigger modes' });
    const body = h('div', { class: 'binding-extras-body' });
    body.append(h('p', { class: 'shell-warning', text: `${EXPERIMENTAL}. Effects are not saved to profiles and last until you reset or apply your profile.` }));
    const mode = select(Object.entries(TRIGGER_MODES).map(([k, v]) => [k, v.label]), 'simpleFeedback', () => renderParams());
    mode.id = 'experimental-trigger-mode';
    const params = h('div', { class: 'extra-params' });
    const values = {};
    function renderParams() {
      params.replaceChildren();
      for (const [key, label, min, max, def] of TRIGGER_MODES[mode.value].params) {
        const input = numberInput(min, max, 1, values[`${mode.value}.${key}`] ?? def, v => { values[`${mode.value}.${key}`] = v; });
        input.id = `experimental-trigger-${key}`;
        params.append(field(`${label} (${min}–${max})`, input));
      }
    }
    renderParams();
    const status = h('small', { id: 'experimental-trigger-status', role: 'status', text: 'Pick a mode, then try it on the selected trigger.' });
    const effect = () => {
      const out = { mode: mode.value };
      for (const [key, , min, max, def] of TRIGGER_MODES[mode.value].params) out[key] = clamp(Math.round(Number($(`#experimental-trigger-${key}`).value)), min, max) || (min === 0 ? 0 : def);
      return out;
    };
    const tryButton = h('button', { type: 'button', class: 'quiet-action', id: 'experimental-trigger-try', text: 'Try on selected trigger' });
    tryButton.addEventListener('click', async () => {
      if (!state.connected) { toast('Connect a controller first.'); return; }
      const side = state.side === 'left' ? 'left' : 'right';
      const ok = await send({ action: 'setAdaptiveTrigger', [side]: effect() });
      status.textContent = ok ? `${TRIGGER_MODES[mode.value].label} sent to ${side === 'left' ? 'L2' : 'R2'}. ${EXPERIMENTAL}.` : 'The controller rejected those values.';
      if (ok) details.dataset.uiActive = 'true';
    });
    const restore = h('button', { type: 'button', class: 'quiet-action', id: 'experimental-trigger-restore', text: 'Restore profile triggers' });
    restore.addEventListener('click', () => { if (state.connected) { app.applyProfileHardware(true); details.dataset.uiActive = 'false'; } else toast('Connect a controller first.'); });
    body.append(field('Mode', mode), params, h('div', { class: 'extra-row' }, [tryButton, restore]), status);
    details.append(body);
    modes.insertAdjacentElement('afterend', details);
  }

  /* ---------- lights extras ---------- */
  let monitors = [], monitorIndex = load('controller.screenMonitor.v1', null), renderLightsProfile = null, refreshLightsButtons = () => {};
  const hexToRgb = hex => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
  const rgbToHex = rgb => '#' + rgb.map(v => clamp(v, 0, 255).toString(16).padStart(2, '0')).join('');
  const ledMask = () => $$('#player-leds [data-player-led]').reduce((m, b) => m | (b.getAttribute('aria-pressed') === 'true' ? 1 << +b.dataset.playerLed : 0), 0);
  function monitorOption() { return monitorIndex != null && monitors.length > 1 ? { monitor: monitorIndex } : {}; }
  function renderMonitorState() {
    const row = $('#screen-monitor-row'); if (!row) return;
    row.hidden = monitors.length < 2;
  }
  async function loadMonitors() {
    if (!native) return;
    try {
      const list = await invoke('screen_monitors');
      monitors = Array.isArray(list) ? list : [];
      const sel = $('#screen-monitor'); if (!sel) return;
      sel.replaceChildren(...monitors.map(m => new Option(`Display ${m.index + 1}${m.primary ? ' (primary)' : ''} · ${m.width}×${m.height}`, String(m.index))));
      if (monitorIndex == null || !monitors.some(m => m.index === monitorIndex)) monitorIndex = null;
      sel.value = String(monitorIndex ?? monitors.find(m => m.primary)?.index ?? 0);
      renderMonitorState();
    } catch { /* picker stays hidden */ }
  }
  const ANIMATIONS = [['off', 'Off (static color)'], ['breathe', 'Breathe'], ['pulse', 'Pulse'], ['rainbow', 'Rainbow'], ['lowBatteryBlink', 'Blink when battery is low']];
  function renderLightsExtras() {
    const layout = $('#lights .light-layout');
    if (!layout || $('#lights-extras')) return;
    const { box, body } = panel('Animation, microphone and volume', 'USB', 'lights-extras');
    Object.assign(box.dataset, { uiSection: 'lights.extras', level: 'advanced', title: 'Animation, microphone and volume', hint: 'Lightbar animation, volumes and brightness', keywords: 'mic microphone volume brightness animation headphone speaker gain breathe pulse rainbow' });
    // Screen monitor picker.
    const monitorSel = select([['0', 'Primary display']], '0', v => {
      monitorIndex = +v; save('controller.screenMonitor.v1', monitorIndex);
      if (state.screenLight) send({ action: 'setScreenLightbar', enabled: true, monitor: monitorIndex });
    });
    monitorSel.id = 'screen-monitor';
    const monitorRow = field('Follow screen colour from', monitorSel, 'Applies to Follow screen colour.');
    monitorRow.id = 'screen-monitor-row'; monitorRow.hidden = true;
    // Animation.
    const anim = select(ANIMATIONS, 'off', () => {}); anim.id = 'lightbar-animation';
    const period = numberInput(300, 20000, 100, 2000, () => {}); period.id = 'lightbar-period';
    const animApply = h('button', { type: 'button', class: 'quiet-action', id: 'lightbar-animation-apply', text: 'Apply animation', disabled: true });
    animApply.addEventListener('click', () => send({ action: 'setLightbarAnimation', mode: anim.value, rgb: hexToRgb($('#light-color').value), periodMs: clamp(+period.value || 2000, 300, 20000) }));
    // Mic LED, volumes, brightness (experimental).
    const exp = h('div', { class: 'extra-group' }, [h('strong', { text: 'Microphone light' }), h('small', { class: 'experimental-tag', text: EXPERIMENTAL })]);
    const micLed = select([['0', 'Off'], ['1', 'On'], ['2', 'Pulse']], '0', v => { send({ action: 'setMicrophoneLed', mode: +v }); micBox.dataset.uiActive = String(+v !== 0); });
    micLed.id = 'mic-led-mode'; micLed.disabled = true;
    const micBox = h('div', { class: 'extra-micled' });
    Object.assign(micBox.dataset, { uiSection: 'lights.mic-light', level: 'experimental', title: 'Microphone light', hint: 'Unverified on hardware', keywords: 'mic microphone light led mute' });
    const vols = { headphone: 100, speaker: 70, microphone: 100 };
    const pushVol = key => send({ action: 'setAudioVolumes', [key]: vols[key] });
    const vHead = rangeField('Headphone volume', 0, 127, 1, vols.headphone, v => { vols.headphone = v; pushVol('headphone'); });
    const vSpk = rangeField('Speaker volume', 0, 100, 1, vols.speaker, v => { vols.speaker = v; pushVol('speaker'); });
    const vMic = rangeField('Microphone gain', 0, 127, 1, vols.microphone, v => { vols.microphone = v; pushVol('microphone'); });
    named(vHead, 'volume-headphone'); named(vSpk, 'volume-speaker'); named(vMic, 'volume-microphone');
    const bright = select([['0', 'Bright'], ['1', 'Medium'], ['2', 'Dim']], '0', v => { send({ action: 'setLedBrightness', brightness: +v }); });
    bright.id = 'led-brightness'; bright.disabled = true;
    // Save with profile.
    const saveRow = h('div', { class: 'extra-group' });
    const saveLights = h('button', { type: 'button', class: 'main-action', id: 'save-lightbar-profile', text: 'Save lightbar with this profile' });
    const clearLights = h('button', { type: 'button', class: 'quiet-action', id: 'clear-lightbar-profile', text: 'Remove from profile' });
    const profileNote = h('small', { id: 'lightbar-profile-note', role: 'status' });
    saveLights.addEventListener('click', () => {
      const p = profile();
      p.lightbar = { color: $('#light-color').value.toLowerCase(), playerLeds: ledMask(), mode: anim.value, periodMs: clamp(+period.value || 2000, 300, 20000) };
      p.audioPreset = { headphone: vols.headphone, speaker: vols.speaker, microphone: vols.microphone };
      app.saveProfiles(); renderLightsProfile(); toast('Lightbar and audio levels saved with this profile.', { error: false });
    });
    clearLights.addEventListener('click', () => { const p = profile(); delete p.lightbar; delete p.audioPreset; app.saveProfiles(); renderLightsProfile(); });
    saveRow.append(h('div', { class: 'extra-row' }, [saveLights, clearLights]), profileNote);
    body.append(monitorRow, h('div', { class: 'extra-group' }, [h('strong', { text: 'Lightbar animation' }), field('Mode', anim), field('Cycle time (ms)', period), animApply]),
      vHead, vSpk, vMic, field('Lightbar brightness', bright), micBox, saveRow);
    micBox.append(exp, field('Microphone light', micLed));
    layout.append(box);
    refreshLightsButtons = () => { const on = state.connected; animApply.disabled = !on; micLed.disabled = !on; bright.disabled = !on; };
    renderLightsProfile = () => {
      const p = profile();
      if (p.lightbar) {
        anim.value = ANIMATIONS.some(([k]) => k === p.lightbar.mode) ? p.lightbar.mode : 'off'; period.value = String(p.lightbar.periodMs || 2000);
        profileNote.textContent = `Saved with this profile: ${String(p.lightbar.color).toUpperCase()}${p.lightbar.mode && p.lightbar.mode !== 'off' ? `, ${p.lightbar.mode}` : ''}. It is sent when the profile is applied.`;
      } else profileNote.textContent = 'Not saved with this profile. The current colour is left alone when you apply it.';
      clearLights.hidden = !p.lightbar && !p.audioPreset;
      if (p.audioPreset) {
        for (const [key, wrap] of [['headphone', vHead], ['speaker', vSpk], ['microphone', vMic]]) {
          if (Number.isInteger(p.audioPreset[key])) { vols[key] = p.audioPreset[key]; const i = wrap.querySelector('input'); i.value = String(vols[key]); i.dispatchEvent(new Event('input')); }
        }
      }
    };
    refreshLightsButtons(); renderLightsProfile(); loadMonitors();
  }
  async function applyProfileLights(p, dispatch) {
    // Colour and player lights must land; animation and audio levels are best-effort so a
    // rejection warns without making the whole profile apply read as failed.
    const soft = async (command, what) => {
      let ok = false;
      try { ok = await dispatch(command, true); } catch { ok = false; }
      if (!ok) toast(`${what} was not applied. The rest of the profile was sent.`, { error: true });
      return ok;
    };
    if (p.lightbar && /^#[0-9a-f]{6}$/i.test(p.lightbar.color || '')) {
      if (!await dispatch({ action: 'setLightbar', rgb: hexToRgb(p.lightbar.color) })) return false;
      if (Number.isInteger(p.lightbar.playerLeds) && !await dispatch({ action: 'setPlayerLeds', mask: clamp(p.lightbar.playerLeds, 0, 31) })) return false;
      if (p.lightbar.mode && p.lightbar.mode !== 'off') await soft({ action: 'setLightbarAnimation', mode: p.lightbar.mode, rgb: hexToRgb(p.lightbar.color), periodMs: clamp(Math.round(p.lightbar.periodMs || 2000), 300, 20000) }, 'The lightbar animation');
    }
    if (p.audioPreset) {
      const a = p.audioPreset, cmd = { action: 'setAudioVolumes' };
      for (const [key, max] of [['headphone', 127], ['speaker', 100], ['microphone', 127]]) if (Number.isInteger(a[key])) cmd[key] = clamp(a[key], 0, max);
      if (Object.keys(cmd).length > 1) await soft(cmd, 'The audio levels');
    }
    return true;
  }

  /* ---------- Steam Input, telemetry source ---------- */
  async function checkSteam() {
    const warn = $('#steam-input-warning');
    if (!native || !warn) return;
    try {
      const s = await invoke('steam_input_status');
      warn.hidden = !s?.mayConflict;
      if (s?.mayConflict) warn.textContent = `Steam Input may be grabbing the controller. ${s.note || 'Turn off PlayStation Configuration Support in Steam, or games may see doubled input.'}`;
    } catch { warn.hidden = true; }
  }
  $('[data-view="profiles"]')?.addEventListener('click', checkSteam);
  async function loadTelemetrySource() {
    const input = $('#telemetry-allowed-source'), status = $('#telemetry-source-status');
    if (!native) { $('#telemetry-source-row').hidden = true; return; }
    try {
      const ip = await invoke('get_telemetry_allowed_source');
      input.value = ip || '';
      status.textContent = ip ? `Only ${ip} can send telemetry over the LAN.` : 'Any sender on the allowed network.';
    } catch { /* leave the default text */ }
  }
  $('#telemetry-source-save').addEventListener('click', async () => {
    const value = $('#telemetry-allowed-source').value.trim();
    try {
      const ip = await invoke('set_telemetry_allowed_source', { ip: value || null });
      $('#telemetry-allowed-source').value = ip || '';
      $('#telemetry-source-status').textContent = ip ? `Only ${ip} can send telemetry over the LAN.` : 'Restriction cleared. Any sender on the allowed network.';
    } catch (error) { $('#telemetry-source-status').textContent = `Could not save: ${error}`; }
  });

  /* ---------- diagnostics ---------- */
  async function diagnosticsLines() {
    const lines = [
      '', 'Environment',
      `App version: ${$('#app-version')?.textContent || 'unknown'}`,
      `OS: ${navigator.userAgentData?.platform || navigator.platform || 'unknown'} (${navigator.userAgent})`,
      `${$('#core-status')?.textContent || 'Core: unknown'} · service ${$('#core-service-chip')?.textContent || 'n/a'}`,
    ];
    let log = null;
    if (native) {
      try {
        const r = await invoke('export_diagnostics', { save: false });
        const rep = r?.report;
        if (rep) {
          lines.push(`Core build: ${rep.coreBuild ?? 'n/a'} · ${rep.os}/${rep.arch}${rep.windowsBuild ? ` · Windows build ${rep.windowsBuild}` : ''}`);
          if (rep.core?.error) lines.push(`Core error: ${rep.core.error}`);
          if (Array.isArray(rep.log)) log = rep.log.slice(-40);
        }
      } catch { /* fall back to the visible log */ }
    }
    if (!log) log = ['(app log unavailable here; use Copy log on this page for the session log)'];
    lines.push('', 'Recent app log', ...(log.length ? log : ['(empty)']));
    return lines;
  }
  $('#export-diagnostics').addEventListener('click', async event => {
    const button = event.currentTarget, status = $('#diag-export-status');
    status.hidden = false;
    if (!native) { status.textContent = 'Exporting diagnostics works in the installed app. Use Copy diagnostics here.'; return; }
    button.disabled = true; status.textContent = 'Saving…';
    try {
      const r = await invoke('export_diagnostics', { save: true });
      status.textContent = r?.path ? `Saved to ${r.path}` : 'Diagnostics were not saved.';
      if (r?.path) toast('Diagnostics saved.', { error: false });
    } catch (error) { status.textContent = `Could not save diagnostics: ${error}`; }
    finally { button.disabled = false; }
  });

  /* ---------- wire up ---------- */
  renderGyroExtras(); renderTouchOptions(); renderPcExtras(); renderExperimentalTriggers(); renderLightsExtras();
  const originalRenderProfile = window.studioHooks?.renderProfile;
  void originalRenderProfile;
  refreshBatteryControls(); renderService(); refreshSettings(); loadTelemetrySource();
  if (native) {
    window.__TAURI__.event?.listen?.('trayHidden', e => toast(e?.payload?.message || 'CONTROLLER is still running in the system tray.', { error: false }));
    checkSteam();
  }

  window.shellExtras = {
    event(e) { handlers[e.type]?.(e); },
    persistent, gyroExtras, pcOptions, touchOptions, monitorOption, bindingExtras, applyProfileLights,
    profileChanged() {
      profileChanged();
      $('#gyro-extras')?.render?.();
    },
    recoveryStatus, diagnosticsLines,
    chips: () => [...chips.keys()],
  };
  profileChanged();
  if (state.loadFailed) recoveryStatus(true);
})();
