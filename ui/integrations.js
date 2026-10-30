/* Explicit capture routes and opt-in transport experiments. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id),copy=v=>structuredClone(v);
  class ControllerIntegrations {
    constructor(a){this.a=a;this.running=false;this.handoff=false;this.activeSource=null;this.results={};this.refreshId=0;this.build();this.renderPlatform();this.refresh();document.addEventListener('controller-platform',()=>this.renderPlatform());}
    node(tag,props,children=[]){return this.a.gameNode(tag,props,children);}
    button(text,id,run){const b=this.node('button',{type:'button',text,id,class:'quiet-action'});b.addEventListener('click',run);return b;}
    panel(parent,title,id){const box=this.node('section',{id,class:'studio-card framed'}),body=this.node('div',{class:'studio-body'});box.append(this.node('div',{class:'panel-cap',text:title}),body);$(parent).append(box);return body;}
    build(){
      const n=this.node.bind(this),source=this.panel('haptics','Audio capture source','capture-source-panel');this.sourcePanel=$('capture-source-panel');document.querySelector('.bridge-panel')?.before(this.sourcePanel);
      this.mode=this.a.gameSelect([['system','System audio'],['process','Selected app'],['nativeEndpoint','Four-channel relay · prototype']],'system',()=>this.renderSelection());this.mode.id='capture-mode';
      this.process=this.a.gameSelect([['','Refresh to list running apps']],'',()=>{});this.process.id='capture-process';
      this.endpoint=this.a.gameSelect([['','No compatible source listed']],'',()=>{});this.endpoint.id='capture-endpoint';
      this.processField=this.a.gameField('Running app and its child processes',this.process);this.endpointField=this.a.gameField('Virtual source · channels 3/4 only',this.endpoint);
      const row=n('div',{class:'game-form-grid'},[this.a.gameField('Capture from',this.mode),this.processField,this.endpointField]);
      this.captureStatus=n('p',{id:'capture-source-status',role:'status',text:'System audio is the default source.'});
      source.append(row,n('div',{class:'game-buttons'},[this.button('Refresh sources','capture-refresh',()=>this.refresh()),this.button('Use selected source','capture-use',()=>this.capture())]),this.captureStatus,
        n('p',{class:'pc-note',text:'Selected app capture includes its process tree and stops when that process exits. A failed capture never switches to all system audio. Choose System audio explicitly if needed.'}),
        n('p',{id:'native-relay-note',class:'pc-note',text:'The relay requires an existing four-channel, 48 kHz virtual render endpoint. Only channels 3/4 go to the grips, without music conversion. This prototype does not install a virtual DualSense HID/audio driver.'}));
      const access=this.panel('pc-controls','Desktop access','runtime-access-panel');
      this.accessStatus=n('p',{id:'runtime-access-status',role:'status',text:'Checking session capabilities…'});
      access.append(this.accessStatus,n('p',{text:'Test opens the desktop backend without moving the pointer or sending keys. It stops current effects and mappings first. A connection does not prove access to every app, especially on Wayland.'}),this.button('Test desktop access','runtime-access-test',()=>this.testAccess()));
      const native=this.panel('profiles','Native game feedback','native-feedback-panel');this.nativeStatus=n('p',{id:'native-handoff-status',role:'status',text:'Monitoring is active. This app has no virtual native DualSense driver.'});
      this.handoffButton=this.button('Release controller to game','native-handoff',()=>this.toggleHandoff());
      native.append(this.nativeStatus,this.handoffButton,n('p',{class:'pc-note',text:'Release closes this app’s HID and audio handles so a native DualSense game can own them. Live input pauses. Resume monitoring afterward. The app’s Stop all cannot stop effects owned by another game.'}),
        n('p',{id:'dongle-status',text:'Connect a USB controller to check for a compatible DS5Dongle interface.'}));
      this.nativeBridge=n('div',{class:'native-bridge-readiness',id:'native-bridge-readiness'},[
        n('strong',{text:'External Bluetooth bridge · vDS'}),
        n('p',{id:'native-bridge-status',text:'Refresh sources to check standard install locations.'}),
        n('p',{id:'native-bridge-requirements'}),
        n('p',{text:'vDS exposes a virtual USB DualSense for native game feedback. Install and attach it externally, then leave monitoring released here while it owns the controller. App effects are paused during handoff.'}),
        this.button('Copy setup guide','native-bridge-guide',async()=>{try{await navigator.clipboard.writeText(this.info?.nativeBridge?.guideUrl||'https://github.com/hurryman2212/vds');this.a.toast('Setup link copied.');}catch{this.a.toast('Could not copy the setup link.');}})
      ]);native.append(this.nativeBridge);
      const lab=this.panel('diagnostics','Wireless haptics lab · experimental','wireless-lab');
      this.strength=n('input',{type:'number',id:'link-test-strength',min:1,max:25,step:1,value:10});
      this.runButton=this.button('Run 2-second test','link-test-start',()=>this.testLink());
      this.testStatus=n('p',{id:'link-test-status',role:'status',text:'No test run. Bluetooth output is experimental and has not been physically verified.'});
      this.comparison=n('div',{class:'link-comparison',id:'link-comparison'});
      lab.append(n('p',{text:'USB and Bluetooth use an 80 Hz, four-pulse-per-second pattern at the selected level. Bluetooth uses 3 kHz signed stereo samples. Tests stop after two seconds and honor personal grip caps. This stops other app effects and input mappings.'}),this.a.gameField('Test strength · %',this.strength),
        n('div',{class:'game-buttons'},[this.runButton,this.button('Stop test','link-test-stop',()=>this.a.send({action:'stopLinkExperiment'}))]),this.testStatus,this.comparison,
        n('p',{class:'pc-note',text:'The figures measure software dispatch and scheduling, not physical latency or equivalent force. USB mixer levels and personal settings also affect feel. Physical comparison needs external measurement.'}));
      const preview=n('details',{class:'native-packet-preview'});preview.append(n('summary',{text:'Native output packet prototype'}));
      this.packet=n('textarea',{id:'native-packet',maxLength:100,rows:3});this.packet.setAttribute('aria-label','USB output report in base64');this.packetStatus=n('p',{id:'native-packet-status',role:'status'});
      preview.append(n('p',{text:'Inspect one captured USB 0x02 output report and preview Bluetooth control framing. This does not write hardware or emulate a driver.'}),this.packet,this.button('Inspect packet','native-packet-inspect',()=>this.inspect()),this.packetStatus);lab.append(preview);
      this.renderSelection();
    }
    renderPlatform(){this.nativeBridge.hidden=this.a.state.platformOS==='macos';this.sourcePanel.hidden=!window.controllerPlatform.systemAudioBridge;this.runButton.disabled=!this.a.state.connected||this.running;$('capture-use').disabled=!this.a.state.connected||this.a.state.transport!=='USB'||!window.controllerPlatform.systemAudioBridge;}
    renderSelection(){this.processField.hidden=this.mode.value!=='process';this.endpointField.hidden=this.mode.value!=='nativeEndpoint';$('native-relay-note').hidden=this.mode.value!=='nativeEndpoint';}
    selectedSource(){
      if(this.mode.value==='process'){const pid=Number(this.process.value);if(!Number.isInteger(pid)||pid<=4)throw Error('Choose a running app.');return{kind:'process',pid};}
      if(this.mode.value==='nativeEndpoint'){if(!this.endpoint.value)throw Error('Choose a compatible virtual source endpoint.');return{kind:'nativeEndpoint',id:this.endpoint.value};}
      return{kind:'system'};
    }
    commandSource(){return copy(this.a.state.audioBridge&&this.activeSource?this.activeSource:this.selectedSource());}
    async refresh(){
      const id=++this.refreshId;$('capture-refresh').disabled=true;
      try{
        const info=await this.a.invoke('integration_info');if(id!==this.refreshId||!info)return;
        this.info=info;const oldPid=this.process.value,oldEndpoint=this.endpoint.value;
        this.nativeBridge.hidden=(info.nativeBridge?.platform||this.a.state.platformOS)==='macos';
        const bridge=info.nativeBridge;
        $('native-bridge-status').textContent=bridge?`${bridge.toolFound?'Control tool found':'Control tool not found'} · ${bridge.daemonFound?'daemon found':'daemon not found'}. Driver and service readiness have not been tested.`:'External bridge discovery is unavailable.';
        $('native-bridge-requirements').textContent=bridge?.requirements||'';
        this.process.replaceChildren(new Option('Choose a running app',''));for(const p of info.processes||[])this.process.add(new Option(`${p.name} · PID ${p.pid}`,String(p.pid)));
        if([...this.process.options].some(o=>o.value===oldPid))this.process.value=oldPid;
        this.endpoint.replaceChildren(new Option('Choose a four-channel source',''));for(const e of info.nativeRelayEndpoints||[])this.endpoint.add(new Option(e.name,e.id));
        if([...this.endpoint.options].some(o=>o.value===oldEndpoint))this.endpoint.value=oldEndpoint;
        this.mode.querySelector('[value=process]').disabled=!info.processCapture;
        if(!info.processCapture&&this.mode.value==='process'){this.mode.value='system';this.renderSelection();}
        if(!this.accessChecked)this.accessStatus.textContent=info.environment?.message||'Session information unavailable.';
        if(!this.a.state.audioBridge)this.captureStatus.textContent=info.processCapture?'Choose the app to capture; PID selections are not saved in profiles.':'Process capture needs Windows build 20348 or newer. System audio remains available.';
      }catch(error){if(id===this.refreshId)this.captureStatus.textContent=`Source discovery failed: ${error}`;}
      finally{if(id===this.refreshId)$('capture-refresh').disabled=false;}
    }
    async capture(){
      if(!this.a.state.connected)return this.a.toast('Connect the controller by USB.');
      try{const source=this.selectedSource();this.pendingCapture=true;this.captureStatus.textContent='Starting selected capture…';
        if(!await this.a.send({action:'setAudioBridge',enabled:true,gain:+$('bridge-gain').value/100,preset:$('bridge-preset').value,source})){this.pendingCapture=false;this.captureStatus.textContent='Capture request failed.';}
      }catch(error){this.captureStatus.textContent=String(error);}
    }
    async testAccess(){
      this.accessStatus.textContent='Opening desktop backend…';$('runtime-access-test').disabled=true;
      if(!await this.a.send({action:'checkDesktopInput'})){this.accessStatus.textContent='Could not contact the controller core.';$('runtime-access-test').disabled=false;}
    }
    async toggleHandoff(){
      if(!this.handoff)await window.dailyUse.stopAll();
      await this.a.send({action:'setNativeHandoff',enabled:!this.handoff});
    }
    async testLink(){
      const strength=Number(this.strength.value);if(!Number.isInteger(strength)||strength<1||strength>25)return this.a.toast('Choose 1–25%.');
      if(!this.a.state.connected)return this.a.toast('Connect a controller first.');
      this.startingLink=true;this.runButton.disabled=true;this.testStatus.textContent='Starting bounded link test…';
      if(!await this.a.send({action:'startLinkExperiment',strength})){this.testStatus.textContent='Test request failed.';this.renderPlatform();}
    }
    async inspect(){try{const result=await this.a.invoke('inspect_native_report',{reportBase64:this.packet.value.trim()});if(!result)return;this.packetStatus.textContent=`Preview only · ${result.usbBytes} USB bytes → ${result.bluetoothBytes} Bluetooth bytes. L2 mode ${result.leftTriggerMode}, R2 mode ${result.rightTriggerMode}. No hardware write.`;}catch(error){this.packetStatus.textContent=String(error);}}
    event(e){
      if(e.type==='device'){
        if(e.connected){this.handoff=false;this.handoffButton.textContent='Release controller to game';const d=e.dongle;$('dongle-status').textContent=d?.compatible?`DS5Dongle-compatible interface · ${d.firmware}. The app uses its USB route; audio still needs an available four-channel endpoint. Firmware behavior is unverified here.`:'No compatible dongle interface identified. A USB identity alone cannot distinguish a cable from a bridge.';}
        else{if(this.running||this.startingLink)this.testStatus.textContent='Link test stopped: controller disconnected.';this.startingLink=false;this.running=false;this.activeSource=null;this.pendingCapture=false;this.captureStatus.textContent='Capture stopped; no controller connection owned by the app.';}
        queueMicrotask(()=>this.renderPlatform());
      }
      if(e.type==='nativeHandoff'){this.handoff=!!e.enabled;this.handoffButton.textContent=this.handoff?'Resume monitoring':'Release controller to game';this.nativeStatus.textContent=this.handoff?'Released. The native game owns input/audio feedback; app monitoring and output are paused.':'Monitoring resumed. Native game feedback may compete with app effects.';}
      if(e.type==='audioStatus'&&typeof e.bridge==='boolean'){this.activeSource=e.bridge?e.captureSource||this.activeSource||{kind:'system'}:null;this.pendingCapture=false;this.captureStatus.textContent=e.bridge?`Active capture: ${this.activeSource.kind==='process'?'process tree '+this.activeSource.pid:this.activeSource.kind==='nativeEndpoint'?'four-channel relay':'system audio'}.`:e.message||'Capture stopped.';if(e.captureError)this.captureStatus.textContent=`${e.message}. ${e.bridge?'Previous capture remains active.':'Capture is off.'}`;}
      if(e.type==='audioMeters'&&e.captureSource){this.activeSource=e.captureSource;if(e.captureState==='starting')this.captureStatus.textContent='Starting selected app capture; waiting for the Windows audio client.';else if(e.captureState==='running')this.captureStatus.textContent=`Capturing ${e.captureSource.kind==='process'?'process tree '+e.captureSource.pid:e.captureSource.kind==='nativeEndpoint'?'native haptic channels 3/4':'system audio'}.`;}
      if(e.type==='desktopInputStatus'){this.accessChecked=true;this.accessStatus.textContent=(e.ready?'Backend connection opened. ':'Access unavailable. ')+(e.message||'');$('runtime-access-test').disabled=false;}
      if(e.type==='linkExperiment'){
        this.startingLink=false;this.running=!!e.running;this.runButton.disabled=this.running||!this.a.state.connected;
        if(e.error){this.testStatus.textContent=e.error;return;}
        const timing=e.transport==='Bluetooth'?`max schedule delay ${((e.maxScheduleLateUs||0)/1000).toFixed(2)} ms / max HID write ${((e.maxWriteUs||0)/1000).toFixed(2)} ms`:`startup ${((e.startupUs||0)/1000).toFixed(2)} ms; HID timing not applicable`;
        this.testStatus.textContent=`${e.transport} \u00b7 ${e.running?'running':e.completed?'finished':'stopped'} \u00b7 ${e.sent||0} ${e.unit||'reports'} \u00b7 skipped ${e.skipped||0} \u00b7 ${timing}. Physical latency not measured.`;
        if(!e.running&&e.completed&&e.transport){this.results[e.transport]=e;this.comparison.replaceChildren();for(const [transport,result]of Object.entries(this.results))this.comparison.append(this.node('p',{text:`${transport}: ${result.sent||0} ${result.unit||'reports'}, ${transport==='Bluetooth'?((result.maxWriteUs||0)/1000).toFixed(2)+' ms maximum HID write':((result.startupUs||0)/1000).toFixed(2)+' ms startup; HID timing not applicable'}.`}));}
      }
      if(e.type==='error'&&this.startingLink){this.startingLink=false;this.testStatus.textContent=e.message||'Link test failed.';this.renderPlatform();}
      if(e.type==='error'&&this.pendingCapture){this.pendingCapture=false;this.captureStatus.textContent=e.message||'Capture failed. No source fallback was enabled.';}
      if(e.type==='safetyReset'){if(this.running||this.startingLink)this.testStatus.textContent='Link test stopped by Reset.';this.running=false;this.startingLink=false;this.activeSource=null;this.pendingCapture=false;this.captureStatus.textContent='Capture stopped by Reset.';this.runButton.disabled=!this.a.state.connected;}
      if(e.type==='core'&&e.status==='ready'){this.handoff=false;this.handoffButton.textContent='Release controller to game';}
    }
  }
  window.ControllerIntegrations=ControllerIntegrations;
})();
