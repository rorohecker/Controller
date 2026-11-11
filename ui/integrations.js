/* Explicit capture routes and opt-in transport experiments. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id),copy=v=>structuredClone(v);
  class ControllerIntegrations {
    constructor(a){this.a=a;this.running=false;this.handoff=false;this.activeSource=null;this.results={};this.refreshId=0;this.build();this.renderPlatform();this.refresh();document.addEventListener('controller-platform',()=>this.renderPlatform());}
    node(tag,props,children=[]){return this.a.gameNode(tag,props,children);}
    button(text,id,run){const b=this.node('button',{type:'button',text,id,class:'quiet-action'});b.addEventListener('click',run);return b;}
    panel(parent,title,id,meta){const box=this.node('section',{id,class:'studio-card framed'}),body=this.node('div',{class:'studio-body'});box.append(this.node('div',{class:'panel-cap',text:title}),body);if(meta)window.controllerPlatform?.section?.(box,meta.id,meta.level,meta.title||title,meta.hint,meta);$(parent).append(box);return body;}
    build(){
      const n=this.node.bind(this),source=this.panel('haptics','Audio capture source','capture-source-panel',{id:'haptics.capture',level:'advanced',title:'Audio capture source',hint:'Which app or device the bridge listens to',keywords:'capture source app process system audio loopback relay virtual endpoint'});this.sourcePanel=$('capture-source-panel');this.sourcePanel.dataset.hapticGroup='bridge';document.querySelector('.bridge-panel')?.before(this.sourcePanel);
      this.mode=this.a.gameSelect([['system','System audio'],['process','Selected app'],['nativeEndpoint','Four-channel relay · prototype']],'system',()=>this.renderSelection());this.mode.id='capture-mode';
      this.process=this.a.gameSelect([['','Refresh to list running apps']],'',()=>{});this.process.id='capture-process';
      this.endpoint=this.a.gameSelect([['','No compatible source listed']],'',()=>{});this.endpoint.id='capture-endpoint';
      this.processField=this.a.gameField('Running app and its child processes',this.process);this.endpointField=this.a.gameField('Virtual source · channels 3/4 only',this.endpoint);
      this.reattach=n('input',{type:'checkbox',id:'capture-reattach'});
      try{this.reattach.checked=localStorage.getItem('controller.processReattach')==='1';}catch{}
      this.reattach.addEventListener('change',()=>{try{localStorage.setItem('controller.processReattach',this.reattach.checked?'1':'0');}catch{}if(this.a.state.connected)this.a.send({action:'setProcessReattach',enabled:this.reattach.checked});});
      this.reattachField=n('label',{class:'check-row',id:'capture-reattach-row',title:'If the captured app exits and starts again, resume capture automatically for up to 30 seconds.'},[this.reattach,document.createTextNode(' Reattach if the app restarts (30 s)')]);
      const row=n('div',{class:'game-form-grid'},[this.a.gameField('Capture from',this.mode),this.processField,this.endpointField,this.reattachField]);
      this.captureStatus=n('p',{id:'capture-source-status',role:'status',text:'System audio is the default source.'});
      source.append(row,n('div',{class:'game-buttons'},[this.button('Refresh sources','capture-refresh',()=>this.refresh()),this.button('Use selected source','capture-use',()=>this.capture())]),this.captureStatus,
        n('p',{class:'pc-note',text:'Selected app capture includes its process tree and stops when that process exits. A failed capture never switches to all system audio. Choose System audio explicitly if needed.'}),
        n('p',{id:'native-relay-note',class:'pc-note',text:'The relay requires an existing four-channel, 48 kHz virtual render endpoint. Only channels 3/4 go to the grips, without music conversion. This prototype does not install a virtual DualSense HID/audio driver.'}));
      const access=this.panel('pc-controls','Desktop access','runtime-access-panel',{id:'pc.access',level:'tool',title:'Desktop access',hint:'Test that this PC lets the app move the pointer',keywords:'desktop access permission wayland accessibility test pointer keys'});
      const bt=this.panel('haptics','Bluetooth grip audio (experimental)','bluetooth-audio-panel',{id:'haptics.bluetooth-audio',level:'experimental',title:'Bluetooth grip audio',hint:'Unverified audio over Bluetooth',keywords:'bluetooth wireless audio grips haptics experimental'});this.btPanel=$('bluetooth-audio-panel');this.btPanel.dataset.hapticGroup='bridge';
      this.btStatus=n('p',{id:'bluetooth-audio-status',role:'status',text:'Off. Enable for this connection to use haptic previews, Grips only playback, and system audio. 3 kHz stereo; peak output is limited to 25%. The speaker needs USB.'});
      this.btButton=this.button('Enable Bluetooth grip audio','bluetooth-audio-toggle',async()=>{this.btPending=true;this.renderPlatform();if(!await this.a.send({action:'setBluetoothAudio',controllerId:this.controllerId,enabled:!this.btEnabled})){this.btPending=false;this.btStatus.textContent='Request failed. Try again.';this.renderPlatform();}});bt.append(this.btStatus,this.btButton);
      /* The panel is experimental-gated; Bluetooth users get a plain note and a switch instead of nothing. */
      this.btNote=n('div',{id:'bluetooth-audio-note',class:'pc-note gate-note',hidden:true},[n('span',{text:'Grip vibration over Bluetooth audio is experimental and hidden.'}),this.button('Show experimental features','bluetooth-audio-show',()=>window.controllerUI?.showExperimental(true))]);
      this.btNote.dataset.hapticGroup='bridge';$('haptics').append(this.btNote);window.controllerUI?.onChange?.(()=>this.renderBtNote());
      this.accessStatus=n('p',{id:'runtime-access-status',role:'status',text:'Checking session capabilities…'});
      access.append(this.accessStatus,n('p',{text:'Test opens the desktop backend without moving the pointer or sending keys. It stops current effects and mappings first. A connection does not prove access to every app, especially on Wayland.'}),this.button('Test desktop access','runtime-access-test',()=>this.testAccess()));
      const native=this.panel('profiles','Native game feedback','native-feedback-panel',{id:'profiles.native',level:'tool',title:'Native game feedback',hint:'Release the controller to a native game or use an external bridge',keywords:'native handoff release vds bridge bluetooth dualsense game feedback'});this.nativeStatus=n('p',{id:'native-handoff-status',role:'status',text:'Monitoring is active. Release monitoring for native games, or use an installed vDS Bluetooth bridge.'});
      this.handoffButton=this.button('Release controller to game','native-handoff',()=>this.toggleHandoff());
      native.append(this.nativeStatus,this.handoffButton,n('p',{class:'pc-note',text:'Release closes this app’s HID and audio handles so a native DualSense game can own them. Live input pauses. Resume monitoring afterward. The app’s Stop all cannot stop effects owned by another game.'}),
        n('p',{id:'dongle-status',text:'Connect a USB controller to check for a compatible DS5Dongle interface.'}));
      this.nativeBridge=n('div',{class:'native-bridge-readiness',id:'native-bridge-readiness'},[
        n('strong',{text:'External Bluetooth bridge · vDS'}),
        n('p',{id:'native-bridge-status',text:'Refresh sources to check standard install locations.'}),
        n('p',{id:'native-bridge-requirements'}),
        n('p',{text:'vDS exposes a virtual USB DualSense for native game feedback. Install its runtime and drivers, inspect the service, then attach the selected Bluetooth controller. App monitoring and effects pause during handoff.'}),
        this.button('Copy setup guide','native-bridge-guide',async()=>{try{await navigator.clipboard.writeText(this.info?.nativeBridge?.guideUrl||'https://github.com/hurryman2212/vds');this.a.toast('Setup link copied.');}catch{this.a.toast('Could not copy the setup link.');}})
      ]);native.append(this.nativeBridge);
      this.providerDetails=n('p',{id:'native-provider-status',role:'status',text:'Inspect an installed bridge to check its service and paired controllers.'});
      this.providerInspect=this.button('Inspect bridge','native-provider-inspect',()=>this.providerCommand('inspect'));
      this.providerAttach=this.button('Attach selected controller','native-provider-attach',()=>this.providerCommand('attach'));
      this.providerDetach=this.button('Detach & resume','native-provider-detach',()=>this.providerCommand('detach'));
      this.providerRecover=this.button('Recover connection','native-provider-recover',()=>{this.providerRecovery.hidden=false;});
      this.providerRecovery=n('div',{class:'hardware-write-review',hidden:true},[n('p',{text:'Stop the external vDS service first. Clearing this record allows app monitoring to resume; it does not stop a running bridge.'}),this.button('Bridge stopped · clear record','native-provider-forget',()=>{this.providerRecovery.hidden=true;this.providerCommand('releaseRecord',true);}),this.button('Cancel','native-provider-cancel',()=>{this.providerRecovery.hidden=true;})]);
      this.nativeBridge.append(this.providerDetails,n('div',{class:'game-buttons'},[this.providerInspect,this.providerAttach,this.providerDetach,this.providerRecover]),this.providerRecovery);
      const lab=this.panel('diagnostics','Wireless haptics lab (experimental)','wireless-lab',{id:'diag.wireless-lab',level:'experimental',title:'Wireless haptics lab',hint:'Unverified Bluetooth tests',keywords:'bluetooth wireless latency link test haptics lab'});
      this.strength=n('input',{type:'number',id:'link-test-strength',min:1,max:25,step:1,value:10});
      this.runButton=this.button('Run 2-second test','link-test-start',()=>this.testLink());
      this.testStatus=n('p',{id:'link-test-status',role:'status',text:'No test run. Bluetooth output is experimental and has not been physically verified.'});
      this.comparison=n('div',{class:'link-comparison',id:'link-comparison'});
      lab.append(n('p',{text:'USB and Bluetooth use an 80 Hz, four-pulse-per-second pattern at the selected level. Bluetooth uses 3 kHz signed stereo samples. Tests stop after two seconds and honor personal grip caps. This stops other app effects and input mappings.'}),this.a.gameField('Test strength · %',this.strength),
        n('div',{class:'game-buttons'},[this.runButton,this.button('Stop test','link-test-stop',()=>this.a.send({action:'stopLinkExperiment'}))]),this.testStatus,this.comparison,
        n('p',{class:'pc-note',text:'The figures measure software dispatch and scheduling, not physical latency or equivalent force. USB mixer levels and personal settings also affect feel. Physical comparison needs external measurement.'}));
      const preview=n('details',{class:'native-packet-preview'});window.controllerPlatform?.section?.(preview,'diag.native-packet','experimental','Native output packet prototype','Inspect a native report',{keywords:'packet native output report usb bluetooth inspect'});preview.append(n('summary',{text:'Native output packet prototype'}));
      this.packet=n('textarea',{id:'native-packet',maxLength:100,rows:3});this.packet.setAttribute('aria-label','USB output report in base64');this.packetStatus=n('p',{id:'native-packet-status',role:'status'});
      preview.append(n('p',{text:'Inspect one captured USB 0x02 output report and preview Bluetooth control framing. This does not write hardware or emulate a driver.'}),this.packet,this.button('Inspect packet','native-packet-inspect',()=>this.inspect()),this.packetStatus);lab.append(preview);
      this.renderSelection();
    }
    renderPlatform(){const P=window.controllerPlatform,conn=!!this.a.state.connected;this.nativeBridge.hidden=this.a.state.platformOS==='macos';this.sourcePanel.hidden=!window.controllerPlatform.systemAudioBridge;this.runButton.disabled=!this.a.state.connected||this.running;P.why(this.runButton,this.runButton.disabled&&!conn?'Needs a connected controller: connect one by USB or Bluetooth.':null);$('link-test-stop').disabled=!conn&&!this.running;P.why($('link-test-stop'),$('link-test-stop').disabled?'Needs a running test: connect a controller and start one.':null);$('capture-use').disabled=!this.a.state.connected||!(this.a.state.transport==='USB'||this.btEnabled)||!window.controllerPlatform.systemAudioBridge;P.why($('capture-use'),$('capture-use').disabled?(!window.controllerPlatform.systemAudioBridge?'Needs system audio capture, which this platform does not support.':!conn?'Needs a connected controller: connect one by USB.':'Needs a USB connection (or Bluetooth grip audio turned on).'):null);this.syncModes();this.btPanel.hidden=this.a.state.transport!=='Bluetooth';this.btButton.disabled=!this.a.state.connected||!!this.btPending||this.running;P.why(this.btButton,!conn?'Needs a connected controller over Bluetooth.':null);this.btButton.textContent=this.btEnabled?'Disable Bluetooth grip audio':'Enable Bluetooth grip audio';this.btButton.setAttribute('aria-pressed',String(!!this.btEnabled));this.renderBtNote();this.renderProvider();}
    renderBtNote(){const ui=window.controllerUI?.prefs?.();const gated=!!ui&&!ui.showExperimental&&!ui.showAll&&document.querySelector('[data-ui-wrap="haptics.bluetooth-audio"]')?.dataset.uiOn!=='true';const show=gated&&this.a.state.connected&&this.a.state.transport==='Bluetooth';if(this.btNote.hidden===show)this.btNote.hidden=!show;}
    syncModes(){
      const os=this.a.state.platformOS,info=this.info;
      const want=[['system','System audio',true,false],['process','Selected app',os==='windows'||!!info?.processCapture,!!info&&!info.processCapture],['nativeEndpoint','Four-channel relay · prototype',os!=='macos',false]].filter(item=>item[2]);
      const key=want.map(item=>item[0]+item[3]).join('|');
      if(key!==this.modeKey){const current=this.mode.value;this.modeKey=key;this.mode.replaceChildren();for(const [value,label,,disabled] of want){const option=new Option(label,value);option.disabled=disabled;this.mode.add(option);}
        this.mode.value=[...this.mode.options].some(o=>o.value===current&&!o.disabled)?current:'system';this.renderSelection();}
    }
    renderSelection(){this.processField.hidden=this.mode.value!=='process';this.reattachField.hidden=this.mode.value!=='process';this.endpointField.hidden=this.mode.value!=='nativeEndpoint';$('native-relay-note').hidden=this.mode.value!=='nativeEndpoint';}
    async providerCommand(operation,confirmed=false){this.providerBusy=true;this.providerRequest=((this.providerRequest||0)+1)>>>0;this.renderProvider();this.providerDetails.textContent='Waiting for the native bridge…';if(!await this.a.send({action:'nativeProviderControl',operation,controllerId:this.controllerId||this.providerOwner||null,requestId:this.providerRequest,confirmed})){this.providerBusy=false;this.providerDetails.textContent='Bridge request failed. Inspect before retrying.';this.renderProvider();}}
    renderProvider(){const found=!!this.info?.nativeBridge?.toolFound,busy=!!this.providerBusy;this.providerInspect.disabled=!found||busy;this.providerAttach.disabled=!found||busy||!!this.providerOwned||!this.a.state.connected||this.a.state.transport!=='Bluetooth';this.providerDetach.disabled=!found||busy||!this.providerOwned;this.providerRecover.disabled=busy||!this.providerOwned;this.handoffButton.disabled=!!this.providerOwned||(!this.a.state.connected&&!this.handoff);const P=window.controllerPlatform;P.why(this.providerInspect,this.providerInspect.disabled&&!found?'Needs the vDS control tool to be installed.':null);P.why(this.providerAttach,this.providerAttach.disabled&&!busy&&!this.providerOwned?(!found?'Needs the vDS control tool to be installed.':!this.a.state.connected?'Needs a connected controller over Bluetooth.':'Needs the controller to be connected by Bluetooth.'):null);P.why(this.handoffButton,this.handoffButton.disabled&&!this.providerOwned?'Needs a connected controller to release.':null);}
    selectedSource(){
      if(this.mode.value==='process'){const pid=Number(this.process.value);if(!Number.isInteger(pid)||pid<=4)throw Error('Choose a running app.');return{kind:'process',pid};}
      if(this.mode.value==='nativeEndpoint'){if(!this.endpoint.value)throw Error('Choose a compatible virtual source endpoint.');return{kind:'nativeEndpoint',id:this.endpoint.value};}
      return{kind:'system'};
    }
    commandSource(){return copy(this.a.state.audioBridge&&this.activeSource?this.activeSource:this.selectedSource());}
    async refresh(){
      const id=++this.refreshId;$('capture-refresh').disabled=true;
      try{
        if(typeof this.a.invoke!=='function'){this.captureStatus.textContent=window.controllerPlatform.desktopOnlyMessage('Source discovery');return;}
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
        this.syncModes();
        const savedSource=this.savedSource();
        if(savedSource&&!this.restoredSource){this.restoredSource=true;const match=[...this.process.options].find(o=>o.value&&o.textContent.startsWith(savedSource.processName+' '));if(match&&savedSource.mode==='process'&&!this.mode.querySelector('[value=process]')?.disabled){this.mode.value='process';this.process.value=match.value;this.renderSelection();}else if(savedSource.mode==='system'||savedSource.mode==='nativeEndpoint'&&[...this.mode.options].some(o=>o.value===savedSource.mode)){this.mode.value=savedSource.mode;this.renderSelection();}}
        if(!this.accessChecked)this.accessStatus.textContent=info.environment?.message||'Session information unavailable.';
        if(!this.a.state.audioBridge)this.captureStatus.textContent=info.processCapture?'Choose the app to capture; PID selections are not saved in profiles.':this.a.state.platformOS==='windows'?'Process capture needs Windows build 20348 or newer. System audio remains available.':'System audio captures the selected desktop output. Microphone input is not used.';
      }catch(error){if(id===this.refreshId)this.captureStatus.textContent=`Source discovery failed: ${error}`;}
      finally{if(id===this.refreshId)$('capture-refresh').disabled=false;}
    }
    savedSource(){try{const v=JSON.parse(localStorage.getItem('controller.captureSource'));return v&&typeof v==='object'?v:null;}catch{return null;}}
    async capture(){
      if(!this.a.state.connected)return this.a.toast('Connect the controller by USB.');
      try{const source=this.selectedSource();
        try{localStorage.setItem('controller.captureSource',JSON.stringify({mode:this.mode.value,processName:this.mode.value==='process'?(this.process.selectedOptions[0]?.textContent||'').split(' · PID')[0]:''}));}catch{}this.pendingCapture=true;this.captureStatus.textContent='Starting selected capture…';
        if(source?.kind==='process')await this.a.send({action:'setProcessReattach',enabled:this.reattach.checked});
        if(!await this.a.send({action:'setAudioBridge',enabled:true,gain:+$('bridge-gain').value/100,preset:$('bridge-preset').value,source})){this.pendingCapture=false;this.captureStatus.textContent='Capture request failed.';}
      }catch(error){this.captureStatus.textContent=String(error);}
    }
    async testAccess(){
      this.accessStatus.textContent='Opening desktop backend…';$('runtime-access-test').disabled=true;
      if(!await this.a.send({action:'checkDesktopInput'})){this.accessStatus.textContent='Could not contact the controller core.';$('runtime-access-test').disabled=false;}
    }
    async toggleHandoff(){
      if(!this.handoff&&!this.a.state.connected)return this.a.toast('Connect a controller first.');
      if(!this.handoff)await window.dailyUse.stopAll();
      await this.a.send({action:'setNativeHandoff',enabled:!this.handoff});
    }
    async testLink(){
      const strength=Number(this.strength.value);if(!Number.isInteger(strength)||strength<1||strength>25)return this.a.toast('Choose 1–25%.');
      if(!this.a.state.connected)return this.a.toast('Connect a controller first.');
      this.startingLink=true;this.runButton.disabled=true;this.testStatus.textContent='Starting bounded link test…';
      if(!await this.a.send({action:'startLinkExperiment',strength})){this.startingLink=false;this.testStatus.textContent='Test request failed.';this.renderPlatform();}
    }
    async inspect(){if(typeof this.a.invoke!=='function'){this.packetStatus.textContent=window.controllerPlatform.desktopOnlyMessage('Packet inspection');return;}try{const result=await this.a.invoke('inspect_native_report',{reportBase64:this.packet.value.trim()});if(!result)return;this.packetStatus.textContent=`Preview only · ${result.usbBytes} USB bytes → ${result.bluetoothBytes} Bluetooth bytes. L2 mode ${result.leftTriggerMode}, R2 mode ${result.rightTriggerMode}. No hardware write.`;}catch(error){this.packetStatus.textContent=String(error);}}
    event(e){
      if(e.type==='device'){
        this.controllerId=e.connected?e.controllerId:null;this.btEnabled=!!e.bluetoothAudio;this.btPending=false;
        if(e.connected){this.handoff=false;this.handoffButton.textContent='Release controller to game';const d=e.dongle;$('dongle-status').textContent=d?.compatible?`DS5Dongle-compatible interface · ${d.firmware}. The app uses its USB route; audio still needs an available four-channel endpoint. Firmware behavior is unverified here.`:'No compatible dongle interface identified. A USB identity alone cannot distinguish a cable from a bridge.';}
        else{if(this.running||this.startingLink)this.testStatus.textContent='Link test stopped: controller disconnected.';this.startingLink=false;this.running=false;this.activeSource=null;this.pendingCapture=false;this.captureStatus.textContent='Capture stopped; no controller connection owned by the app.';}
        queueMicrotask(()=>this.renderPlatform());
      }
      if(e.type==='nativeProviderStatus'){this.providerOwned=!!e.owned;this.providerBusy=!!e.busy;this.providerOwner=e.owner||null;this.providerDetails.textContent=e.error|| (e.busy?'Waiting for the native bridge…':e.owned?(e.verified&&e.data?.ownedConnected?'Bridge connected. App input and effects are paused.':e.verified&&e.data?.ownedRegistered?'Bridge registered; waiting for Bluetooth. App monitoring is paused.':'Monitoring is paused for saved bridge ownership. Inspect or detach to resume.'):e.data?`Bridge service responded. ${e.data.connected} connected · ${e.data.registered} registered · ${e.data.onlineTargets} online targets.`:'No app-owned bridge session.');this.renderProvider();}
      if(e.type==='bluetoothAudioStatus'&&e.controllerId===this.controllerId){this.btEnabled=!!e.enabled;this.btPending=false;this.btStatus.textContent=e.error|| (e.enabled?'On for this connection. Choose Grips only in the player. Bluetooth speaker playback is unavailable.':'Bluetooth grip audio off.');this.renderPlatform();}
      if(e.type==='safetyReset'){this.btEnabled=false;this.btPending=false;this.renderPlatform();}
      if(e.type==='nativeHandoff'){this.handoff=!!e.enabled;this.handoffButton.textContent=this.handoff?'Resume monitoring':'Release controller to game';this.nativeStatus.textContent=this.handoff?'Released. The native game owns input/audio feedback; app monitoring and output are paused.':'Monitoring resumed. Native game feedback may compete with app effects.';this.renderProvider();}
      if(e.type==='audioStatus'&&typeof e.bridge==='boolean'){this.activeSource=e.bridge?e.captureSource||this.activeSource||{kind:'system'}:null;this.pendingCapture=false;this.captureStatus.textContent=e.bridge?`Active capture: ${this.activeSource.kind==='process'?'process tree '+this.activeSource.pid:this.activeSource.kind==='nativeEndpoint'?'four-channel relay':'system audio'}.`:e.message||'Capture stopped.';if(e.captureError)this.captureStatus.textContent=`${e.message}. ${e.bridge?'Previous capture remains active.':'Capture is off.'}`;}
      if(e.type==='audioMeters'&&typeof e.processReattach==='boolean'&&this.reattach.checked!==e.processReattach&&document.activeElement!==this.reattach)this.reattach.checked=e.processReattach;
      if(e.type==='audioMeters'&&e.captureSource){this.activeSource=e.captureSource;if(e.captureState==='starting')this.captureStatus.textContent='Starting selected app capture; waiting for the Windows audio client.';else if(e.captureState==='reattaching')this.captureStatus.textContent='Captured app ended; waiting up to 30 seconds for it to restart.';else if(e.captureState==='reconnecting')this.captureStatus.textContent='Audio device changed; reconnecting capture.';else if(e.captureState==='running')this.captureStatus.textContent=`Capturing ${e.captureSource.kind==='process'?'process tree '+e.captureSource.pid:e.captureSource.kind==='nativeEndpoint'?'native haptic channels 3/4':'system audio'}.`;}
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
      if(e.type==='core'&&e.status==='ready'){this.handoff=false;this.handoffButton.textContent='Release controller to game';this.renderProvider();}
    }
  }
  window.ControllerIntegrations=ControllerIntegrations;
})();
