/* File transport. Commands are ordered; every track has an identity so stale
   chunks and acknowledgements cannot revive stopped playback. */
(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const format = n => `${Math.floor(n / 60)}:${String(Math.floor(n % 60)).padStart(2, '0')}`;
  /* Waveform previews: previewEnvelope request -> `envelope` event. Needs a native file path. */
  const envelopes = window.controllerEnvelope = {
    pending: new Map(), cache: new Map(), next: 7_000_000,
    // A changed file (size or modified time, when the backend reports them) must not reuse a stale envelope.
    stamp(info) { return info ? ['modifiedMs', 'modified', 'mtime', 'size', 'bytes'].map(k => info[k] ?? '').join(':') : ''; },
    request(send, path, kind, points = 256, stamp = '') {
      if (!path || typeof send !== 'function') return Promise.resolve(null);
      const key = `${path}\n${kind}\n${points}\n${stamp}`;
      if (this.cache.has(key)) return Promise.resolve(this.cache.get(key));
      const id = ++this.next;
      return new Promise(resolve => {
        const timer = setTimeout(() => { this.pending.delete(id); resolve(null); }, 20_000);
        this.pending.set(id, { path, kind, key, resolve, timer });
        Promise.resolve(send({ action: 'previewEnvelope', path, kind, points, id })).then(ok => {
          if (ok === false && this.pending.delete(id)) { clearTimeout(timer); resolve(null); }
        }, () => { if (this.pending.delete(id)) { clearTimeout(timer); resolve(null); } });
      });
    },
    handle(event) {
      if (!event || event.type !== 'envelope') return false;
      const entry = this.pending.get(event.id);
      if (!entry || entry.path !== event.path || entry.kind !== event.kind) return false;
      this.pending.delete(event.id); clearTimeout(entry.timer);
      const values = Array.isArray(event.points) && !event.error ? event.points.map(v => Math.max(0, Math.min(1, Number(v) || 0))) : null;
      if (values && values.length) { this.cache.set(entry.key, values); if (this.cache.size > 64) this.cache.delete(this.cache.keys().next().value); }
      entry.resolve(values && values.length ? values : null);
      return true;
    },
    draw(canvas, values) {
      const ratio = window.devicePixelRatio || 1, width = Math.max(40, canvas.clientWidth || 300), height = canvas.clientHeight || 28;
      canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
      const g = canvas.getContext('2d'); if (!g) return;
      g.clearRect(0, 0, canvas.width, canvas.height);
      if (!values || !values.length) return;
      const style = getComputedStyle(canvas);
      g.fillStyle = style.getPropertyValue('--envelope-color').trim() || style.color || '#4cc3c9';
      const bars = Math.min(values.length, Math.floor(canvas.width / (2 * ratio))), step = values.length / bars;
      for (let i = 0; i < bars; i++) {
        let peak = 0; for (let j = Math.floor(i * step); j < Math.floor((i + 1) * step) || j === Math.floor(i * step); j++) peak = Math.max(peak, values[j] || 0);
        const h = Math.max(1, peak * canvas.height), x = i * canvas.width / bars;
        g.fillRect(x, (canvas.height - h) / 2, Math.max(1, canvas.width / bars - 1), h);
      }
    }
  };
  class ControllerPlayer {
    constructor(send, connected, invoke = null) {
      this.invoke = invoke; this.nativeFile = null;
      const P = window.controllerPlatform, panel = document.querySelector('.speaker-panel');
      P?.section?.(panel, 'haptics.player', 'basic', 'Play music through the controller', 'Play a file through speaker, grips or both', { open: true, keywords: 'music audio player file play speaker grips controller track song mp3 wav' });
      const cap = document.querySelector('.speaker-panel .panel-cap span'); if (cap) cap.textContent = 'Play music through the controller';
      this.send = send; this.connected = connected; this.trackId = Date.now() * 1000;
      this.mode = 'stopped'; this.position = 0; this.base = 0; this.next = 0;
      this.duration = 0; this.decoded = null; this.chain = Promise.resolve();
      this.scrubbing = false; this.chunkPending = false; this.hasStream = false;
      $('speaker-file-play').addEventListener('click', () => this.toggle());
      $('speaker-file-stop').addEventListener('click', () => this.stop());
      $('speaker-file').addEventListener('change', e => { const file = e.target.files[0]; e.target.value = ''; this.load(file); });
      $('speaker-route').addEventListener('change', () => this.stop());
      this.extraKinds();
      $('speaker-content-kind').addEventListener('change', () => {
        this.drawEnvelope();
        if(!this.nativeFile&&!['music','haptic'].includes($('speaker-content-kind').value)){$('speaker-content-kind').value='music';this.stop('Choose a native audio file to use conversion modes.');return;}
        if(this.nativeFile)this.rememberMode(this.nativeFile.name,$('speaker-content-kind').value);
        this.updateBadge();
        $('speaker-content-note').textContent={music:'Convert bass and higher notes for the grips.',bass:'Focus grip feedback on low frequencies.',impact:'Emphasize onsets; steady sounds fade.',texture:'Turn higher notes into a tactile carrier.',haptic:'Keep the stereo haptic waveform.',beats:'Pulse on detected beats.','bands:1,1,1':'Low, mid and high bands weighted equally.','bands:1.5,1,0.5':'Low, mid and high bands with extra bass.','bands:0.5,1,1.5':'Low, mid and high bands with extra treble.'}[$('speaker-content-kind').value];
        this.stop('Conversion changed. Press Play.');
      });
      $('speaker-native-file').addEventListener('click', () => this.chooseNative());
      const updatePicker=()=>{ $('speaker-file').hidden=!!window.controllerPlatform.nativeAudioFiles; };
      document.addEventListener('controller-platform',updatePicker);updatePicker();
      const seek = $('speaker-start');
      seek.addEventListener('pointerdown', () => this.scrubbing = true);
      seek.addEventListener('keydown', () => this.scrubbing = true);
      seek.addEventListener('input', () => $('speaker-time').textContent = `${format(+seek.value)} / ${format(this.duration)}`);
      seek.addEventListener('change', () => this.seek(+seek.value));
      seek.addEventListener('blur', () => { this.scrubbing = false; this.render(); });
      window.addEventListener('pointerup', () => { this.scrubbing = false; });
      document.addEventListener('controller-view',()=>this.render());
      document.addEventListener('visibilitychange',()=>this.render());
      this.buildExtras();
      this.render();
    }
    extraKinds() {
      const select = $('speaker-content-kind'); if (!select) return;
      for (const [value, text] of [['beats', 'Beats'], ['bands:1,1,1', 'Three-band · balanced'], ['bands:1.5,1,0.5', 'Three-band · bass-weighted'], ['bands:0.5,1,1.5', 'Three-band · treble-weighted']])
        if (![...select.options].some(o => o.value === value)) select.add(new Option(text, value));
    }
    drawEnvelope() {
      const canvas = this.strip; if (!canvas) return;
      const token = ++this.envelopeToken || (this.envelopeToken = 1);
      canvas.hidden = !this.nativeFile;
      if (!this.nativeFile) { envelopes.draw(canvas, null); return; }
      const kind = $('speaker-content-kind').value;
      envelopes.draw(canvas, null);
      envelopes.request(this.send, this.nativeFile.path, kind, 256, envelopes.stamp(this.nativeFile)).then(values => {
        if (token !== this.envelopeToken || !this.nativeFile) return;
        canvas.title = values ? `Energy envelope (${$('speaker-content-kind').selectedOptions[0]?.textContent || kind})` : 'Envelope preview unavailable';
        envelopes.draw(canvas, values);
      });
    }
    modeMemory() { try { const v = JSON.parse(localStorage.getItem('controller.trackModes')); return v && typeof v === 'object' ? v : {}; } catch { return {}; } }
    rememberMode(name, kind) { if (!name) return; const all = this.modeMemory(); all[name] = kind; const keys = Object.keys(all); if (keys.length > 100) delete all[keys[0]]; try { localStorage.setItem('controller.trackModes', JSON.stringify(all)); } catch {} }
    updateBadge() {
      const kind = $('speaker-content-kind'), badge = this.badge; if (!badge) return;
      badge.textContent = kind.selectedOptions[0]?.textContent || kind.value;
      badge.title = 'Conversion mode for this track';
    }
    remember() {
      if (!this.nativeFile) return;
      try { localStorage.setItem('controller.lastTrack', JSON.stringify({ info: this.nativeFile, position: Math.floor(this.current()) })); } catch {}
    }
    useNative(info, mode) {
      this.nativeFile = info; this.decoded = { native: true }; this.duration = info.duration; this.mode = 'stopped'; this.loopA = this.loopB = null;
      $('speaker-track-name').textContent = info.name; $('speaker-track-name').title = info.name;
      $('speaker-file-format').textContent = `${info.channels === 1 ? 'Mono' : 'Stereo'} / ${(info.sampleRate / 1000).toLocaleString(undefined, { maximumFractionDigits: 3 })} kHz`;
      const saved = mode || this.modeMemory()[info.name], select = $('speaker-content-kind');
      if (saved && [...select.options].some(o => o.value === saved)) select.value = saved;
      this.updateBadge(); this.drawEnvelope();
      const queue = this.queue || (this.queue = []), at = queue.findIndex(q => q.path === info.path);
      if (at < 0) { queue.push(info); if (queue.length > 12) queue.shift(); }
      this.queueIndex = queue.findIndex(q => q.path === info.path);
      this.renderQueue();
    }
    buildExtras() {
      const name = $('speaker-track-name'), host = $('speaker-file-stop').parentElement, seek = $('speaker-start');
      this.badge = document.createElement('span'); this.badge.className = 'player-mode-badge'; name.after(this.badge); this.updateBadge();
      const strip = document.createElement('canvas'); strip.className = 'player-energy-strip'; strip.setAttribute('aria-hidden', 'true'); strip.hidden = true; this.strip = strip; seek.after(strip);
      const style = document.createElement('style');
      style.textContent = '.player-mode-badge{display:inline-block;margin-left:8px;padding:1px 8px;border-radius:10px;font-size:.72rem;background:var(--teal-deep,#176f7b);color:var(--cream,#f0f8f7);vertical-align:middle}.player-energy-strip{display:block;width:100%;height:28px;margin:2px 0;border-radius:3px;color:var(--teal,#4cc3c9);opacity:.75}.player-energy-strip[hidden]{display:none}.player-extras{display:flex;flex-wrap:wrap;gap:6px;align-items:center;margin-top:8px}.player-extras .ui-note{flex-basis:100%}.player-extras button[aria-pressed=true]{outline:2px solid var(--teal,#54c6c5)}';
      document.head.append(style);
      const row = document.createElement('div'); row.className = 'player-extras'; row.id = 'player-extras'; window.controllerPlatform?.section?.(row, 'haptics.player-queue', 'advanced', 'Queue and loop', 'Previous, next, shuffle, repeat and A-B loop', { keywords: 'queue playlist shuffle repeat loop previous next reopen' });
      const btn = (text, id, run, label) => { const b = document.createElement('button'); b.type = 'button'; b.className = 'quiet-action'; b.id = id; b.textContent = text; if (label) b.setAttribute('aria-label', label); b.addEventListener('click', run); row.append(b); return b; };
      this.prevBtn = btn('Previous track', 'player-prev', () => this.step(-1));
      this.nextBtn = btn('Next track', 'player-next', () => this.step(1));
      this.shuffleBtn = btn('Shuffle: off', 'player-shuffle', () => { this.shuffle = !this.shuffle; this.shuffleBtn.textContent = `Shuffle: ${this.shuffle ? 'on' : 'off'}`; this.shuffleBtn.setAttribute('aria-pressed', String(this.shuffle)); });
      this.repeat = 'off';
      this.repeatBtn = btn('Repeat: off', 'player-repeat', () => { this.repeat = { off: 'one', one: 'all', all: 'off' }[this.repeat]; this.repeatBtn.textContent = `Repeat: ${this.repeat}`; });
      this.abBtn = btn('Loop A', 'player-ab', () => {
        const here = Math.floor(this.current() * 10) / 10;
        if (this.loopA == null) { this.loopA = here; this.loopB = null; this.abBtn.textContent = 'Loop B'; }
        else if (this.loopB == null) { if (here <= this.loopA + .5) { this.loopStatus('Choose B at least half a second after A.'); return; } this.loopB = here; this.abBtn.textContent = 'Clear loop'; }
        else { this.loopA = this.loopB = null; this.abBtn.textContent = 'Loop A'; }
        this.loopStatus(this.loopA == null ? 'Loop cleared.' : this.loopB == null ? `A set at ${format(this.loopA)}.` : `Looping ${format(this.loopA)} to ${format(this.loopB)}.`);
      });
      this.reopen = btn('Reopen last track', 'player-reopen', () => this.reopenLast());
      this.queueNote = document.createElement('span'); this.queueNote.className = 'ui-note'; this.queueNote.id = 'player-queue-note'; this.queueNote.setAttribute('role', 'status'); row.append(this.queueNote);
      host.after(row);
      let last = null; try { last = JSON.parse(localStorage.getItem('controller.lastTrack')); } catch {}
      this.last = last && last.info && typeof last.info.path === 'string' ? last : null;
      this.reopen.hidden = !this.last || !this.invoke;
      if (this.last) this.reopen.title = `${this.last.info.name} at ${format(this.last.position || 0)}`;
      this.renderQueue();
      document.addEventListener('keydown', e => this.key(e));
      window.addEventListener('pagehide', () => this.remember());
      setInterval(() => this.remember(), 5000);
    }
    loopStatus(text) { this.queueNote.textContent = text; }
    renderQueue() {
      if (!this.nextBtn) return;
      const n = (this.queue || []).length;
      this.prevBtn.disabled = this.nextBtn.disabled = n < 2;
      this.queueNote.textContent = n ? `Queue: ${n} track${n === 1 ? '' : 's'} chosen this session.` : 'Choose native tracks to build a queue.';
    }
    async reopenLast() {
      if (!this.last) return;
      await this.stop();
      this.useNative(this.last.info);
      this.position = Math.max(0, Math.min(this.duration, this.last.position || 0));
      $('speaker-file-status').textContent = 'Ready. If the file moved, press Play to see an error.'; this.render();
    }
    async step(direction) {
      const queue = this.queue || []; if (queue.length < 2) return;
      let index = this.shuffle ? Math.floor(Math.random() * queue.length) : (this.queueIndex + direction + queue.length) % queue.length;
      if (this.shuffle && index === this.queueIndex) index = (index + 1) % queue.length;
      await this.stop(); this.useNative(queue[index]); this.render();
      if (this.connected($('speaker-route').value)) this.startAt(0);
    }
    key(e) {
      if (e.defaultPrevented || e.repeat || e.ctrlKey || e.metaKey || e.altKey || !this.decoded || $('haptics').hidden) return;
      if (document.querySelector('dialog[open]')) return;
      const t = e.target; if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT|BUTTON|SUMMARY|A)$/.test(t.tagName) || t.closest?.('[role=tab],[role=slider],[role=radio]'))) return;
      if (document.querySelector('.ui-confirm-backdrop')) return;
      if (e.code === 'Space') { e.preventDefault(); this.toggle(); }
      else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') ) { e.preventDefault(); this.seek(this.current() + (e.key === 'ArrowLeft' ? -5 : 5)); }
    }
    current() {
      return Math.min(this.previewEnd??this.duration, this.position + (this.mode === 'playing' ? (performance.now() - this.started) / 1000 : 0));
    }
    command(command, id = this.trackId, unconditional = false) {
      const operation = this.chain.then(() => unconditional || id === this.trackId ? this.send(command) : false);
      this.chain = operation.catch(() => false);
      return operation;
    }
    stop(message = 'Stopped') {
      this.trackId++; this.mode = 'stopped'; this.position = 0; this.base = 0;
      this.next = 0; this.hasStream = false; this.chunkPending = false; this.previewEnd=null;
      $('speaker-file-status').textContent = message;
      this.render();
      return this.connected($('speaker-route').value)
        ? this.command({action: 'stopSpeakerSound', trackId: this.trackId}, this.trackId, true)
        : Promise.resolve(true);
    }
    async load(file) {
      const pendingStop = this.stop('No sound selected.');
      const loadId = this.trackId;
      await pendingStop;
      if (loadId !== this.trackId) return;
      this.decoded = null; this.nativeFile = null; this.duration = 0; $('speaker-file-format').textContent=''; this.loopA = this.loopB = null; if (this.abBtn) this.abBtn.textContent = 'Loop A'; this.render();
      $('speaker-track-name').textContent = file?.name || 'Choose a track';
      $('speaker-track-name').title = file?.name || '';
      if (!file) return;
      const id = this.trackId;
      if (file.size > 64_000_000) { $('speaker-file-status').textContent = 'Choose a file under 64 MB.'; return; }
      this.mode = 'loading'; this.render();
      let context;
      try {
        context = new (window.AudioContext || window.webkitAudioContext)();
        const decoded = await context.decodeAudioData(await file.arrayBuffer());
        if (id !== this.trackId) return;
        if (!decoded.duration || decoded.duration > 600) throw Error('Choose a track up to ten minutes long.');
        this.decoded = decoded; this.duration = decoded.duration; this.mode = 'stopped';
        $('speaker-track-name').textContent = file.name;
        $('speaker-track-name').title = file.name;
        $('speaker-file-status').textContent = 'Ready';
        this.drawEnvelope();
      } catch (error) {
        if (id === this.trackId) { this.mode = 'stopped'; $('speaker-file-status').textContent = `Could not read sound: ${error.message}`; }
      } finally { if (context) await context.close(); this.render(); }
    }
    async chooseNative() {
      if(!this.invoke){$('speaker-file-status').textContent=window.controllerPlatform.desktopOnlyMessage('Native audio file playback');return;}
      const button=$('speaker-native-file');button.disabled=true;
      try {
        const info=await this.invoke('choose_audio_file');if(!info)return;
        await this.stop();
        this.useNative(info);this.remember();
        $('speaker-file-status').textContent='Ready';
      } catch(error) { $('speaker-file-status').textContent=String(error); }
      finally {button.disabled=false;this.render();}
    }
    encode(start) {
      const count = Math.min(144_000, Math.ceil((this.duration - start) * 48_000));
      if (count <= 0) return null;
      const channels = Array.from({length: Math.min(2, this.decoded.numberOfChannels)}, (_, i) => this.decoded.getChannelData(i));
      const bytes = new Uint8Array(count * 2), view = new DataView(bytes.buffer);
      for (let i = 0; i < count; i++) {
        const at = (start + i / 48_000) * this.decoded.sampleRate, index = Math.floor(at), mix = at - index;
        let sample = 0;
        for (const data of channels) sample += (data[index] || 0) * (1 - mix) + (data[index + 1] || 0) * mix;
        view.setInt16(i * 2, Math.round(Math.max(-1, Math.min(1, sample / channels.length * .85)) * 32767), true);
      }
      let binary = '';
      for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
      return {pcmBase64: btoa(binary), duration: count / 48_000};
    }
    async audition(kind,position,seconds=5,onStart=null){
      if(!this.nativeFile){$('speaker-file-status').textContent='Choose a native audio file to compare conversion modes.';return false;}
      if(!this.connected('grips')){$('speaker-file-status').textContent='Connect a controller by USB to audition on the grips.';return false;}
      if(!['music','bass','impact','texture','beats'].includes(kind)&&!/^bands:[\d.,]+$/.test(kind))return false;
      const pendingStop=this.stop(),stopped=this.trackId;await pendingStop;
      if(stopped!==this.trackId)return false;
      position=Math.max(0,Math.min(this.duration,position));
      if(position>=this.duration){$('speaker-file-status').textContent='Capture a position before the end of the track.';return false;}
      return this.startAt(position,{kind,end:Math.min(this.duration,position+seconds),onStart});
    }
    async startAt(position,comparison=null) {
      const id = ++this.trackId;
      comparison?.onStart?.(id);
      this.previewEnd=comparison?.end??null;
      this.route = comparison?'grips':$('speaker-route').value; this.base = position; this.position = position;
      this.mode = 'starting'; this.next = position; this.hasStream = false;
      $('speaker-file-status').textContent = 'Opening controller audio…'; this.render();
      if(this.nativeFile){
        const ok=await this.command({action:'playAudioFile',path:this.nativeFile.path,position,duration:this.previewEnd??this.duration,kind:comparison?.kind||$('speaker-content-kind').value,route:this.route,trackId:id},id);
        if(id!==this.trackId)return;
        if(!ok)this.stop('Could not start playback. Check Diagnostics.');
        else this.hasStream=true;
        return;
      }
      const chunk = this.encode(position);
      if (!chunk) { this.stop('Finished'); return; }
      const ok = await this.command({action: 'playSpeakerSound', pcmBase64: chunk.pcmBase64, route: this.route, trackId: id}, id);
      if (id !== this.trackId) return;
      if (!ok) { this.stop('Could not start playback. Check Diagnostics.'); return; }
      this.hasStream = true; this.next = position + chunk.duration; this.pump();
    }
    async toggle() {
      if (!this.decoded || !this.connected($('speaker-route').value)) return;
      if (['playing','buffering'].includes(this.mode)) {
        this.position = this.current(); this.mode = 'paused';
        $('speaker-file-status').textContent = 'Paused'; this.render();
        const id = this.trackId;
        if (!await this.command({action: 'setSpeakerPaused', paused: true, trackId: id}, id) && id === this.trackId) this.stop('Could not pause playback.');
      } else if (this.mode === 'paused' && this.hasStream) {
        const id = this.trackId; this.mode = 'playing'; this.started = performance.now();
        $('speaker-file-status').textContent = 'Playing'; this.render();
        if (!await this.command({action: 'setSpeakerPaused', paused: false, trackId: id}, id) && id === this.trackId) this.stop('Could not resume playback.');
      } else if (this.mode !== 'loading') {
        await this.startAt(this.position >= this.duration ? 0 : this.position);
      }
    }
    seek(position) {
      const playing = ['playing','starting','buffering'].includes(this.mode), paused = this.mode === 'paused';
      this.stop(); this.position = Math.max(0, Math.min(this.duration, position)); this.scrubbing = false;
      if (playing && this.position < this.duration) this.startAt(this.position);
      else { this.mode = paused ? 'paused' : 'stopped'; $('speaker-file-status').textContent = paused ? 'Paused' : 'Ready'; this.render(); }
    }
    async pump() {
      if (this.nativeFile || this.mode !== 'playing' || !this.hasStream || this.chunkPending || this.next >= this.duration || this.next - this.current() > 6) return;
      const id = this.trackId, chunk = this.encode(this.next);
      if (!chunk) return;
      this.chunkPending = true;
      const ok = await this.command({action: 'playSpeakerSound', route: this.route, trackId: id, append: true, pcmBase64: chunk.pcmBase64}, id);
      if (id !== this.trackId) return;
      this.chunkPending = false;
      if (ok) this.next += chunk.duration;
      else this.stop('Playback stopped. Check Diagnostics.');
    }
    event(event) {
      if(event.type==='audioMeters'){
        const file=event.file;
        if(!this.nativeFile||!file||file.trackId!==this.trackId)return;
        if(file.mode==='finished'){
          if(this.previewEnd!=null){const anchor=this.base;this.stop('Comparison finished');this.position=anchor;this.render();return;}
          this.duration=this.base+file.duration;this.stop('Finished');this.position=this.duration;this.render();
          if(this.repeat==='one')this.startAt(0);else if(this.repeat==='all'||this.shuffle)this.step(1);
          return;
        }
        if(!['starting','playing','buffering','paused'].includes(this.mode))return;
        if((file.mode==='paused')!==(this.mode==='paused'))return;
        this.position=Math.min(this.duration,this.base+file.position);this.started=performance.now();
        this.mode=file.mode;$('speaker-file-status').textContent={playing:'Playing',paused:'Paused',starting:'Opening controller audio...',buffering:'Buffering...'}[file.mode]||'Playing';
        this.render();return;
      }
      if (event.trackId != null && event.trackId !== this.trackId) return;
      if (event.source === 'error') {
        const message = event.message || 'Playback failed. Check Diagnostics.';
        if (this.mode !== 'stopped') this.stop(message);
        else $('speaker-file-status').textContent = message;
        return;
      }
      if (event.source === 'file' && this.mode === 'starting') {
        this.mode = 'playing'; this.position = this.base; this.started = performance.now();
        $('speaker-file-status').textContent = 'Playing'; this.render(); this.pump();
      }
      if (event.source === 'transport' && this.hasStream && ['playing','paused'].includes(this.mode)) {
        // Ignore an acknowledgement overtaken by another local transport action.
        if (!!event.paused !== (this.mode === 'paused')) return;
        this.position = Math.min(this.duration, this.base + Math.max(0, event.position || 0));
        this.started = performance.now(); this.render();
      }
    }
    render() {
      const visible=!document.hidden&&!$('haptics').hidden;
      const ticking=['playing','buffering','starting'].includes(this.mode)&&(!this.nativeFile||visible);
      if(this.lastMode!==this.mode){this.lastMode=this.mode;document.dispatchEvent(new Event('controller-player-state'));}
      if(!ticking&&this.tickTimer){clearTimeout(this.tickTimer);this.tickTimer=0;}
      if(ticking&&!this.tickTimer)this.tickTimer=setTimeout(()=>{this.tickTimer=0;this.pump();this.render();},100);
      if(!visible)return;
      const current = this.current(), ready = !!this.decoded && this.connected($('speaker-route').value);
      if (this.loopA != null && this.loopB != null && this.mode === 'playing' && current >= this.loopB && !this.looping) { this.looping = true; this.seek(this.loopA); setTimeout(() => this.looping = false, 600); }
      if (!this.nativeFile && this.mode === 'playing' && current >= this.duration) {
        this.stop('Finished'); this.position = this.duration;
      }
      const button = $('speaker-file-play');
      button.textContent = this.mode === 'loading' ? 'Loading…' : this.mode === 'starting' ? 'Starting…' : ['playing','buffering'].includes(this.mode) ? 'Pause' : this.mode === 'paused' ? 'Resume' : 'Play track';
      button.setAttribute('aria-label', ['playing','buffering'].includes(this.mode) ? 'Pause music' : 'Play music');
      button.disabled = !ready || ['loading','starting'].includes(this.mode);
      window.controllerPlatform?.why?.(button, !this.decoded ? 'Needs a sound: choose an audio file first.' : !ready ? 'Needs a connected controller by USB (the speaker and grips play over USB).' : null);
      $('speaker-file-stop').disabled = !this.decoded || (this.mode === 'stopped' && this.position === 0);
      $('speaker-start').disabled = !this.decoded || this.mode === 'loading';
      $('speaker-start').max = String(this.duration); $('speaker-progress').max = Math.max(1, this.duration);
      if (!this.scrubbing) {
        $('speaker-start').value = String(current); $('speaker-start').style.setProperty('--fill',`${this.duration?current/this.duration*100:0}%`); $('speaker-progress').value = current;
        $('speaker-start-value').textContent = format(current);
        $('speaker-time').textContent = `${format(current)} / ${format(this.duration)}`;
      }
    }
  }
  window.ControllerPlayer = ControllerPlayer;
})();
