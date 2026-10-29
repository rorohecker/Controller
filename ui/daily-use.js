/* Save/apply state, source inspection and local presentation preferences. */
(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const copy = value => structuredClone(value);
  const stable = value => JSON.stringify(value, (_, v) => v && typeof v === 'object' && !Array.isArray(v)
    ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b))) : v);
  const groups = [
    ['Details', ['name', 'description', 'tag', 'favorite', 'matchExe', 'matchTitle', 'matchPath', 'autoApply', 'community', 'session']],
    ['Triggers', ['triggers', 'inputEffects']], ['Haptic preview', ['haptics']],
    ['Motion and sticks', ['gyroAim', 'controls']], ['Keyboard remap', ['remap']],
    ['Game actions', ['actions', 'actionsEnabled', 'gameLink', 'weaponBindings']]
  ];
  const hardwareKeys = ['triggers', 'inputEffects', 'gyroAim', 'controls', 'remap', 'actions', 'actionsEnabled', 'gameLink'];
  const hardware = p => Object.fromEntries(hardwareKeys.map(key => [key, copy(p[key])]));
  const profileCommands = new Set(['applyTriggers', 'setInputEffects', 'setControlProfile', 'setGyroAim', 'setRemap', 'setGameActions', 'setGameLink']);
  class ControllerDailyUse {
    constructor(api) {
      this.a = api; this.savedProfiles = new Map(); this.request = 0; this.saveRequest = 0;
      this.writes = new Set(); this.pending = null; this.applied = null; this.uncertain = false;
      this.owner = ''; this.sources = {}; this.armed = new Set(); this.saveError = ''; this.applyMessage = 'Not applied';
      this.systemMotion = matchMedia('(prefers-reduced-motion: reduce)');
      this.preferences = { density: 'comfortable', numbers: true };
      try {
        const v = JSON.parse(localStorage.getItem('controller.display.v1'));
        if (v && ['compact', 'comfortable'].includes(v.density) && typeof v.numbers === 'boolean') this.preferences = v;
      } catch {}
      this.build(); this.applyPreferences(); this.mountRanges(document);
      this.visibility = new IntersectionObserver(entries => {
        this.bar.classList.toggle('is-away', !entries[0].isIntersecting);
      });
      this.visibility.observe(this.bar);
      document.addEventListener('input', () => this.changed());
      document.addEventListener('change', () => this.changed());
      document.addEventListener('click', () => this.changed());
      document.addEventListener('keydown', e => {
        if(e.key==='Escape'&&this.details.open&&this.details.contains(document.activeElement)){
          this.details.open=false;this.summary.focus();return;
        }
        if (e.key === 'Backspace' && e.shiftKey && (window.controllerPlatform.os === 'macos' ? e.metaKey : e.ctrlKey)) {
          e.preventDefault(); if (!e.repeat) this.stopAll();
        }
      });
      this.observer = new MutationObserver(records => {
        for (const record of records) for (const node of record.addedNodes) {
          if (node.nodeType === 1 && !node.closest('.range-tools')) this.mountRanges(node);
        }
      });
      this.observer.observe(document.querySelector('main'), { childList: true, subtree: true });
      this.systemMotion.addEventListener('change', () => this.renderMotion());
      document.addEventListener('controller-platform', () => {
        this.stopButton.title = `${window.controllerPlatform.primaryLabel} + Shift + Backspace: stop output and input mappings`;
      });
      this.timer = setInterval(() => { if (this.sources.lease || this.sources.audio || this.sources.report || this.sources.timeline) this.renderSources(); }, 1000);
    }
    node(tag, props, children = []) { return this.a.gameNode(tag, props, children); }
    button(label, id, run) {
      const b = this.node('button', { type: 'button', id, text: label, class: 'quiet-action' });
      b.addEventListener('click', run); return b;
    }
    build() {
      const n = this.node.bind(this);
      this.bar = n('section', { class: 'daily-bar', id: 'daily-status' });
      this.bar.setAttribute('aria-label', 'Profile state and feedback');
      this.saveLabel = n('strong', { id: 'profile-save-state', text: 'Loading profiles' });
      this.applyLabel = n('span', { id: 'profile-apply-state', text: 'Not applied' });
      const state = n('div', { class: 'daily-state' }, [this.saveLabel, this.applyLabel]);
      this.summary = n('summary', { text: 'Changes and feedback', id: 'daily-summary' });
      this.details = n('details', { class: 'daily-details' }, [this.summary]);
      this.changeList = n('p', { id: 'profile-change-summary', text: 'No profile changes' });
      this.liveSources = n('p', { id: 'feedback-sources', text: 'No active feedback' });
      this.ruleSources = n('p', { id: 'feedback-armed', text: 'No input effects armed' });
      this.actions = n('div', { class: 'daily-actions' }, [
        this.button('Save profile', 'daily-save', async () => { if (await this.a.commitProfileDraft()) this.render(); }),
        this.button('Apply profile', 'daily-apply', async () => { if (await this.a.commitProfileDraft()) await this.a.applyProfileHardware(true); }),
        this.button('Revert unsaved', 'daily-revert', () => this.revert(false)),
        this.button('Restore applied settings', 'daily-restore-applied', () => this.revert(true))
      ]);
      this.details.append(n('div', { class: 'daily-detail-content' }, [this.changeList, this.liveSources, this.ruleSources, this.actions,
        n('small', { text: 'Applied confirms profile command processing. Preview levels are saved separately; active sources show what is playing. Revert changes the editor only.' })]));
      this.stopButton = this.button('Stop all', 'stop-all', () => this.stopAll());
      this.stopButton.className = 'stop-all';
      this.stopButton.title = `${window.controllerPlatform.primaryLabel} + Shift + Backspace: stop output and input mappings`;
      this.bar.append(state, this.details, this.stopButton);
      document.querySelector('.session-rail').after(this.bar);
      this.notice = n('p', { id: 'daily-announcement', class: 'sr-only', role: 'status' });
      this.bar.after(this.notice);
      const box = n('section', { class: 'studio-card framed', id: 'layout-preferences' });
      const body = n('div', { class: 'studio-body' });
      const density = this.a.gameSelect([['comfortable', 'Comfortable'], ['compact', 'Compact']], this.preferences.density,
        value => { this.preferences.density = value; this.storePreferences(); });
      density.id = 'display-density';
      const numbers = n('input', { type: 'checkbox', checked: this.preferences.numbers, id: 'display-numbers' });
      numbers.addEventListener('change', () => { this.preferences.numbers = numbers.checked; this.storePreferences(); });
      this.motionNote = n('p', { id: 'display-motion-status' });
      body.append(this.a.gameField('Control spacing', density), n('label', { class: 'check-row', text: 'Show slider values and reset buttons' }, [numbers]),
        this.motionNote, n('p', { text: 'Tab moves between controls. Arrow keys adjust sliders. Stop all is available on every page.' }),
        this.button('Reset layout preferences', 'display-reset', () => { this.preferences = { density: 'comfortable', numbers: true }; this.storePreferences(); }));
      box.append(n('div', { class: 'panel-cap', text: 'Layout and controls' }), body);
      document.querySelector('.settings-grid').append(box);
      this.renderMotion();
    }
    reducedMotion() { return this.systemMotion.matches || document.body.classList.contains('studio-reduced-motion'); }
    renderMotion() { this.motionNote.textContent = this.reducedMotion() ? 'Reduced motion is active.' : 'Decorative motion follows your display preference.'; }
    storePreferences() {
      try { localStorage.setItem('controller.display.v1', JSON.stringify(this.preferences)); }
      catch { this.a.toast('Layout changed for this window; preferences could not be saved.'); }
      this.applyPreferences(); this.renderMotion();
    }
    applyPreferences() {
      document.body.dataset.density = this.preferences.density;
      document.body.classList.toggle('hide-range-tools', !this.preferences.numbers);
      if ($('display-density')) $('display-density').value = this.preferences.density;
      if ($('display-numbers')) $('display-numbers').checked = this.preferences.numbers;
    }
    mountRanges(root) {
      const ranges = [...(root.matches?.('input[type=range]') ? [root] : []), ...root.querySelectorAll('input[type=range]')];
      for (const range of ranges) {
        // The music thumb is part of its timeline, with its own elapsed readout.
        if (range.id === 'speaker-start' || range.dataset.numericTools || !range.id) continue;
        const label = document.querySelector(`label[for="${CSS.escape(range.id)}"]`);
        if (!label) continue;
        range.dataset.numericTools = 'true';
        const name = label.querySelector('span')?.textContent || [...label.childNodes].filter(n=>n.nodeType===3).map(n=>n.textContent).join(' ').trim() || range.id;
        const initial = range.defaultValue || range.value;
        const input = this.node('input', { type: 'number', class: 'range-number', id: `${range.id}-number-control`, value: range.value });
        input.setAttribute('aria-label', `${name} value`);
        const reset = this.button('Reset', `${range.id}-reset-control`, () => {
          const min=Number(range.min||0),max=Number(range.max||100),step=Number(range.step||1);
          const value=Math.max(min,Math.min(max,Number(initial)));
          const target=step>0?min+Math.max(0,Math.min(Math.floor((max-min)/step),Math.round((value-min)/step)))*step:value;
          setValue(target);
        });
        reset.setAttribute('aria-label', `Reset ${name}`); reset.title = `Default: ${initial}`;
        const tools = this.node('div', { class: 'range-tools' }, [input, reset]);
        const sync = () => {
          input.min = range.min; input.max = range.max; input.step = range.step || '1';
          if (document.activeElement !== input) input.value = range.value;
          input.disabled = range.disabled; reset.disabled = range.disabled;
        };
        const setValue = value => {
          const v = Number(value), min = Number(range.min || 0), max = Number(range.max || 100), step = Number(range.step || 1);
          if (!Number.isFinite(v) || v < min || v > max || step > 0 && Math.abs((v - min) / step - Math.round((v - min) / step)) > .00001) {
            this.a.toast(`Enter ${min}–${max} in steps of ${step}.`); input.value = range.value; return;
          }
          range.value = String(v); range.dispatchEvent(new Event('input', { bubbles: true }));
          range.dispatchEvent(new Event('change', { bubbles: true })); input.value = range.value; sync();
        };
        input.addEventListener('change', () => setValue(input.value));
        input.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); input.blur(); } });
        range.addEventListener('input', sync); range.addEventListener('change', sync);
        const wrapper = this.node('div', { class: 'range-with-tools' });
        range.before(wrapper); wrapper.append(range, tools); range._syncNumber = sync; sync();
      }
    }
    draft() {
      const p = copy(this.a.profile());
      for (const [id, key] of [['profile-name', 'name'], ['profile-description', 'description'], ['profile-match', 'matchExe'], ['profile-match-title', 'matchTitle'], ['profile-match-path', 'matchPath']]) {
        if ($(id)) p[key] = $(id).value.trim();
      }
      if ($('profile-auto-apply')) p.autoApply = $('profile-auto-apply').checked;
      return p;
    }
    changed() {
      if (this.scheduled) return;
      this.scheduled = true;
      queueMicrotask(() => { this.scheduled = false; this.render(); this.renderMotion();
        for (const input of document.querySelectorAll('.screen.is-current input[type=range]')) input._syncNumber?.(); });
    }
    loaded(hasArchive, error = '') {
      this.loadedOnce = true;
      this.loadError = error;
      this.savedProfiles = new Map(hasArchive ? this.a.state.profiles.map(p => [p.id, copy(p)]) : []);
      this.render();
    }
    saving() { const id = ++this.saveRequest; this.writes.add(id); this.render(); return id; }
    saved(archive, id) {
      this.writes.delete(id); this.savedProfiles = new Map(archive.profiles.map(p => [p.id, copy(p)]));
      if (id >= (this.errorRequest || 0)) this.saveError = '';
      this.loadError = '';
      this.render();
    }
    saveFailed(id, error) { this.writes.delete(id); this.errorRequest = id; this.saveError = String(error); this.announce('Profile save failed. Changes remain in the editor.'); this.render(); }
    beginApply(profile) {
      this.cancelPending(); const id = ++this.request;
      this.pending = { id, profile: copy(profile), owner: this.owner, error: '', timer: setTimeout(() => {
        if (this.pending?.id !== id) return;
        this.pending = null; this.uncertain = true; this.applyMessage = 'Apply not confirmed'; this.render();
      }, 8000) };
      this.applyMessage = 'Applying…'; this.render(); return id;
    }
    cancelPending() { if (this.pending) clearTimeout(this.pending.timer); this.pending = null; }
    commandFailed(error) { if (this.pending) this.pending.error = String(error); this.uncertain = true; this.applyMessage = 'Command failed'; this.render(); }
    abortApply(id) {
      if(this.pending?.id!==id)return;
      const message=this.pending.error||'Profile apply interrupted';this.cancelPending();this.uncertain=true;this.applyMessage=message;this.render();
    }
    command(command,ticket=null) {
      if(this.pending&&profileCommands.has(command.action)&&ticket!==this.pending.id)this.pending.error='Settings changed while the profile was applying. Apply again.';
      if (command.action === 'reset') { this.clearOutput(); this.applyMessage = 'Stopped'; this.announce('Output stopped. Apply a profile to resume.'); }
      else if (profileCommands.has(command.action) && !this.pending) { this.uncertain = true; this.applyMessage = 'Settings changed separately'; }
      this.render();
    }
    clearOutput() { this.cancelPending(); this.applied = null; this.uncertain = false; this.sources = {}; this.armed.clear(); this.renderSources(); }
    async stopAll() {
      this.a.state.autoSuppressed = true;
      this.a.getPlayer().stop('Stopped');
      // Cancel session continuations now, including commands awaiting IPC.
      window.gameSessions?.command({ action: 'reset' });
      this.cancelPending(); this.applyMessage = 'Stopping…'; this.render();
      if (!await this.a.send({ action: 'reset' })) { this.applyMessage = 'Stop not confirmed — check connection'; this.announce(this.applyMessage); this.render(); }
    }
    async revert(applied) {
      const current = this.a.profile(), saved = this.savedProfiles.get(current.id);
      if (applied) {
        if (!this.applied || this.applied.id !== current.id) return;
        Object.assign(current, copy(this.applied.settings));
      } else {
        if (!saved || this.writes.size) return;this.saveError='';
        this.a.state.profiles[this.a.state.profiles.findIndex(p => p.id === current.id)] = copy(saved);
      }
      this.a.renderAllProfileControls(); this.announce(applied ? 'Applied settings restored in the editor. Save or apply when ready.' : 'Unsaved changes reverted.');
      this.render();
    }
    announce(message) { this.notice.textContent = message; }
    pulse(kind) {
      if (this.reducedMotion()) return;
      this.bar.dataset.pulse = kind;
      clearTimeout(this.pulseTimer); this.pulseTimer = setTimeout(() => delete this.bar.dataset.pulse, 650);
    }
    render() {
      if (!this.bar) return;
      const p = this.draft(), saved = this.savedProfiles.get(p.id), edited = !saved || stable(saved) !== stable(p);
      const changed = saved ? groups.filter(([, keys]) => keys.some(key => stable(saved[key]) !== stable(p[key]))).map(([name]) => name) : ['New profile'];
      this.saveLabel.textContent = this.saveError ? 'Save failed' : this.writes.size ? 'Saving…' : !this.loadedOnce ? 'Loading profiles' : this.loadError ? 'Load failed' : edited ? 'Unsaved changes' : 'Saved';
      $('footer-profile-state').textContent = this.saveError || this.loadError ? 'Profiles not saved' : this.writes.size ? 'Saving profiles' : !this.loadedOnce ? 'Loading profiles' : edited ? 'Profile edits not saved' : 'Profiles saved locally';
      this.bar.dataset.state = this.saveError || this.loadError ? 'error' : edited ? 'edited' : 'saved';
      const matching = this.applied?.id === p.id && stable(this.applied.settings) === stable(hardware(p));
      const appliedText = this.pending ? 'Applying…' : !this.a.state.connected ? 'No controller' : this.applied && !this.uncertain ? matching ? 'Applied' : this.applied.id===p.id?'Changes not applied':'Another profile is applied' : this.applyMessage;
      this.applyLabel.textContent = appliedText;
      this.changeList.textContent = this.saveError ? `Save failed: ${this.saveError}` : this.loadError ? `Profile load failed: ${this.loadError}. The editor contains defaults.` : changed.length ? `Unsaved: ${changed.join(', ')}.` : 'Saved profile matches the editor.';
      $('daily-revert').disabled = !saved || !edited || !!this.writes.size;
      $('daily-restore-applied').disabled = !this.applied || this.applied.id !== p.id || matching;
      $('daily-apply').disabled = !this.a.state.connected || !!this.pending;
      this.renderSources();
    }
    renderSources() {
      const s = this.sources, now = performance.now(), audio = s.audio && now - s.audio.at < 4000 ? s.audio.value : null;
      const grips = [], speaker = [];
      if (audio) {
        if (audio.file && ['playing', 'starting', 'buffering'].includes(audio.file.mode)) {
          if (audio.file.grips) grips.push('Music'); if (audio.file.speaker) speaker.push('Music');
        }
        if (audio.cueActive) grips.push('Haptic cue');
        if (audio.timelineActive) grips.push('Timeline');
        if (audio.gameActive) grips.push('Xbox rumble');
        if (audio.bridge) grips.push('System audio');
      }
      const report=s.report&&now-s.report.at<4000?s.report.value:null;
      if(report?.motorSource&&report.motorSource!=='Idle')grips.push(report.motorSource);
      const timeline=s.timelineUntil>now?s.timeline:'';
      const trigger = report?.triggerSource || (timeline&&s.timelineTrigger ? `Timeline: ${timeline}` : s.lease > now ? 'Profile preview' : 'Idle');
      this.liveSources.textContent = `Triggers: ${trigger} · Grips: ${grips.join(' + ') || 'Idle'} · Speaker: ${speaker.join(' + ') || 'Idle'}${report?' \u00b7 Lightbar: '+report.lightSource:''}`;
      this.ruleSources.textContent = this.armed.size ? `Armed: ${[...this.armed].join(', ')}. Armed effects wait for input.` : 'No input effects armed.';
      this.summary.textContent = grips.length || speaker.length || timeline || report?.triggerSource&&report.triggerSource!=='Idle' || s.lease > now ? 'Changes · feedback active' : 'Changes and feedback';
    }
    event(e) {
      if(e.type==='foreground')this.changed();
      if (e.type === 'device') {
        const owner = e.connected ? e.controllerId || e.path || `${e.name}/${e.transport}` : '';
        if (owner !== this.owner || !e.connected) { this.clearOutput(); this.owner = owner; this.applyMessage = 'Not applied'; }
        this.changed(); return;
      }
      if (e.type === 'error') {
        if(this.pending)this.pending.error=e.message||'Native command failed';
        else if(this.applied){this.uncertain=true;this.applyMessage='Output error; check Diagnostics';this.render();}
      }
      if (e.type === 'profileCheckpoint' && this.pending?.id === e.requestId) {
        const pending = this.pending; this.cancelPending();
        if (pending.error || !e.connected || pending.owner !== this.owner || e.controllerId && e.controllerId !== this.owner) {
          this.uncertain = true; this.applyMessage = pending.error ? `Apply failed: ${pending.error}` : 'Controller changed during apply';
        } else {
          this.applied = { id: pending.profile.id, settings: hardware(pending.profile) };
          this.uncertain = false; this.applyMessage = 'Applied'; this.announce(`${pending.profile.name} applied.`); this.pulse('applied');
        }
        this.render();
      }
      if(e.type==='feedbackState'&&(!e.controllerId||e.controllerId===this.owner)){this.sources.report={value:e,at:performance.now()};this.renderSources();}
      if (e.type === 'audioMeters') { this.sources.audio = { value: e, at: performance.now() }; this.renderSources(); }
      if (e.type === 'triggerLease') { this.sources.lease = e.active ? performance.now() + (e.durationMs || 0) : 0; this.renderSources(); }
      if (e.type === 'effectTimeline') { this.sources.timeline = e.playing ? e.name || 'Clip' : ''; this.sources.timelineTrigger=!!e.triggerMask; this.sources.timelineUntil=e.playing?performance.now()+Math.max(0,(e.durationMs||8000)-(e.positionMs||0)):0; this.renderSources(); if (e.playing) this.pulse('feedback'); }
      const rule = { inputEffects: 'Weapon/bow input', gameActionsStatus: 'Game actions', telemetryStatus: 'Telemetry', scriptStatus: 'Script' }[e.type];
      if (rule) { if (e.enabled) this.armed.add(rule); else this.armed.delete(rule); this.renderSources(); }
      if (e.type === 'gameAction') { this.announce(`Game action: ${e.name}`); this.pulse('feedback'); }
      if (e.type === 'safetyReset' || e.type === 'core' && ['stopped', 'failed'].includes(e.status)) { this.clearOutput(); this.applyMessage = 'Stopped'; this.changed(); }
    }
  }
  window.ControllerDailyUse = ControllerDailyUse;
})();
