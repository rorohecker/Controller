/* File transport. Commands are ordered; every track has an identity so stale
   chunks and acknowledgements cannot revive stopped playback. */
(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const format = n => `${Math.floor(n / 60)}:${String(Math.floor(n % 60)).padStart(2, '0')}`;
  class ControllerPlayer {
    constructor(send, connected, invoke = null) {
      this.invoke = invoke; this.nativeFile = null;
      this.send = send; this.connected = connected; this.trackId = Date.now() * 1000;
      this.mode = 'stopped'; this.position = 0; this.base = 0; this.next = 0;
      this.duration = 0; this.decoded = null; this.chain = Promise.resolve();
      this.scrubbing = false; this.chunkPending = false; this.hasStream = false;
      $('speaker-file-play').addEventListener('click', () => this.toggle());
      $('speaker-file-stop').addEventListener('click', () => this.stop());
      $('speaker-file').addEventListener('change', e => this.load(e.target.files[0]));
      $('speaker-route').addEventListener('change', () => this.stop());
      $('speaker-content-kind').addEventListener('change', () => {
        if(!this.nativeFile&&!['music','haptic'].includes($('speaker-content-kind').value)){$('speaker-content-kind').value='music';this.stop('Choose a native audio file to use conversion modes.');return;}
        $('speaker-content-note').textContent={music:'Convert bass and higher notes for the grips.',bass:'Focus grip feedback on low frequencies.',impact:'Emphasize onsets; steady sounds fade.',texture:'Turn higher notes into a tactile carrier.',haptic:'Keep the stereo haptic waveform.'}[$('speaker-content-kind').value];
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
      this.render();
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
      return this.connected()
        ? this.command({action: 'stopSpeakerSound', trackId: this.trackId}, this.trackId, true)
        : Promise.resolve(true);
    }
    async load(file) {
      const pendingStop = this.stop('No sound selected.');
      const loadId = this.trackId;
      await pendingStop;
      if (loadId !== this.trackId) return;
      this.decoded = null; this.nativeFile = null; this.duration = 0; $('speaker-file-format').textContent=''; this.render();
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
      } catch (error) {
        if (id === this.trackId) { this.mode = 'stopped'; $('speaker-file-status').textContent = `Could not read sound: ${error.message}`; }
      } finally { if (context) await context.close(); this.render(); }
    }
    async chooseNative() {
      if(!this.invoke)return;
      const button=$('speaker-native-file');button.disabled=true;
      try {
        const info=await this.invoke('choose_audio_file');if(!info)return;
        await this.stop();
        this.nativeFile=info;this.decoded={native:true};this.duration=info.duration;this.mode='stopped';
        $('speaker-track-name').textContent=info.name;$('speaker-track-name').title=info.name;
        $('speaker-file-format').textContent=`${info.channels===1?'Mono':'Stereo'} / ${(info.sampleRate/1000).toLocaleString(undefined,{maximumFractionDigits:3})} kHz`;
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
      if(!this.nativeFile||!this.connected()||!['music','bass','impact','texture'].includes(kind))return;
      const pendingStop=this.stop(),stopped=this.trackId;await pendingStop;
      if(stopped!==this.trackId)return;
      position=Math.max(0,Math.min(this.duration,position));
      if(position>=this.duration){$('speaker-file-status').textContent='Capture a position before the end of the track.';return;}
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
      if (!this.decoded || !this.connected()) return;
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
          this.duration=this.base+file.duration;this.stop('Finished');this.position=this.duration;this.render();return;
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
      const current = this.current(), ready = !!this.decoded && this.connected();
      if (!this.nativeFile && this.mode === 'playing' && current >= this.duration) {
        this.stop('Finished'); this.position = this.duration;
      }
      const button = $('speaker-file-play');
      button.textContent = this.mode === 'loading' ? 'Loading…' : this.mode === 'starting' ? 'Starting…' : ['playing','buffering'].includes(this.mode) ? 'Pause' : this.mode === 'paused' ? 'Resume' : 'Play';
      button.setAttribute('aria-label', ['playing','buffering'].includes(this.mode) ? 'Pause music' : 'Play music');
      button.disabled = !ready || ['loading','starting'].includes(this.mode);
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
