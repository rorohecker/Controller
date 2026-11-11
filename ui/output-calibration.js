/* Native saved calibration and the values presented to games. */
(() => {
  'use strict';
  const copy=v=>structuredClone(v),$=id=>document.getElementById(id);
  class OutputCalibration {
    constructor(a){
      this.a=a;this.owner='';this.record=null;this.connected=false;this.busy=false;this.sample=null;this.error='';this.epoch=0;
      const card=document.createElement('article');card.className='work-panel framed output-calibration';card.id='output-calibration';window.controllerPlatform?.section?.(card,'calibration.output','advanced','Game input correction','Per-controller output correction, reload and restore',{keywords:'calibration output correction game input xbox virtual reload restore noise rest filter circularity'});
      card.innerHTML=`<div class="panel-cap"><span>Game input correction</span><span id="output-route">Virtual Xbox · Windows</span></div><div class="controls-inner"><p id="output-cal-status" role="status">Connect a controller.</p><p>Saved ranges and rest noise are applied before each game's stick curves. Physical controller reports stay unchanged.</p><div class="output-comparison">${['Left stick','Right stick'].map((name,i)=>`<div><strong>${name}</strong><svg viewBox="0 0 120 120" role="img" aria-label="${name}: raw and game output"><circle cx="60" cy="60" r="48" fill="none" stroke="currentColor" opacity=".4"/><path d="M12 60h96M60 12v96" stroke="currentColor" opacity=".2"/><circle id="output-raw-${i}" cx="60" cy="60" r="5" class="raw-point"/><circle id="output-game-${i}" cx="60" cy="60" r="4" class="game-point"/></svg><small id="output-noise-${i}">Rest filter: none</small></div>`).join('')}<div class="output-trigger-comparison"><strong>Triggers · raw → game</strong><div id="output-trigger-0">L2: 0% → 0%</div><div id="output-trigger-1">R2: 0% → 0%</div><small>Pink: raw · mint: game output</small><small id="output-send-status">Calculated values; bridge is off.</small></div></div><div class="calibration-actions"><label><input id="output-cal-enabled" type="checkbox" checked> Use saved correction</label><button id="output-cal-restore" class="quiet-action" type="button">Restore previous</button></div><p id="output-cal-error" role="status"></p><small id="output-cal-scope">Games must use the virtual Xbox controller. HidHide can prevent duplicate physical input. Closing the app removes virtual output.</small></div>`;
      const page=document.querySelector('#calibration');page?.append(card);
      $('output-cal-enabled').addEventListener('change',async()=>{
        if(!this.record)return;
        const settings=copy(this.record.settings);settings.enabled=$('output-cal-enabled').checked;
        try{await this.persist(settings,'save');}catch(error){this.fail(error);}this.render();
      });
      const reload=document.createElement('button');reload.id='output-cal-reload';reload.type='button';reload.className='quiet-action';reload.textContent='Reload saved';$('output-cal-restore').before(reload);
      reload.addEventListener('click',async()=>{if(!await this.a.send({action:'reloadOutputCalibration',controllerId:this.owner}))this.fail('Could not reload calibration. Reconnect the controller.');});
      $('output-cal-restore').addEventListener('click',async()=>{
        if(!await window.controllerPlatform.confirm('Restore the previously saved calibration? The current saved ranges and rest noise are replaced.',{title:'Restore previous calibration',confirmLabel:'Restore previous'}))return;
        try{await this.persist(this.record?.settings||this.defaults(),'restore');}catch(error){this.fail(error);}
      });
      this.buildExtras();
      document.addEventListener('controller-view',()=>this.render());
      document.addEventListener('visibilitychange',()=>{if(!document.hidden)this.render();});this.render();
    }
    buildExtras(){
      const NS='http://www.w3.org/2000/svg';this.trails=[[],[]];
      document.querySelectorAll('#output-calibration .output-comparison svg').forEach((svg,i)=>{
        const ring=document.createElementNS(NS,'circle');ring.id='output-dz-'+i;ring.setAttribute('cx',60);ring.setAttribute('cy',60);ring.setAttribute('r',0);ring.setAttribute('fill','none');ring.setAttribute('stroke-dasharray','3 3');ring.style.stroke='var(--gold)';
        const trail=document.createElementNS(NS,'polyline');trail.id='output-trail-'+i;trail.setAttribute('fill','none');trail.style.stroke='var(--teal)';trail.setAttribute('opacity','.6');
        const circ=document.createElementNS(NS,'polygon');circ.id='output-circ-'+i;circ.setAttribute('fill','none');circ.style.stroke='var(--teal-glow)';circ.setAttribute('opacity','.7');
        svg.insertBefore(circ,svg.firstChild);svg.insertBefore(ring,svg.firstChild);svg.insertBefore(trail,svg.firstChild);
      });
      const host=document.querySelector('#output-calibration .controls-inner'),box=document.createElement('div');box.className='output-circularity';
      box.innerHTML='<button id="output-circ-start" type="button" class="quiet-action">Stick circularity test</button> <button id="output-circ-stop" type="button" class="quiet-action" hidden>Finish test</button><p id="output-circ-result" role="status">Start, then roll both sticks slowly around their full edge. Finish to see how round the range is.</p>';
      host.append(box);
      this.circ=null;
      $('output-circ-start').addEventListener('click',()=>{if(!this.connected){$('output-circ-result').textContent='Connect a controller to run the circularity test.';return;}this.circ=[0,1].map(()=>Array(32).fill(0));$('output-circ-start').hidden=true;$('output-circ-stop').hidden=false;$('output-circ-result').textContent='Recording... roll both sticks around the edge, then press Finish test.';});
      $('output-circ-stop').addEventListener('click',()=>this.finishCirc());
    }
    sampleCirc(raw){
      if(!this.circ)return;
      for(let i=0;i<2;i++){const x=raw[i*2]||0,y=raw[i*2+1]||0,m=Math.hypot(x,y);if(m<.3)continue;const b=Math.floor(((Math.atan2(y,x)+Math.PI)/(2*Math.PI))*32)%32;if(m>this.circ[i][b])this.circ[i][b]=m;}
    }
    finishCirc(){
      const data=this.circ;this.circ=null;$('output-circ-start').hidden=false;$('output-circ-stop').hidden=true;if(!data)return;
      const lines=[];
      data.forEach((buckets,i)=>{
        const filled=buckets.filter(v=>v>0);const name=i?'Right':'Left';
        const poly=$('output-circ-'+i);poly.setAttribute('points',buckets.map((v,b)=>{const a=(b+.5)/32*2*Math.PI-Math.PI;return `${(60+48*Math.min(1,v)*Math.cos(a)).toFixed(1)},${(60+48*Math.min(1,v)*Math.sin(a)).toFixed(1)}`;}).join(' '));
        if(filled.length<24){lines.push(`${name}: only ${filled.length}/32 directions covered. Roll the full circle.`);return;}
        const min=Math.min(...filled),max=Math.max(...filled),suggest=Math.max(60,Math.min(100,Math.floor(min*100)));
        lines.push(`${name}: roundness ${Math.round(min/max*100)}% (weakest edge ${Math.round(min*100)}%, strongest ${Math.round(max*100)}%). Suggested outer range: ${suggest}% in Stick response.`);
      });
      $('output-circ-result').textContent=lines.join(' ');
    }
    defaults(calibration){return {calibration:calibration||{axisCenters:[0,0,0,0],axisMins:[-1,-1,-1,-1],axisMaxs:[1,1,1,1],triggerRests:[0,0],triggerFulls:[1,1],touchMins:[0,0],touchMaxs:[1,1]},axisNoise:[0,0,0,0],triggerNoise:[0,0],enabled:true};}
    fail(error){this.error=String(error?.message||error);this.render();}
    async persist(settings,operation='save'){
      if(!this.connected||this.busy)throw Error('Connect a controller and wait for the current save.');
      if(!this.a.native)throw Error('Native calibration storage is available in the desktop app.');
      const owner=this.owner,epoch=this.epoch;this.busy=true;this.error='';this.render();
      try{
        const record=await this.a.invoke('save_output_calibration',{controllerId:owner,settings,expectedRevision:this.record?.revision??null,operation});
        if(!record||record.controllerId!==owner)throw Error('Calibration storage returned no matching record.');
        if(owner!==this.owner||epoch!==this.epoch)throw Error('Saved for the previous controller. Reconnect it to apply.');
        this.record=record;
        if(!await this.a.send({action:'reloadOutputCalibration',controllerId:owner}))throw Error('Saved; reconnect the controller to apply.');
        return record;
      }finally{if(owner===this.owner&&epoch===this.epoch){this.busy=false;this.render();}}
    }
    async save(calibration,run=null,operation='save'){
      const settings=this.defaults(calibration);
      if(run?.restSamples?.length){
        settings.axisNoise=calibration.axisCenters.map((center,i)=>{
          const span=Math.min(calibration.axisMaxs[i]-center,center-calibration.axisMins[i]);
          return Math.min(.2,(Math.max(...run.restSamples.map(s=>Math.abs(s.axes[i]-center)))+1/127)/span);
        });
        settings.triggerNoise=calibration.triggerRests.map((rest,i)=>Math.min(.1,(Math.max(0,...run.restSamples.map(s=>s.triggers[i]-rest))+1/255)/(calibration.triggerFulls[i]-rest)));
      }
      return this.persist(settings,operation);
    }
    device(e,legacy){
      this.epoch++;this.owner=e.controllerId||e.path||`${e.name}:${e.transport}`;this.connected=!!e.connected;this.busy=false;this.sample=null;this.stale=false;
      this.record=e.outputCalibration?.record||null;this.error=e.outputCalibration?.error||'';this.render();this.a.refreshCalibration?.();
      // One-time migration. A native record (including a bypass choice) wins.
      if(this.a.native&&this.owner&&!this.record&&!this.error&&legacy){this.save(copy(legacy)).catch(error=>this.fail(error));}
    }
    event(e){
      if(e.type==='device'&&!e.connected){this.epoch++;this.connected=false;this.busy=false;this.sample=null;this.render();}
      if(e.controllerId&&e.controllerId!==this.owner)return;
      if(e.type==='outputCalibration'){this.record=e.record||null;this.error=e.error||'';this.a.restoreCalibration?.(this.record?.settings.calibration||null);this.render();}
      if(e.type==='outputInputHealth'){this.stale=e.stale;this.render();}
      if(e.type==='input'){this.sample=e;this.sampleCirc(e.rawAxes||[0,0,0,0]);if(this.a.state.currentView==='calibration'&&!document.hidden)this.paint();}
    }
    render(){
      if(this.a.state.currentView!=='calibration'||document.hidden)return;
      const measured=!!this.record,platform=this.a.state.platformOS||'windows';
      $('output-route').textContent=platform==='windows'?'Virtual Xbox · Windows':platform==='linux'?'Virtual gamepad · Linux':'Input correction';
      $('output-cal-scope').textContent=platform==='windows'?'Games must use the virtual Xbox controller. HidHide can prevent duplicate physical input. Closing the app removes virtual output.':platform==='linux'?'Games must use the uinput virtual gamepad. Physical controller reports stay unchanged. Closing the app removes virtual output.':'Saved ranges apply to this app and desktop controls. A macOS virtual gamepad provider is not integrated.';
      $('output-cal-status').textContent=!this.connected?'Connect a controller.':this.busy?'Saving calibration…':this.stale?'Input paused; virtual output is neutral.':measured?`Native save · revision ${this.record.revision} · correction ${this.record.settings.enabled?'on':'bypassed'}`:'Finish calibration to save ranges and rest noise in the native core.';
      $('output-cal-error').textContent=this.error;
      const blocked=!this.connected||this.busy||!!this.a.calibrationInProgress?.();
      const P=window.controllerPlatform,why=!this.connected?'Needs a connected controller: connect one to use saved calibration.':this.busy?'Wait for the current save to finish.':this.a.calibrationInProgress?.()?'Finish or cancel the calibration steps first.':null;$('output-cal-enabled').disabled=blocked||!measured;P.why($('output-cal-enabled'),$('output-cal-enabled').disabled?why||'Needs a finished calibration: complete the four steps above first.':null);$('output-cal-enabled').checked=this.record?.settings.enabled!==false;
      $('output-cal-enabled').parentElement.hidden=!['windows','linux'].includes(platform);
      $('output-cal-restore').disabled=blocked||!this.a.native;
      $('output-cal-reload').disabled=blocked||!this.a.native;
      for(const id of ['output-cal-restore','output-cal-reload'])P.why($(id),$(id).disabled?(!this.a.native?window.controllerPlatform.desktopOnlyMessage('Saved calibration'):why):null);
      P.gate($('output-circ-start'),!this.connected,'Needs a connected controller: connect one to test stick circularity.');
      for(let i=0;i<2;i++){const noise=this.record?.settings.axisNoise||[0,0,0,0],ring=$('output-dz-'+i);if(ring)ring.setAttribute('r',String(48*Math.min(1,((noise[i*2]||0)+(noise[i*2+1]||0))/2)));}
      for(let i=0;i<2;i++)$('output-noise-'+i).textContent='Rest filter: '+(this.record?this.record.settings.axisNoise.slice(i*2,i*2+2).map(v=>(v*100).toFixed(1)+'%').join(' / '):'none');
      this.paint();
    }
    paint(){
      const e=this.sample,raw=e?.rawAxes||[0,0,0,0],output=this.stale?[0,0,0,0]:(e?.virtualAxes||[0,0,0,0]);
      const key=JSON.stringify([raw,output,e?.rawTriggers,e?.virtualTriggers,this.stale,e?.virtualOutputActive]);
      if(key===this.paintKey)return;this.paintKey=key;
      const bound=v=>Number.isFinite(v)?Math.max(-1,Math.min(1,v)):0;
      if(this.trails&&e){for(let i=0;i<2;i++){const t=this.trails[i];t.push(`${(60+48*bound(raw[i*2])).toFixed(1)},${(60+48*bound(raw[i*2+1])).toFixed(1)}`);if(t.length>14)t.shift();const el=$('output-trail-'+i);if(el)el.setAttribute('points',t.join(' '));}}
      for(let i=0;i<2;i++)for(const [name,values]of [['raw',raw],['game',output]]){const dot=$(`output-${name}-${i}`);dot.setAttribute('cx',60+48*bound(values[i*2]));dot.setAttribute('cy',60+48*bound(values[i*2+1]));}
      for(let i=0;i<2;i++)$(`output-trigger-${i}`).textContent=`${i?'R2':'L2'}: ${Math.round((e?.rawTriggers?.[i]||0)*100)}% → ${Math.round((this.stale?0:e?.virtualTriggers?.[i]||0)*100)}%`;
      $('output-send-status').textContent=this.stale?'Neutral: input stream paused.':e?.virtualOutputActive?'Sent to the virtual Xbox controller.':'Calculated values; bridge is off.';
    }
  }
  window.ControllerOutputCalibration=OutputCalibration;
})();
