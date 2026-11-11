/* Passive diagnostics. Trace analysis never feeds input or effects back to the core. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id);
  const quantiles=values=>{
    const sorted=values.filter(Number.isFinite).sort((a,b)=>a-b),count=sorted.length;
    return count?{count,p50:sorted[Math.floor((count-1)/2)],p95:sorted[Math.ceil(count*.95)-1],max:sorted[count-1]}:{count:0};
  };
  class ControllerRuntimeLab {
    constructor(api){
      this.a=api;this.pending=new Map();this.nextProbe=1;this.rtts=[];this.replayId=0;this.tracePending='';this.active=false;
      const n=api.gameNode,host=$('diagnostics');
      this.box=n('section',{id:'runtime-lab',class:'work-panel framed runtime-lab'});window.controllerPlatform?.section?.(this.box,'diag.runtime','tool','Timing and traces','Round-trip timing and raw input traces',{keywords:'timing latency round trip ping trace hid record input replay cpu memory jitter sample'});
      const body=n('div',{class:'game-content'});
      this.box.append(n('div',{class:'panel-cap',text:'Timing and input traces'}),body);host.append(this.box);
      body.append(n('p',{class:'pc-note',text:'Software timings show where the app waits. They do not measure physical controller latency.'}));
      this.status=n('p',{id:'runtime-status',role:'status',text:'Open the desktop app to inspect the core.'});
      this.table=n('div',{id:'runtime-times',class:'runtime-times'});
      this.resources=n('p',{id:'runtime-resources',class:'pc-note',text:'Core process resources: waiting for a sample.'});
      const controls=n('div',{class:'game-buttons'});
      this.ping=this.button('Measure round trip','runtime-ping',()=>this.probe());
      this.sample=this.button('Capture 5 s sample','runtime-sample',()=>this.capture());
      this.copy=this.button('Copy timing report','runtime-copy',()=>this.copyReport());
      controls.append(this.ping,this.sample,this.copy);body.append(this.status,this.table,this.resources,controls);
      const trace=n('div',{class:'runtime-trace'}),buttons=n('div',{class:'game-buttons'});
      this.record=this.button('Record input','hid-record',async()=>{await this.a.send({action:'setHidRecording',enabled:true});});
      this.stop=this.button('Stop recording','hid-stop',()=>this.a.send({action:'setHidRecording',enabled:false}));this.stop.disabled=true;
      this.exportTrace=this.button('Export trace','hid-export',()=>this.getTrace('download'));
      this.copyTrace=this.button('Copy trace','hid-copy',()=>this.getTrace('copy'));
      this.exportTrace.disabled=this.copyTrace.disabled=true;
      buttons.append(this.record,this.stop,this.exportTrace,this.copyTrace);
      this.traceStatus=n('p',{id:'hid-trace-status',role:'status',text:'Record up to ten seconds. The latest 2,048 events are kept in memory.'});
      this.file=n('input',{type:'file',id:'hid-replay-file',accept:'.json,application/json'});this.file.setAttribute('aria-label','Input trace to analyze');
      this.file.addEventListener('change',async()=>{
        const file=this.file.files[0];if(!file)return;
        if(file.size>1024*1024){this.replay.textContent='Trace exceeds 1 MiB.';this.file.value='';return;}
        const id=++this.replayId;this.replay.textContent='Analyzing input trace…';
        try{const json=await file.text();if(id!==this.replayId)return;
          if(!await this.a.send({action:'inspectHidTrace',json,requestId:id}))this.replay.textContent='Could not send the trace to the core.';
        }catch(error){if(id===this.replayId)this.replay.textContent=`Could not read trace: ${error.message}`;}
        this.file.value='';
      });
      this.replay=n('p',{id:'hid-replay-result',role:'status',text:'Analyze a trace to check decoding, reconnects and report timing. No inputs or effects are played.'});
      trace.append(n('h3',{text:'Raw input trace'}),buttons,this.traceStatus,this.file,this.replay);body.append(trace);
      this.file.disabled=!api.native;this.gate();
      document.addEventListener('controller-view',()=>this.watch());
      document.addEventListener('visibilitychange',()=>this.watch());
      window.addEventListener('pagehide',()=>{clearInterval(this.lease);this.a.send({action:'setRuntimeDiagnostics',enabled:false});});
    }
    pushHistory(e){
      const hist=this.hist||(this.hist={});
      for(const [label,value] of [['HID read wait',e.metrics?.readWait],['Decode',e.metrics?.decode],['Input processing',e.metrics?.dispatch],['Loop period',e.metrics?.loopPeriod],['HID write call',e.write]]){
        if(!value?.count)continue;const h=hist[label]||(hist[label]={p50:[],p95:[]});h.p50.push(value.p50Us/1000);h.p95.push(value.p95Us/1000);if(h.p50.length>60){h.p50.shift();h.p95.shift();}
      }
    }
    spark(values,w=90,h=20,color='var(--teal)'){
      const NS='http://www.w3.org/2000/svg',svg=document.createElementNS(NS,'svg');svg.setAttribute('viewBox',`0 0 ${w} ${h}`);svg.setAttribute('width',w);svg.setAttribute('height',h);svg.setAttribute('role','img');svg.setAttribute('aria-label','Trend of recent samples');svg.classList.add('runtime-spark');
      if(values.length<2)return svg;const max=Math.max(...values,1e-6),min=Math.min(...values);const span=max-min||1;
      const path=document.createElementNS(NS,'polyline');path.setAttribute('fill','none');path.style.stroke=color;path.setAttribute('stroke-width','1.5');
      path.setAttribute('points',values.map((v,i)=>`${(i/(values.length-1)*(w-2)+1).toFixed(1)},${(h-2-(v-min)/span*(h-4)).toFixed(1)}`).join(' '));svg.append(path);return svg;
    }
    renderChart(){
      if(!this.chart){this.chart=this.a.gameNode('div',{id:'runtime-chart',class:'runtime-chart'});this.table.after(this.chart);const st=document.createElement('style');st.textContent='.runtime-time{display:grid;grid-template-columns:150px 1fr auto;gap:8px;align-items:center}.runtime-spark{background:var(--stage-mid,#192e42);border-radius:3px}.runtime-chart small{display:block;color:var(--parchment-dim,#d5e3df)}';document.head.append(st);}
      const loop=this.hist?.['Loop period'],jitter=loop?loop.p95.map((v,i)=>Math.max(0,v-loop.p50[i])):[];
      this.chart.replaceChildren();
      const row=(title,values,unit)=>{const wrap=this.a.gameNode('div');wrap.append(this.a.gameNode('small',{text:`${title}${values.length?` - latest ${values[values.length-1].toFixed(2)} ${unit}, peak ${Math.max(...values).toFixed(2)} ${unit}`:' - no samples yet'}`}),this.spark(values,320,40));this.chart.append(wrap);};
      row('Loop jitter over time (p95 minus median)',jitter,'ms');
      row('UI/core round trip, last probe',this.rtts||[],'ms');
    }
    gate(){
      const P=window.controllerPlatform,nat=!!this.a.native,conn=!!this.a.state.connected,reason=!nat?window.controllerPlatform.desktopOnlyMessage('This tool'):!conn?'Needs a connected controller: connect one to measure or record.':null;
      for(const [button,busy] of [[this.ping,!!this.probing],[this.sample,!!this.sampling],[this.record,!!this.recording]])P.gate(button,!reason?busy:true,reason);
    }
    button(text,id,run){const button=this.a.gameNode('button',{type:'button',id,class:'quiet-action',text});button.addEventListener('click',run);return button;}
    visible(){return !document.hidden&&this.a.state.currentView==='diagnostics';}
    watch(){
      const enabled=!!(this.a.native&&(this.visible()||this.sampling));
      if(enabled!==this.active){this.active=enabled;this.a.send({action:'setRuntimeDiagnostics',enabled});}
      clearInterval(this.lease);this.lease=0;
      if(enabled)this.lease=setInterval(()=>this.a.send({action:'setRuntimeDiagnostics',enabled:true}),15000);
      if(this.visible())this.render();
    }
    event(e){
      if(e.type==='device'){queueMicrotask(()=>this.gate());if(!e.connected||e.controllerId!==this.audio?.controllerId)this.audio=null;}
      if(e.type==='audioMeters'){this.audio=e;return;}
      if(e.type==='runtimeStats'){
        const previous=this.latest?.core;this.latest=e;this.pushHistory(e);
        const dt=e.core?.sampledAtUs-previous?.sampledAtUs,dc=e.core?.cpuTimeUs-previous?.cpuTimeUs;
        this.cpu=dt>0&&dc>=0&&e.core.pid===previous.pid?dc/dt*100:null;
        if(this.sampling&&this.samples.length<30)this.samples.push({core:e.core,cpuPercent:this.cpu,view:this.a.state.currentView,hidden:document.hidden});
        if(this.visible())this.render();return;
      }
      if(e.type==='latencyEcho'){
        if(!this.pending.has(e.id))return;
        this.rtts.push(performance.now()-this.pending.get(e.id));this.pending.delete(e.id);
        if(this.finishedSending&&!this.pending.size)this.finishProbe();return;
      }
      if(e.type==='hidRecording'){
        this.recording=!!e.active;this.gate();this.stop.disabled=!this.recording;
        this.exportTrace.disabled=this.copyTrace.disabled=!!this.tracePending||(!this.recording&&!e.events);
        this.traceStatus.textContent=this.recording?'Recording raw input. Effects and desktop input are not captured.':`${e.events||0} events available. Recording stopped.`;return;
      }
      if(e.type==='hidTrace'&&this.tracePending){
        const mode=this.tracePending;this.tracePending='';this.exportTrace.disabled=this.copyTrace.disabled=false;
        if(e.error){this.traceStatus.textContent=e.error;return;}
        const json=JSON.stringify(e.trace);
        if(mode==='copy')navigator.clipboard.writeText(json).then(()=>this.a.toast('Input trace copied.'),()=>this.a.toast('Could not copy the trace. Try Export.'));
        else this.download(json,'controller-input-trace.json');return;
      }
      if(e.type==='hidReplay'&&e.requestId===this.replayId){
        if(e.error){this.replay.textContent=e.error;return;}
        const r=e.report,q=quantiles(r.intervalUs||[]),interval=q.count?`${(q.p50/1000).toFixed(2)} ms median report interval`:'no report intervals';
        this.replay.textContent=`${r.validReports} valid / ${r.invalidReports} invalid reports; ${r.connections} connection states, ${r.disconnects} disconnects, ${r.readErrors} read errors. ${r.sequenceGaps} sequence gaps, ${r.duplicateSequences} duplicates, ${r.staleIntervals} pauses over 250 ms; ${interval}.${r.droppedEvents?` ${r.droppedEvents} older events were trimmed.`:''}`;
      }
    }
    render(){
      if(!this.visible())return;
      const n=this.a.gameNode,stats=this.latest;
      if(stats){
        this.table.replaceChildren();
        for(const [label,value] of [['HID read wait',stats.metrics.readWait],['Decode',stats.metrics.decode],['Input processing',stats.metrics.dispatch],['Loop period',stats.metrics.loopPeriod],['HID write call',stats.write]]){
          const row=n('div',{class:'runtime-time'});row.append(n('strong',{text:label}),n('span',{text:value?.count?`${(value.p50Us/1000).toFixed(3)} ms median / ${(value.p95Us/1000).toFixed(3)} ms p95 / ${(value.maxUs/1000).toFixed(3)} ms max`:'No samples'}),this.spark(this.hist?.[label]?.p50||[]));this.table.append(row);
        }
        this.renderChart();
        const discovery=stats.processDiscovery,ram=stats.core?.residentBytes;
        this.resources.textContent=`Core CPU: ${this.cpu==null?'waiting':this.cpu.toFixed(1)+'%'}; memory: ${ram==null?'not available':(ram/1048576).toFixed(1)+' MiB'}. Discovery: ${discovery?.ready?`${(discovery.durationUs/1000).toFixed(1)} ms; ${discovery.ageMs} ms old`:'starting'}. CPU uses 100% for one core; memory covers this process only.`;
      }
      if(!this.probing&&!this.sampling)this.status.textContent=this.probeResult?`UI/core round trip: ${this.probeResult.count}/20 replies; median ${this.probeResult.p50?.toFixed(2)??'—'} ms, p95 ${this.probeResult.p95?.toFixed(2)??'—'} ms.${this.probeResult.missing?' Some replies timed out.':''}`:'Timings use the latest 512 samples. HID read wait includes time waiting for a report.';
    }
    async probe(){
      if(this.probing)return;if(!this.a.native){this.status.textContent=window.controllerPlatform.desktopOnlyMessage('The round trip measurement');return;}if(!this.a.state.connected){this.status.textContent='Connect a controller to measure the round trip.';return;}this.probing=true;this.gate();this.rtts=[];this.pending.clear();this.finishedSending=false;
      this.status.textContent='Measuring 20 UI/core round trips. No controller output is sent.';
      for(let i=0;i<20&&this.probing;i++){
        const id=this.nextProbe++;this.pending.set(id,performance.now());
        if(!await this.a.send({action:'latencyProbe',id}))this.pending.delete(id);
        await new Promise(resolve=>setTimeout(resolve,50));
      }
      this.finishedSending=true;
      if(!this.pending.size)this.finishProbe();else this.probeDeadline=setTimeout(()=>this.finishProbe(),4000);
    }
    finishProbe(){
      if(!this.probing)return;clearTimeout(this.probeDeadline);this.probing=false;this.gate();
      this.probeResult={...quantiles(this.rtts),missing:20-this.rtts.length};this.pending.clear();this.render();
    }
    capture(){
      if(this.sampling)return;this.sampling=true;this.samples=[];this.sampleStart=this.a.visualStats();this.gate();this.watch();
      this.status.textContent='Capturing five seconds of core resources and render activity. You can minimize the window.';
      setTimeout(()=>{this.sampling=false;this.gate();this.sampleEnd=this.a.visualStats();this.watch();if(this.visible())this.status.textContent=`Captured ${this.samples.length} core samples. Copy the timing report to save them.`;},5000);
    }
    getTrace(mode){
      if(this.tracePending)return;this.tracePending=mode;this.exportTrace.disabled=this.copyTrace.disabled=true;
      this.a.send({action:'getHidTrace'}).then(ok=>{if(!ok){this.tracePending='';this.exportTrace.disabled=this.copyTrace.disabled=false;this.traceStatus.textContent='Could not retrieve trace.';}});
    }
    copyReport(){
      const report={schemaVersion:1,appVersion:document.querySelector('.side-code')?.textContent,softwareOnly:true,latest:this.latest||null,roundTrip:this.probeResult||null,resources:this.samples||[],renderStart:this.sampleStart||null,renderEnd:this.sampleEnd||this.a.visualStats()};
      navigator.clipboard.writeText(JSON.stringify(report,null,2)).then(()=>this.a.toast('Timing report copied.'),()=>this.a.toast('Could not copy timing report.'));
    }
    download(json,name){const url=URL.createObjectURL(new Blob([json],{type:'application/json'})),link=document.createElement('a');link.href=url;link.download=name;link.click();setTimeout(()=>URL.revokeObjectURL(url),30000);}
  }
  window.ControllerRuntimeLab=ControllerRuntimeLab;
})();
