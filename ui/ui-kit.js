/* UI kit: progressive disclosure for every page.
   Declarative convention (read from the DOM, so panels never depend on script order):
     data-ui-section="<page>.<name>"  unique id
     data-level="basic|advanced|experimental|tool"
     data-title, data-hint, data-keywords (space separated palette synonyms), data-default-open="true|false"
   basic        -> plain, always-open section (no wrapper)
   advanced/tool/experimental -> the ORIGINAL element is wrapped in <details class="ui-section"> (never cloned);
                   an element that already is a <details> is enhanced in place instead.
   Everything works uncollapsed when this file is absent: the attributes are inert markup. */
(() => {
  'use strict';
  if (window.controllerUI) return;
  const KEY = 'controller.ui.v1';
  const BADGE = { advanced: 'Advanced', experimental: 'Experimental', tool: 'Tool' };
  const LEVELS = ['basic', 'advanced', 'experimental', 'tool'];
  const FOCUSABLE = 'input:not([type=hidden]):not(:disabled),select:not(:disabled),textarea:not(:disabled),button:not(:disabled),summary';

  /* ---------- persisted state (always try/catch: storage can be blocked) ---------- */
  const fresh = () => ({ v: 1, open: {}, showExperimental: false, showAll: false, navTools: false });
  const load = () => {
    try {
      const raw = JSON.parse(localStorage.getItem(KEY));
      if (raw && raw.v === 1) {
        const open = {};
        if (raw.open && typeof raw.open === 'object') for (const [k, v] of Object.entries(raw.open)) open[k] = !!v;
        return { v: 1, open, showExperimental: !!raw.showExperimental, showAll: !!raw.showAll, navTools: !!raw.navTools };
      }
    } catch { /* fall through */ }
    return fresh();
  };
  let store = load();
  const persist = () => { try { localStorage.setItem(KEY, JSON.stringify(store)); } catch { /* ignore */ } };
  const listeners = new Set();
  const emit = (reason) => { const p = prefs(); for (const cb of [...listeners]) { try { cb(p, reason); } catch { /* listener errors must not break the kit */ } } };
  const prefs = () => ({ showExperimental: store.showExperimental, showAll: store.showAll, navTools: store.navTools, open: { ...store.open } });

  /* ---------- registry ---------- */
  const sections = new Map(); // id -> { id, el, root, level, title, hint, keywords, defaultOpen, summary, badgeOn }
  let seq = 0;
  const txt = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const level = (v) => (LEVELS.includes(v) ? v : 'advanced');
  const isOpenNow = (sec) => (sec.root.tagName === 'DETAILS' ? sec.root.open : true);

  function setOpen(sec, open, user) {
    const d = sec.root;
    if (d.tagName !== 'DETAILS' || d.open === !!open) return;
    if (!user) d._uiProg = true;
    d.open = !!open;
  }
  /* With the experimental gate on, an experimental section (or a section that holds one) is open by default,
     so what the user just asked to see is not hidden behind a collapsed summary. An explicit choice still wins. */
  const holdsExperimental = (sec) => sec.level === 'experimental' || !!sec.el.querySelector('[data-level="experimental"]');
  const wantOpen = (sec) => {
    if (sec.level === 'basic') return true;
    if (store.showAll && sec.el.dataset.uiAllOpen !== 'false' && !sec.userClosedInAll) return true;
    if (sec.id in store.open) return store.open[sec.id];
    return sec.defaultOpen || (store.showExperimental && holdsExperimental(sec));
  };

  function decorate(sec, summary) {
    const badge = txt('span', 'ui-badge', BADGE[sec.level] || '');
    badge.dataset.level = sec.level;
    const on = txt('span', 'ui-badge ui-badge-on', 'On');
    on.hidden = true;
    sec.badgeOn = on;
    summary.append(badge, on);
  }

  function enhance(el) {
    const id = el.dataset.uiSection;
    if (!id || el._uiSec) return el._uiSec || null;
    const known = sections.get(id);
    if (known && known.el !== el && known.el.isConnected) return null; // duplicate id: the first live element wins
    if (known) dispose(known);
    const sec = {
      id, el, level: level(el.dataset.level), title: el.dataset.title || id, hint: el.dataset.hint || '',
      keywords: el.dataset.keywords || '', defaultOpen: el.dataset.defaultOpen === 'true',
    };
    el.dataset.level = sec.level;
    if (sec.level === 'basic') {
      sec.root = el;
      el.classList.add('ui-basic');
    } else if (el.tagName === 'DETAILS') {
      sec.root = el;
      el.classList.add('ui-section', 'ui-native');
      let summary = el.querySelector(':scope > summary');
      if (!summary) { summary = document.createElement('summary'); summary.textContent = sec.title; el.prepend(summary); }
      decorate(sec, summary);
      el.addEventListener('toggle', () => onToggle(sec));
      setOpen(sec, wantOpen(sec));
    } else {
      const d = document.createElement('details');
      d.className = 'ui-section';
      d.dataset.level = sec.level;
      d.dataset.uiWrap = id;
      if (el.dataset.hapticGroup) d.dataset.hapticGroup = el.dataset.hapticGroup; // keep the haptic tab bar working
      // The tab bar may have hidden the element before it was wrapped; from now on the wrapper (its direct child
      // in the page) carries that state, so a stale class on the inner element would hide it for good.
      el.classList.remove('haptic-tab-hidden');
      const summary = document.createElement('summary');
      const main = txt('span', 'ui-sum-main');
      main.append(txt('span', 'ui-sum-title', sec.title));
      summary.append(main);
      decorate(sec, main);
      if (sec.hint) summary.append(txt('span', 'ui-sum-hint', sec.hint));
      const body = txt('div', 'ui-section-body');
      d.append(summary, body);
      if (el.parentNode) el.parentNode.insertBefore(d, el);
      body.append(el);
      sec.root = d;
      d.addEventListener('toggle', () => onToggle(sec));
      setOpen(sec, wantOpen(sec));
      // Keep the wrapper hidden whenever the module hides its own element.
      const sync = () => { d.hidden = el.hidden; };
      sync();
      new MutationObserver(sync).observe(el, { attributes: true, attributeFilter: ['hidden'] });
    }
    el._uiSec = sec;
    sections.set(id, sec);
    refreshActive(sec);
    return sec;
  }

  function dispose(sec) {
    sections.delete(sec.id);
    if (sec.root !== sec.el && sec.root.isConnected && !sec.root.contains(sec.el)) sec.root.remove();
  }

  function onToggle(sec) {
    const d = sec.root;
    if (d._uiProg) { d._uiProg = false; return; }
    // The user's own choice always persists. Under "Expand all" a collapse is also remembered as an
    // override, so the section stays closed for the rest of Expand all and keeps that state afterwards.
    if (store.showAll) sec.userClosedInAll = !d.open;
    store.open[sec.id] = d.open;
    persist();
  }

  /* ---------- experimental gate ---------- */
  function isActive(sec) {
    const el = sec.el;
    if (el.dataset.uiActive === 'true') return true;
    return !!el.querySelector('button[aria-pressed="true"]');
  }
  /* Every write below is guarded: an unchanged write still queues a mutation record, and the
     observer that triggered this scan would see its own records forever. */
  const el_toggle = (el, cls, on) => { if (el.classList.contains(cls) !== !!on) el.classList.toggle(cls, !!on); };
  function refreshActive(sec) {
    const on = sec.level === 'experimental' && isActive(sec);
    el_toggle(sec.root, 'ui-visible', on);
    el_toggle(sec.el, 'ui-visible', on);
    if (sec.badgeOn && sec.badgeOn.hidden === on) sec.badgeOn.hidden = !on;
    const flag = on ? 'true' : 'false';
    if (sec.root.dataset.uiOn !== flag) sec.root.dataset.uiOn = flag;
  }

  function applyBody() {
    const body = document.body;
    if (!body) return;
    const exp = String(store.showExperimental || store.showAll), all = String(store.showAll);
    if (body.dataset.showExperimental !== exp) body.dataset.showExperimental = exp;
    if (body.dataset.showAll !== all) body.dataset.showAll = all;
  }

  function applyAll() {
    applyBody();
    for (const sec of sections.values()) {
      if (sec.root !== sec.el || sec.root.tagName === 'DETAILS') setOpen(sec, wantOpen(sec));
    }
    syncNav();
    syncExpandables();
    syncControls();
  }

  const checkbox = () => document.getElementById('ui-show-experimental');
  const expandBtn = () => document.getElementById('ui-expand-all');
  function syncControls() {
    const c = checkbox();
    if (c && c.checked !== store.showExperimental) c.checked = store.showExperimental;
    const b = expandBtn();
    // The label stays constant; aria-pressed carries the state.
    if (b && b.getAttribute('aria-pressed') !== String(store.showAll)) b.setAttribute('aria-pressed', String(store.showAll));
  }

  /* Plain <details> that are not sections (session status, "More actions"): open under "Expand all", else as the user left them. */
  function syncExpandables() {
    for (const d of document.querySelectorAll('details[data-ui-expand]')) {
      if (!d._uiBound) {
        d._uiBound = true;
        d.addEventListener('toggle', () => { if (d._uiProg) { d._uiProg = false; return; } d._uiUser = d.open; });
      }
      const want = store.showAll || !!d._uiUser;
      if (d.open !== want) { d._uiProg = true; d.open = want; }
    }
  }

  /* ---------- navigation group (Calibration, Diagnostics) ---------- */
  const navGroup = () => document.querySelector('details[data-nav-group="tools"]');
  function syncNav(view) {
    const g = navGroup();
    if (!g) return;
    const current = view || window.controllerStudio?.state?.currentView || location.hash.slice(1);
    const want = store.showAll || store.navTools || current === 'calibration' || current === 'diagnostics';
    if (g.open !== want) { g._uiProg = true; g.open = want; }
  }
  function bindNav() {
    const g = navGroup();
    if (!g || g._uiBound) return;
    g._uiBound = true;
    g.addEventListener('toggle', () => {
      if (g._uiProg) { g._uiProg = false; return; }
      if (store.showAll) return;
      store.navTools = g.open;
      persist();
    });
    syncNav();
  }
  document.addEventListener('controller-view', (e) => syncNav(e.detail?.view));
  window.addEventListener('hashchange', () => syncNav());

  /* ---------- disabled-state explanations ---------- */
  const whyGroups = new Map(); // container -> Map(reason -> { note, els:Set })
  let whySeq = 0;
  function why(el, reason) {
    if (!el) return el;
    const prev = el._uiWhy;
    if (prev) {
      if (prev.reason === (reason || null)) return el;
      prev.els.delete(el);
      if (!prev.els.size) { prev.note.remove(); prev.group.delete(prev.reason); }
      // Restore what the element had before we explained it (it may have had its own title or description).
      const orig = el._uiWhyOrig || {};
      if (el.getAttribute('aria-describedby') === el._uiWhyDesc) {
        if (orig.describedby) el.setAttribute('aria-describedby', orig.describedby); else el.removeAttribute('aria-describedby');
      }
      if (el.title === prev.reason) {
        if (orig.title != null) el.setAttribute('title', orig.title); else el.removeAttribute('title');
      }
      el._uiWhy = null;
      el._uiWhyOrig = null;
      el._uiWhyDesc = null;
    }
    if (!reason) return el;
    const container = el.closest('[data-ui-section],.work-panel,.controls-panel,.screen') || document.body;
    let group = whyGroups.get(container);
    if (!group) { group = new Map(); whyGroups.set(container, group); }
    let entry = group.get(reason);
    if (!entry || !entry.note.isConnected) {
      const note = txt('small', 'ui-why', reason);
      note.id = `ui-why-${++whySeq}`;
      note.setAttribute('role', 'note');
      // Sit after the whole button row, never between siblings of a flex or grid row.
      let anchor = el.closest('label') && el.closest('label').parentNode === el.parentNode ? el.closest('label') : el;
      const isRow = (n) => /flex|grid/.test(getComputedStyle(n).display);
      for (let hops = 0; el.isConnected && anchor.parentElement && anchor.parentElement !== container && hops < 3 && isRow(anchor.parentElement); hops++) anchor = anchor.parentElement;
      anchor.after(note);
      entry = { note, els: new Set(), reason, group };
      group.set(reason, entry);
    }
    entry.els.add(el);
    el._uiWhy = entry;
    el._uiWhyOrig = { title: el.getAttribute('title'), describedby: el.getAttribute('aria-describedby') };
    const described = [el._uiWhyOrig.describedby, entry.note.id].filter(Boolean).join(' ');
    el._uiWhyDesc = described;
    el.setAttribute('aria-describedby', described);
    el.title = reason;
    return el;
  }

  /* ---------- reveal / navigate ---------- */
  function resolve(x) {
    if (x instanceof Element) return x;
    if (typeof x !== 'string' || !x) return null;
    try {
      return document.querySelector(`[data-ui-section="${CSS.escape(x)}"]`) || document.getElementById(x) || document.querySelector(x);
    } catch { return null; }
  }
  function revealHapticTab(el) {
    const tabs = document.getElementById('haptic-tabs');
    if (!tabs) return;
    const holder = el.closest('#haptics > *');
    if (!holder || !holder.classList.contains('haptic-tab-hidden')) return;
    const group = (holder.dataset.hapticGroup || el.closest('[data-haptic-group]')?.dataset.hapticGroup || 'studio');
    (tabs.querySelector(`[data-haptic-tab="${group}"]:not([hidden])`) || tabs.querySelector('[data-haptic-tab="all"]'))?.click();
  }
  function openFor(target) {
    const el = resolve(target);
    if (!el) return false;
    const screen = el.closest('.screen');
    if (screen && !screen.classList.contains('is-current')) window.controllerStudio?.navigate?.(screen.id);
    // An experimental section is only reachable with the gate on: turn it on (and say so) instead of
    // forcing one section visible, so the page, the checkbox and the palette stay consistent.
    for (let n = el; n && n !== document.body; n = n.parentElement) {
      const sec = n._uiSec;
      if (sec && sec.level === 'experimental' && !store.showExperimental && !store.showAll && sec.root.dataset.uiOn !== 'true') {
        api.showExperimental(true);
        window.controllerStudio?.toast?.('Experimental features are now shown. Turn them off in Settings.', { error: false });
        break;
      }
    }
    for (let n = el; n && n !== document.body; n = n.parentElement) {
      if (n.tagName === 'DETAILS') { if (!n.open) { n._uiProg = true; n.open = true; } }
    }
    revealHapticTab(el);
    const section = el._uiSec ? el : el.closest('[data-ui-section]') || el;
    const holder = section._uiSec ? section._uiSec.root : section;
    setTimeout(() => {
      holder.scrollIntoView({ block: 'start', behavior: 'auto' });
      let focus = el.matches(FOCUSABLE) ? el : (el.querySelector(FOCUSABLE) || holder.querySelector?.('summary'));
      if (focus && focus.offsetParent !== null) focus.focus({ preventScroll: true });
      holder.classList.add('ui-highlight');
      setTimeout(() => holder.classList.remove('ui-highlight'), 1800);
    }, 0);
    return true;
  }

  /* ---------- public API ---------- */
  const api = {
    section(opts = {}) {
      const el = opts.el || document.createElement('div');
      if (!el.dataset.uiSection) el.dataset.uiSection = opts.id || `custom.${++seq}`;
      if (opts.level) el.dataset.level = opts.level;
      if (opts.title) el.dataset.title = opts.title;
      if (opts.hint) el.dataset.hint = opts.hint;
      if (opts.keywords) el.dataset.keywords = Array.isArray(opts.keywords) ? opts.keywords.join(' ') : opts.keywords;
      if (opts.defaultOpen != null) el.dataset.defaultOpen = String(!!opts.defaultOpen);
      const sec = enhance(el);
      if (!sec) return null;
      return {
        root: sec.root, body: sec.el,
        open: () => { setOpen(sec, true, true); }, close: () => { setOpen(sec, false, true); }, isOpen: () => isOpenNow(sec),
      };
    },
    wrap(el, opts = {}) { return api.section({ ...opts, el }); },
    openFor,
    index() {
      return [...sections.values()].map((s) => ({
        id: s.id, title: s.title, level: s.level, hint: s.hint,
        keywords: s.keywords ? s.keywords.split(/\s+/).filter(Boolean) : [],
        page: s.el.closest('.screen')?.id || '',
      }));
    },
    prefs,
    onChange(cb) { listeners.add(cb); return () => listeners.delete(cb); },
    showAll(on) {
      if (on === undefined) return store.showAll;
      store.showAll = !!on;
      for (const sec of sections.values()) sec.userClosedInAll = false; // overrides last for one Expand all
      persist(); applyAll(); emit('showAll'); return store.showAll;
    },
    showExperimental(on) {
      if (on === undefined) return store.showExperimental;
      store.showExperimental = !!on; persist(); applyAll(); emit('showExperimental'); return store.showExperimental;
    },
    /* Open a section by default, only while the user has not chosen either way. Never persisted. */
    openIfUnset(id) { const s = sections.get(id); if (s && !(s.id in store.open)) setOpen(s, true); },
    resetOpen() { store.open = {}; persist(); applyAll(); emit('open'); },
    why,
    isOpen(id) { const s = sections.get(id); return s ? isOpenNow(s) : false; },
    ready: true,
  };
  window.controllerUI = api;

  /* ---------- scanning ---------- */
  let scanQueued = false;
  function scan() {
    scanQueued = false;
    for (const el of document.querySelectorAll('[data-ui-section]')) if (!el._uiSec) enhance(el);
    for (const sec of [...sections.values()]) {
      if (!sec.el.isConnected && !(sec.root.isConnected && sec.root.contains(sec.el))) dispose(sec);
      else if (sec.level === 'experimental') refreshActive(sec);
    }
    bindNav();
    syncExpandables();
    syncControls();
  }
  const queue = () => { if (!scanQueued) { scanQueued = true; setTimeout(scan, 0); } };

  function start() {
    applyBody();
    scan();
    applyAll();
    // Only structural changes (elements added or removed) and the attributes the kit reads matter.
    // Text-only edits (a waveform caption redrawn every frame) never need a rescan.
    new MutationObserver((records) => {
      for (const r of records) {
        if (r.type === 'attributes') { if (r.target.getAttribute(r.attributeName) !== r.oldValue) { queue(); return; } continue; }
        for (const list of [r.addedNodes, r.removedNodes]) for (const n of list) if (n.nodeType === 1) { queue(); return; }
      }
    }).observe(document.documentElement, {
      childList: true, subtree: true, attributes: true, attributeOldValue: true,
      attributeFilter: ['data-ui-section', 'aria-pressed', 'data-ui-active'],
    });
    document.addEventListener('change', (e) => {
      if (e.target && e.target.id === 'ui-show-experimental') api.showExperimental(e.target.checked);
    });
    document.addEventListener('click', (e) => {
      const t = e.target.closest?.('#ui-expand-all,#ui-reset-open');
      if (!t) return;
      if (t.id === 'ui-expand-all') api.showAll(!store.showAll);
      else api.resetOpen();
    });
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
})();
