/* Source levels and output inspection. Counters stay in the details panel. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id);
  const defaults=()=>({fileGain:1,cueGain:1,gameGain:1,bridgeGain:1,speakerGain:1,ducking:.65,solo:'stereo'});
  const fields=[['fileGain','Music to grips'],['cueGain','Haptic cues'],['gameGain','Xbox game rumble'],['bridgeGain','PC sound to grips'],['speakerGain','Speaker volume'],['ducking','Lower background during cues']];
  class ControllerAudioMixer {
    constructor(send,connected){
      this.send=send;this.connected=connected;this.settings=defaults();window.controllerMixer=this;
      try{
        const saved=JSON.parse(localStorage.getItem('controller.audioMixer'));
        if(saved&&fields.every(([key])=>(saved[key]===undefined&&key==='gameGain'||Number.isFinite(saved[key])&&saved[key]>=0&&saved[key]<=1))&&['stereo','left','right'].includes(saved.solo))this.settings={...this.settings,...saved};
      }catch{}
      for(const [key,label] of fields){
        const row=document.createElement('div');row.className='audio-mix-control';
        if(key==='gameGain'){row.dataset.feature='xinput';row.hidden=!window.controllerPlatform.xinput;}
        if(key==='bridgeGain'){row.dataset.feature='systemAudioBridge';row.hidden=!window.controllerPlatform.systemAudioBridge;}
        const text=document.createElement('label');text.htmlFor=`audio-mix-${key}`;text.className='control-label';
        const name=document.createElement('span');name.textContent=label;
        const value=document.createElement('b');value.id=`audio-mix-${key}-value`;
        const input=document.createElement('input');input.id=`audio-mix-${key}`;input.type='range';input.min='0';input.max='100';input.step='1';
        if(key==='ducking')input.title='Reduce music and system audio during haptic cues. 0% leaves background levels unchanged.';
        if(key==='cueGain')input.title='Grip waveform previews and game cues. Adjust trigger resistance on Triggers.';
        input.addEventListener('input',()=>{this.settings[key]=+input.value/100;this.render();this.schedule();});
        input.addEventListener('change',()=>{this.flush();});
        text.append(name,value);row.append(text,input);$('audio-mixer-controls').append(row);
      }
      $('grip-audition').addEventListener('change',()=>{this.settings.solo=$('grip-audition').value;this.save();});
      $('audio-mixer-reset').addEventListener('click',async()=>{
        const before={...this.settings};
        const ok=await window.controllerPlatform.confirm('Reset every mixer level to its default?',{title:'Reset levels',confirmLabel:'Reset levels'});
        if(!ok)return;
        this.settings=defaults();this.save();this.render();
        window.controllerPlatform.undoToast('Mixer levels reset.',()=>{this.settings=before;this.save();this.render();});
      });
      this.tagPanels();
      this.buildMeterExtras();this.buildSnapshots();this.buildBridge();this.buildDiagnostics();
      document.addEventListener('controller-view',()=>{if(this.latest)this.event(this.latest);});
      document.addEventListener('visibilitychange',()=>{if(this.latest)this.event(this.latest);});
      this.render();this.clear();
    }
    tagPanels(){
      const P=window.controllerPlatform,set=(sel,text)=>{const el=document.querySelector(sel);if(el)el.textContent=text;};
      const mixer=document.querySelector('.audio-mixer-panel'),bridge=document.querySelector('.bridge-panel'),alerts=document.querySelector('.alert-panel');
      P.section(mixer,'haptics.mixer','advanced','Volume balance','Mix music, cues, game rumble and PC sound',{keywords:'volume balance rumble xbox mixer gains ducking solo levels audio music cues snapshot reset'});
      P.section(document.querySelector('details.audio-details'),'haptics.audio-details','advanced','Audio details','Technical output information',{keywords:'audio details endpoint buffer underruns output technical'});
      P.section(bridge,'haptics.bridge','basic','Make PC sound shake the grips','Feel game or system audio in the grips',{open:true,keywords:'system audio bridge pc sound game loopback grips feel intensity'});
      P.section(alerts,'haptics.alerts','basic','Reminders','Grip cues for low battery and timers',{open:true,keywords:'reminders alerts low battery timer find controller ping cue'});
      set('.audio-mixer-panel .panel-cap span','Volume balance');set('.bridge-panel .panel-cap span','Make PC sound shake the grips');set('.alert-panel .panel-cap span','Reminders');
      set('.alert-panel h2','Low battery and timers');set('#alert-ping','Find my controller');
    }
    render(){
      for(const [key] of fields){$('audio-mix-'+key).value=String(Math.round(this.settings[key]*100));$('audio-mix-'+key+'-value').textContent=`${Math.round(this.settings[key]*100)}%`;}
      $('grip-audition').value=this.settings.solo;
    }
    schedule(){if(this.frame)return;this.frame=requestAnimationFrame(()=>{this.frame=0;this.save();});}
    flush(){if(this.frame){cancelAnimationFrame(this.frame);this.frame=0;}this.save();}
    save(){try{localStorage.setItem('controller.audioMixer',JSON.stringify(this.settings));}catch{}this.apply();}
    static snapshots(){try{const v=JSON.parse(localStorage.getItem('controller.mixerSnapshots'));return v&&typeof v==='object'?v:{};}catch{return {};}}
    buildMeterExtras(){
      const host=document.querySelector('.audio-output-meters');if(!host)return;
      this.peakHold={speaker:0,left:0,right:0};this.peakStamp={speaker:0,left:0,right:0};
      const ticks=document.createElement('div');ticks.className='audio-db-ticks';ticks.setAttribute('aria-hidden','true');ticks.textContent='Scale: -inf to 0 dB (full scale)';ticks.title='Marks: -24, -12, -6 and 0 dB';
      const readout=document.createElement('div');readout.id='audio-peak-readout';readout.setAttribute('role','status');readout.className='audio-peak-readout';readout.textContent='Peak hold: -';
      const reduction=document.createElement('label');reduction.append(document.createTextNode('Limiter reduction'));
      const meter=document.createElement('meter');meter.id='audio-limiter-meter';meter.min=0;meter.max=1;meter.value=0;meter.setAttribute('aria-label','Limiter gain reduction');
      reduction.append(meter);
      const clip=document.createElement('button');clip.type='button';clip.id='audio-clip-indicator';clip.className='audio-clip-indicator';clip.hidden=true;clip.textContent='CLIP (click to clear)';clip.title='Output reached full scale.';
      clip.addEventListener('click',()=>{clip.hidden=true;});
      host.append(ticks,readout,reduction,clip);
      const style=document.createElement('style');style.textContent='.audio-db-ticks,.audio-peak-readout{font:.75rem var(--mono,monospace);color:var(--parchment-dim,#d5e3df)}.audio-clip-indicator{justify-self:start;padding:1px 8px;border-radius:4px;border:0;background:var(--danger,#c45c3e);color:#fff;font:.75rem var(--mono,monospace);cursor:pointer}.audio-clip-indicator[hidden]{display:none}.audio-db-ticks{white-space:normal;overflow-wrap:anywhere;max-width:100%}.audio-snapshots{display:flex;gap:6px;flex-wrap:wrap;align-items:center;margin-top:8px}';
      document.head.append(style);
    }
    buildSnapshots(){
      const host=$('audio-mixer-reset')?.parentElement;if(!host)return;
      const row=document.createElement('div');row.className='audio-snapshots';window.controllerPlatform.section(row,'haptics.mixer-snapshots','advanced','Volume snapshots','Save and recall volume presets',{keywords:'mixer snapshot preset movie gaming music save recall levels'});
      const label=document.createElement('label');label.append(document.createTextNode('Mixer snapshot '));
      const select=document.createElement('select');select.id='audio-snapshot';select.setAttribute('aria-label','Mixer snapshot');
      for(const n of ['Movie','Gaming','Music'])select.add(new Option(n,n));
      label.append(select);
      const save=document.createElement('button');save.type='button';save.className='quiet-action';save.id='audio-snapshot-save';save.textContent='Save levels to snapshot';
      const load=document.createElement('button');load.type='button';load.className='quiet-action';load.id='audio-snapshot-load';load.textContent='Apply snapshot';
      const status=document.createElement('span');status.className='ui-note';status.setAttribute('role','status');
      save.addEventListener('click',()=>{const all=ControllerAudioMixer.snapshots();all[select.value]={...this.settings};try{localStorage.setItem('controller.mixerSnapshots',JSON.stringify(all));status.textContent=`Saved ${select.value}.`;}catch{status.textContent='Could not save snapshot.';}});
      load.addEventListener('click',()=>{const s=ControllerAudioMixer.snapshots()[select.value];if(!s){status.textContent=`${select.value} is empty. Save levels first.`;return;}this.applySnapshot(s);status.textContent=this.connected()?`Applied ${select.value}.`:`Applied ${select.value}. It is sent when a controller connects.`;});
      row.append(label,save,load,status);host.append(row);
    }
    bridgeStore(){try{const v=JSON.parse(localStorage.getItem('controller.bridgeTuning'));return v&&typeof v==='object'?v:{};}catch{return {};}}
    bridgeSave(patch){this.bridgeCfg={...this.bridgeCfg,...patch};try{localStorage.setItem('controller.bridgeTuning',JSON.stringify(this.bridgeCfg));}catch{}}
    buildBridge(){
      const controls=document.querySelector('.bridge-controls');if(!controls)return;
      const defaultsCfg={latency:'normal',cutoff:180,bass:.7,transient:.2,gate:0,mode:'standard',bands:[1,1,1],onset:0,pan:0};
      const cfg=this.bridgeCfg={...defaultsCfg,...this.bridgeStore()};
      const lim=(v,lo,hi,d)=>Number.isFinite(+v)&&v!==null&&v!==''?Math.min(hi,Math.max(lo,+v)):d;
      cfg.cutoff=lim(cfg.cutoff,60,400,180);cfg.bass=lim(cfg.bass,0,3,.7);cfg.transient=lim(cfg.transient,0,3,.2);cfg.gate=lim(cfg.gate,0,.2,0);cfg.onset=lim(cfg.onset,0,1,0);cfg.pan=lim(cfg.pan,0,1,0);
      if(!['low','normal','safe'].includes(cfg.latency))cfg.latency='normal';
      if(!['standard','threeBand'].includes(cfg.mode))cfg.mode='standard';
      if(!Array.isArray(cfg.bands)||cfg.bands.length!==3)cfg.bands=[1,1,1];
      cfg.bands=cfg.bands.map(v=>lim(v,0,2,1));
      const preset=$('bridge-preset');
      if(preset&&![...preset.options].some(o=>o.value==='custom'))preset.add(new Option('Custom','custom'));
      const mk=(tag,props={},kids=[])=>{const e=document.createElement(tag);for(const [k,v] of Object.entries(props)){if(k==='text')e.textContent=v;else if(k==='class')e.className=v;else e[k]=v;}e.append(...kids);return e;};
      const lat=mk('select',{id:'bridge-latency'});
      for(const [v,t] of [['low','Low · 10 ms'],['normal','Normal · 20 ms'],['safe','Safe · 40 ms']])lat.add(new Option(t,v));
      lat.value=cfg.latency;
      const latLabel=mk('label',{htmlFor:'bridge-latency',text:'Latency'});
      lat.addEventListener('change',()=>{this.bridgeSave({latency:lat.value});this.sendBridge('latency');});
      const slider=(id,label,min,max,step,key,fmt,kind)=>{
        const out=mk('b',{id:id+'-value'});const input=mk('input',{id,type:'range',min:String(min),max:String(max),step:String(step)});
        input.value=String(cfg[key]);out.textContent=fmt(+input.value);
        input.addEventListener('input',()=>{this.bridgeSave({[key]:+input.value});out.textContent=fmt(+input.value);});
        input.addEventListener('change',()=>this.sendBridge(kind));
        return [mk('label',{htmlFor:id,class:'control-label'},[mk('span',{text:label}),out]),input];
      };
      const custom=mk('div',{id:'bridge-custom-controls',class:'bridge-extra-group'},[
        ...slider('bridge-custom-cutoff','Bass cutoff',60,400,1,'cutoff',v=>`${v} Hz`,'custom'),
        ...slider('bridge-custom-bass','Bass weight',0,3,.05,'bass',v=>v.toFixed(2),'custom'),
        ...slider('bridge-custom-transient','Transient weight',0,3,.05,'transient',v=>v.toFixed(2),'custom'),
        ...slider('bridge-custom-gate','Noise gate',0,.2,.005,'gate',v=>v.toFixed(3),'custom')]);
      const mode=mk('select',{id:'bridge-mode'});mode.add(new Option('Standard','standard'));mode.add(new Option('Three-band','threeBand'));mode.value=cfg.mode;
      mode.addEventListener('change',()=>{this.bridgeSave({mode:mode.value});this.renderBridgeExtras();this.sendBridge('options');});
      const bandBox=mk('div',{id:'bridge-band-controls',class:'bridge-extra-group'});
      ['Low band','Mid band','High band'].forEach((name,i)=>{
        const id=`bridge-band-${i}`,out=mk('b',{id:id+'-value'});const input=mk('input',{id,type:'range',min:'0',max:'2',step:'0.05'});
        input.value=String(cfg.bands[i]);out.textContent=`${(+input.value).toFixed(2)}x`;
        input.addEventListener('input',()=>{const b=[...this.bridgeCfg.bands];b[i]=+input.value;this.bridgeSave({bands:b});out.textContent=`${b[i].toFixed(2)}x`;});
        input.addEventListener('change',()=>this.sendBridge('options'));
        bandBox.append(mk('label',{htmlFor:id,class:'control-label'},[mk('span',{text:name}),out]),input);
      });
      const extras=mk('div',{id:'bridge-options-controls',class:'bridge-extra-group'},[
        ...slider('bridge-onset','Onset pulses',0,1,.05,'onset',v=>`${Math.round(v*100)}%`,'options'),
        ...slider('bridge-pan','Pan emphasis',0,1,.05,'pan',v=>`${Math.round(v*100)}%`,'options')]);
      const modeLabel=mk('label',{htmlFor:'bridge-mode',text:'Analysis'});
      const adv=mk('details',{id:'bridge-advanced',class:'audio-details'},[mk('summary',{text:'Fine-tune'}),latLabel,lat,custom,modeLabel,mode,bandBox,extras]);
      controls.after(adv);window.controllerPlatform.section(adv,'haptics.bridge-tuning','advanced','Fine-tune','Latency, bands, onset and pan',{keywords:'bridge tuning latency bass cutoff bands onset pan noise gate analysis three-band'});
      this.renderBridgeExtras=()=>{custom.hidden=preset?.value!=='custom';bandBox.hidden=mode.value!=='threeBand';};
      preset?.addEventListener('change',()=>{this.renderBridgeExtras();if(preset.value==='custom')this.sendBridge('custom');});
      this.renderBridgeExtras();
      const style=document.createElement('style');
      style.textContent='.bridge-extra-group{display:grid;gap:6px;margin:6px 0}.bridge-extra-group[hidden]{display:none}#bridge-advanced select{max-width:100%}.audio-alert-row{display:flex;gap:8px;flex-wrap:wrap;align-items:center}.audio-alert-row[hidden],.audio-thermal[hidden]{display:none}.audio-thermal{display:inline-block;padding:1px 8px;border-radius:4px;background:var(--warn,#a8742a);color:#fff;font:.75rem var(--mono,monospace)}.audio-diagnostics{display:grid;gap:4px;overflow-wrap:anywhere}.audio-diagnostics meter{width:100%}';
      document.head.append(style);
    }
    sendBridge(kind){
      if(!this.connected())return;
      const c=this.bridgeCfg;
      if(kind==='latency')this.send({action:'setBridgeLatency',value:c.latency});
      else if(kind==='custom')this.send({action:'setBridgeCustom',cutoff:c.cutoff,bass:c.bass,transient:c.transient,gate:c.gate});
      else this.send({action:'setBridgeOptions',mode:c.mode,bandGains:[...c.bands],onset:c.onset,panEmphasis:c.pan});
    }
    syncBridge(){
      if(!this.connected()||!this.bridgeCfg)return;
      if(Object.keys(this.bridgeStore()).length===0)return;
      for(const kind of ['latency','custom','options'])this.sendBridge(kind);
    }
    buildDiagnostics(){
      const details=document.querySelector('.audio-details');if(!details)return;
      const mk=(tag,id,text)=>{const e=document.createElement(tag);if(id)e.id=id;if(text)e.textContent=text;return e;};
      const holder=mk('p','audio-endpoint-holder');holder.hidden=true;
      const dev=mk('p','audio-capture-device');dev.hidden=true;
      const cst=mk('p','audio-capture-status');cst.hidden=true;
      const inLabel=document.createElement('label');inLabel.id='audio-capture-input-label';inLabel.append(document.createTextNode('Capture input (before conversion)'));
      const inMeter=document.createElement('meter');inMeter.id='audio-capture-input-meter';inMeter.min=0;inMeter.max=1;inMeter.value=0;inMeter.setAttribute('aria-label','Capture input level before haptic conversion');inLabel.append(inMeter);inLabel.hidden=true;
      const inText=mk('p','audio-capture-input');inText.hidden=true;
      const bt=mk('p','audio-bluetooth-output');bt.hidden=true;
      const under=mk('p','audio-output-underruns');under.hidden=true;
      const box=document.createElement('div');box.className='audio-diagnostics';box.append(holder,dev,cst,inLabel,inText,bt,under);
      details.append(box);
      const alertRow=document.createElement('div');alertRow.id='audio-alert-row';alertRow.className='audio-alert-row';alertRow.hidden=true;
      const thermal=mk('span','audio-thermal-badge','Thermal limit active');thermal.className='audio-thermal';thermal.hidden=true;thermal.title='Grip output is being reduced to protect the controller.';
      const fail=mk('span','audio-output-failure');fail.setAttribute('role','status');
      const retry=mk('button','audio-output-retry','Retry now');retry.type='button';retry.className='quiet-action';retry.hidden=true;
      retry.addEventListener('click',()=>this.retryOutput());
      alertRow.append(thermal,fail,retry);
      $('audio-limiter-status').after(alertRow);
      this.diag={holder,dev,cst,inLabel,inMeter,inText,bt,under,alertRow,thermal,fail,retry};
    }
    retryOutput(){
      if(!this.connected())return;
      // The engine re-arms a failed output when a priming command arrives: re-send the bridge if it was on, else a speaker chirp.
      if(this.latest?.bridge)this.send({action:'setAudioBridge',enabled:true,gain:(+$('bridge-gain').value||50)/100,preset:$('bridge-preset').value,source:window.integrationStudio?.commandSource?.()||{kind:'system'}});
      else this.send({action:'playSpeakerChirp'});
    }
    diagnostics(status){
      const d=this.diag;if(!d)return;
      const names={bridge:'the audio bridge',timeline:'a timeline',gameRumble:'game rumble',cue:'a haptic cue',speakerChirp:'a speaker test',file:'a track',track:'a track'};
      const holding=Array.isArray(status.keepOpenBy)&&status.keepOpenBy.length;
      d.holder.hidden=!status.endpointHolder;
      if(status.endpointHolder)d.holder.textContent=holding?`CONTROLLER is holding the controller audio endpoint because: ${status.keepOpenBy.map(k=>names[k]||k).join(', ')}.`:'CONTROLLER is holding the controller audio endpoint (no active source; it closes shortly).';
      const cd=status.captureDevice;
      d.dev.hidden=!cd;if(cd)d.dev.textContent=`Capture device: ${cd.name} (${Math.round((cd.sampleRate||0)/100)/10} kHz)`;
      d.cst.hidden=!status.captureStatus;if(status.captureStatus)d.cst.textContent=`Capture: ${status.captureStatus}`;
      const ci=status.captureInput,showIn=!!ci&&(status.bridge||status.captureState==='running');
      d.inLabel.hidden=!showIn;d.inText.hidden=!showIn;
      if(showIn){const peak=Math.max(0,Math.min(1,Number(ci.peak)||0)),rms=Math.max(0,Math.min(1,Number(ci.rms)||0));d.inMeter.value=peak;d.inText.textContent=`Capture input: peak ${peak.toFixed(2)} / rms ${rms.toFixed(2)}`;}
      const b=status.bluetoothOutput;d.bt.hidden=!b;if(b)d.bt.textContent=`Bluetooth grip audio: ${b.dropped||0} dropped / ${b.underruns||0} underruns`;
      const u=Number(status.outputUnderruns)||0;d.under.hidden=!u;if(u)d.under.textContent=`Output underruns: ${u}`;
      d.thermal.hidden=!status.thermalLimitActive;
      const secs=Number(status.outputRetrySeconds)||0,failure=status.outputFailure;
      d.fail.textContent=failure?`Audio output failed: ${failure}${secs>0?` Retrying in ${secs} s.`:''}`:'';
      d.retry.hidden=!failure;d.retry.disabled=!this.connected();
      d.alertRow.hidden=!status.thermalLimitActive&&!failure;
      const cs=status.captureState;
      if(cs==='reconnecting'||cs==='reattaching'){d.cst.hidden=false;d.cst.textContent=`Capture: ${cs==='reattaching'?'waiting for the captured app to restart (up to 30 s).':'capture device changed; reconnecting.'}`;}
    }
    applySnapshot(s){
      if(!s||typeof s!=='object')return false;
      const next={...this.settings};
      for(const [key] of fields)if(Number.isFinite(s[key])&&s[key]>=0&&s[key]<=1)next[key]=s[key];
      if(['stereo','left','right'].includes(s.solo))next.solo=s.solo;
      this.settings=next;this.save();this.render();return true;
    }
    // The bridge tuning is re-sent only on connect or device change (syncBridge=true); slider ticks resend just the mixer.
    apply(syncBridge=false){if(this.connected()){this.send({action:'setAudioMixer',settings:{...this.settings}});if(syncBridge)this.syncBridge();}}
    clear(){
      for(const side of ['speaker','left','right'])$('audio-meter-'+side).value=0;
      $('audio-output-state').textContent='USB audio';$('audio-endpoint').textContent='No audio endpoint open';
      $('audio-route-summary').textContent='Play a track, a haptic cue, or the audio bridge to inspect its output.';
      $('audio-source-summary').textContent='No active sources';$('audio-limiter-status').textContent='Peak control ready';
      $('audio-buffer-status').textContent='File queue: 0 ms';$('audio-buffer-counts').textContent='No buffer gaps recorded';
      if(this.diag){for(const e of [this.diag.holder,this.diag.dev,this.diag.cst,this.diag.inLabel,this.diag.inText,this.diag.bt,this.diag.under,this.diag.alertRow])e.hidden=true;}
    }
    event(status){
      this.latest=status;if(document.hidden||$('haptics').hidden)return;
      this.diagnostics(status);
      const peaks=Array.isArray(status.peaks)?status.peaks:[];
      const now=performance.now(),db=v=>v>0.0005?`${(20*Math.log10(v)).toFixed(1)} dB`:'-inf';
      const held=[];
      ['speaker','left','right'].forEach((side,i)=>{
        const v=Math.max(0,Math.min(1,Number(peaks[i])||0));$('audio-meter-'+side).value=v;
        if(this.peakHold&&(v>=this.peakHold[side]||now-this.peakStamp[side]>2000)){this.peakHold[side]=v;this.peakStamp[side]=now;}
        if(v>=0.985&&$('audio-clip-indicator'))$('audio-clip-indicator').hidden=false;
        held.push(`${side} ${db(v)} (hold ${db(this.peakHold?.[side]||0)})`);
      });
      if($('audio-peak-readout'))$('audio-peak-readout').textContent='Peak: '+held.join(' / ');
      if($('audio-limiter-meter'))$('audio-limiter-meter').value=Math.max(0,Math.min(1,Number(status.reduction)||0));
      $('audio-output-state').textContent=status.outputStage==='starting'?'Opening audio':status.active?'USB ready':'Not playing';
      $('audio-endpoint').textContent=status.endpoint||'No audio endpoint open';
      const file=status.file,active=file&&['playing','starting','buffering'].includes(file.mode);
      const sources=[];if(active)sources.push('Track');if(status.bridge)sources.push(status.captureSource?.kind==='process'?'Selected app':status.captureSource?.kind==='nativeEndpoint'?'Native audio relay':'System audio');if(status.cueActive)sources.push('Haptic cue');if(status.timelineActive)sources.push('Timeline');if(status.gameActive)sources.push('Xbox rumble');
      $('audio-source-summary').textContent=sources.length?`Sources: ${sources.join(' + ')}`:file?.mode==='paused'?'Track paused':'No active sources';
      const routes=[];if(active&&file.speaker)routes.push('speaker');if((active&&file.grips)||status.bridge||status.cueActive||status.timelineActive||status.gameActive)routes.push(this.settings.solo==='stereo'?'both grips':this.settings.solo+' grip');
      $('audio-route-summary').textContent=routes.length?`Output: ${routes.join(' + ')}`:status.active?'Ready for playback':'Connect by USB and start playback';
      $('audio-limiter-status').textContent=(status.reduction||0)>.15?'Peak control active':'Levels within range';
      $('audio-buffer-status').textContent=`File queue: ${Math.round(file?.queueMs||0)} ms / system audio: ${Math.round(status.bridgeQueueMs||0)} ms`;
      $('audio-buffer-counts').textContent=`File buffer gaps: ${status.fileUnderruns||0} / capture gaps: ${status.bridgeUnderruns||0} / held frames: ${status.bridgeHoldFrames||0} / old capture frames dropped: ${status.bridgeDropped||0} / device glitches: ${status.glitches||0}`;
    }
  }
  window.ControllerAudioMixer=ControllerAudioMixer;
})();
