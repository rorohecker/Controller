/* Source levels and output inspection. Counters stay in the details panel. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id);
  const defaults=()=>({fileGain:1,cueGain:1,bridgeGain:1,speakerGain:1,ducking:.65,solo:'stereo'});
  const fields=[['fileGain','Music to grips'],['cueGain','Haptic cues'],['bridgeGain','System audio to grips'],['speakerGain','Speaker volume'],['ducking','Lower background during cues']];
  class ControllerAudioMixer {
    constructor(send,connected){
      this.send=send;this.connected=connected;this.settings=defaults();
      try{
        const saved=JSON.parse(localStorage.getItem('controller.audioMixer'));
        if(saved&&fields.every(([key])=>Number.isFinite(saved[key])&&saved[key]>=0&&saved[key]<=1)&&['stereo','left','right'].includes(saved.solo))this.settings={...this.settings,...saved};
      }catch{}
      for(const [key,label] of fields){
        const row=document.createElement('div');row.className='audio-mix-control';
        if(key==='bridgeGain'){row.dataset.feature='systemAudioBridge';row.hidden=!window.controllerPlatform.systemAudioBridge;}
        const text=document.createElement('label');text.htmlFor=`audio-mix-${key}`;text.className='control-label';
        const name=document.createElement('span');name.textContent=label;
        const value=document.createElement('b');value.id=`audio-mix-${key}-value`;
        const input=document.createElement('input');input.id=`audio-mix-${key}`;input.type='range';input.min='0';input.max='100';input.step='1';
        if(key==='ducking')input.title='Reduce music and system audio during haptic cues. 0% leaves background levels unchanged.';
        if(key==='cueGain')input.title='Grip waveform previews and game cues. Adjust trigger resistance on Triggers.';
        input.addEventListener('input',()=>{this.settings[key]=+input.value/100;this.save();this.render();});
        text.append(name,value);row.append(text,input);$('audio-mixer-controls').append(row);
      }
      $('grip-audition').addEventListener('change',()=>{this.settings.solo=$('grip-audition').value;this.save();});
      $('audio-mixer-reset').addEventListener('click',()=>{this.settings=defaults();this.save();this.render();});
      this.render();this.clear();
    }
    render(){
      for(const [key] of fields){$('audio-mix-'+key).value=String(Math.round(this.settings[key]*100));$('audio-mix-'+key+'-value').textContent=`${Math.round(this.settings[key]*100)}%`;}
      $('grip-audition').value=this.settings.solo;
    }
    save(){try{localStorage.setItem('controller.audioMixer',JSON.stringify(this.settings));}catch{}this.apply();}
    apply(){if(this.connected())this.send({action:'setAudioMixer',settings:{...this.settings}});}
    clear(){
      for(const side of ['speaker','left','right'])$('audio-meter-'+side).value=0;
      $('audio-output-state').textContent='USB audio';$('audio-endpoint').textContent='No audio endpoint open';
      $('audio-route-summary').textContent='Play a track, a haptic cue, or the audio bridge to inspect its output.';
      $('audio-source-summary').textContent='No active sources';$('audio-limiter-status').textContent='Peak control ready';
      $('audio-buffer-status').textContent='File queue: 0 ms';$('audio-buffer-counts').textContent='No buffer gaps recorded';
    }
    event(status){
      const peaks=Array.isArray(status.peaks)?status.peaks:[];
      ['speaker','left','right'].forEach((side,i)=>$('audio-meter-'+side).value=Math.max(0,Math.min(1,Number(peaks[i])||0)));
      $('audio-output-state').textContent=status.active?'USB ready':'Not playing';
      $('audio-endpoint').textContent=status.endpoint||'No audio endpoint open';
      const file=status.file,active=file&&['playing','starting','buffering'].includes(file.mode);
      const sources=[];if(active)sources.push('Track');if(status.bridge)sources.push('System audio');if(status.cueActive)sources.push('Haptic cue');
      $('audio-source-summary').textContent=sources.length?`Sources: ${sources.join(' + ')}`:file?.mode==='paused'?'Track paused':'No active sources';
      const routes=[];if(active&&file.speaker)routes.push('speaker');if((active&&file.grips)||status.bridge||status.cueActive)routes.push(this.settings.solo==='stereo'?'both grips':this.settings.solo+' grip');
      $('audio-route-summary').textContent=routes.length?`Output: ${routes.join(' + ')}`:status.active?'Ready for playback':'Connect by USB and start playback';
      $('audio-limiter-status').textContent=(status.reduction||0)>.15?'Peak control active':'Levels within range';
      $('audio-buffer-status').textContent=`File queue: ${Math.round(file?.queueMs||0)} ms / system audio: ${Math.round(status.bridgeQueueMs||0)} ms`;
      $('audio-buffer-counts').textContent=`File buffer gaps: ${status.fileUnderruns||0} / capture gaps: ${status.bridgeUnderruns||0} / old capture frames dropped: ${status.bridgeDropped||0} / device glitches: ${status.glitches||0}`;
    }
  }
  window.ControllerAudioMixer=ControllerAudioMixer;
})();
