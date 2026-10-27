(() => {
  'use strict';
  const $ = (s, root = document) => root.querySelector(s);
  const $$ = (s, root = document) => [...root.querySelectorAll(s)];
  const clamp = (n, lo = 0, hi = 1) => Math.max(lo, Math.min(hi, n));
  const clone = obj => JSON.parse(JSON.stringify(obj));
  const native = window.__TAURI__?.core && window.__TAURI__?.event;
  const invoke = native ? window.__TAURI__.core.invoke : null;
  const names = ['CROSS', 'CIRCLE', 'SQUARE', 'TRIANGLE', 'L1', 'R1', 'L2', 'R2', 'CREATE', 'OPTIONS', 'L3', 'R3', 'UP', 'DOWN', 'LEFT', 'RIGHT', 'PS', 'TOUCHPAD', 'MIC', 'EDGE FN1', 'EDGE FN2', 'LEFT PADDLE', 'RIGHT PADDLE'];
  const vkChoices = [
    {vk: 0x11, label: 'Ctrl'}, {vk: 0x10, label: 'Shift'}, {vk: 0x12, label: 'Alt'},
    {vk: 0x20, label: 'Space'}, {vk: 0x0D, label: 'Enter'}, {vk: 0x1B, label: 'Esc'}, {vk: 0x09, label: 'Tab'},
    {vk: 0x25, label: 'Left'}, {vk: 0x26, label: 'Up'}, {vk: 0x27, label: 'Right'}, {vk: 0x28, label: 'Down'},
    {vk: 0xAD, label: 'Volume mute'}, {vk: 0xAE, label: 'Volume down'}, {vk: 0xAF, label: 'Volume up'}, {vk: 0xB3, label: 'Play / pause'}, {vk: 0xB0, label: 'Next track'}, {vk: 0xB1, label: 'Previous track'},
    {vk: 0x70, label: 'F1'}, {vk: 0x71, label: 'F2'}, {vk: 0x72, label: 'F3'}, {vk: 0x73, label: 'F4'},
    {vk: 0x41, label: 'A'}, {vk: 0x42, label: 'B'}, {vk: 0x43, label: 'C'}, {vk: 0x45, label: 'E'},
    {vk: 0x51, label: 'Q'}, {vk: 0x52, label: 'R'}, {vk: 0x56, label: 'V'}, {vk: 0x58, label: 'X'}, {vk: 0x5A, label: 'Z'}
  ];
  for(let vk=48;vk<=90;vk++)if((vk<=57||vk>=65)&&!vkChoices.some(k=>k.vk===vk))vkChoices.push({vk,label:String.fromCharCode(vk)});
  for(let i=0;i<24;i++)if(!vkChoices.some(k=>k.vk===112+i))vkChoices.push({vk:112+i,label:`F${i+1}`});
  for(const [vk,label] of [[8,'Backspace'],[20,'Caps Lock'],[33,'Page Up'],[34,'Page Down'],[35,'End'],[36,'Home'],[45,'Insert'],[46,'Delete'],[91,'Super / Command'],[186,';'],[187,'='],[188,','],[189,'-'],[190,'.'],[191,'/'],[192,'`'],[219,'['],[220,'Backslash'],[221,']'],[222,"'"]])vkChoices.push({vk,label});
  const presets = {
    Linear: [0,.1,.2,.3,.4,.5,.6,.7,.8,.9,1],
    Wall: [0,0,.02,.04,.1,.84,.96,1,1,1,1],
    Break: [0,.02,.05,.08,.14,.22,.92,.19,.2,.2,.2],
    Swell: [0,.02,.06,.13,.22,.34,.48,.65,.8,.92,1],
    Pulse: [.04,.21,.67,.16,.74,.22,.77,.27,.78,.31,.8]
  };
  const defaultGyro = () => ({enabled:false,sensitivity:1,yawScale:1,pitchScale:1,invertY:false,activation:'always',bias:[0,0,0],deadzone:90,smoothing:.18});
  const defaultRemap = () => ({enabled:false,bindings:[]});
  const defaultInputEffects = () => ({enabled:false,threshold:.82,strength:55,kind:'click',rate:10,spacingMs:0,kickMs:0,kickStyle:'return',startZone:2,endZone:6,returnStrength:100,hapticStrength:0,hapticFrequency:90,hapticDurationMs:95});
  const inputEffectCommand = (c={}) => ({action:'setInputEffects',...Object.fromEntries(Object.entries(defaultInputEffects()).map(([key,value])=>[key,c[key]??value]))});
  const defaultProfiles = [
    {id:'standard',name:'Standard',description:'L2 stays even. R2 gets firmer as you pull.',tag:'GENERAL',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:40,curve:clone(presets.Linear)},right:{mode:'resistance',strength:55,curve:clone(presets.Swell)}},haptics:{shape:'sine',frequency:90,attack:12,release:220,strength:44,pan:0},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()},
    {id:'rally',name:'Rally',description:'L2 brakes hard. R2 pulses for throttle.',tag:'RACING',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:73,curve:clone(presets.Wall)},right:{mode:'vibration',strength:54,curve:clone(presets.Pulse)}},haptics:{shape:'saw',frequency:65,attack:8,release:240,strength:55,pan:-10},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()},
    {id:'arcade',name:'Arcade',description:'A firm break on both triggers.',tag:'ACTION',matchExe:'',autoApply:true,triggers:{left:{mode:'weapon',strength:57,curve:clone(presets.Break)},right:{mode:'weapon',strength:70,curve:clone(presets.Break)}},haptics:{shape:'square',frequency:180,attack:3,release:110,strength:39,pan:0},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()}
  ];
  const overwatchTemplates = {
    hanzo:{name:'Hanzo · Storm Bow',description:'Heavy R2 draw. Release and Storm Arrows cues need game events.',left:{mode:'off',strength:0,curve:clone(presets.Linear)},right:{mode:'bow',strength:90,forceCap:8,curve:clone(presets.Swell),startZone:0,endZone:9,snapStrength:75,zoneForces:[.12,.18,.25,.34,.44,.56,.69,.83,.94,.94]},haptics:{shape:'sine',frequency:50,attack:80,release:180,strength:45,pan:0},inputEffects:{enabled:false,threshold:.95,strength:45}},
    genji:{name:'Genji · Shuriken',description:'Light R2, pulsing L2. Throw and dash cues need game events.',left:{mode:'vibration',strength:42,curve:clone(presets.Pulse),zoneForces:[0,.28,.48,.48,.48,.48,.48,.48,.48,.48]},right:{mode:'resistance',strength:28,curve:clone(presets.Linear),zoneForces:[0,0,.08,.12,.14,.16,.18,.2,.2,.2]},haptics:{shape:'square',frequency:120,attack:2,release:45,strength:42,pan:0},inputEffects:{enabled:true,threshold:.82,strength:32}},
    soldier:{name:'Soldier: 76 · Pulse Rifle',description:'R2 rifle recoil, L2 rocket break. Shot cues need game events.',left:{mode:'weapon',strength:78,curve:clone(presets.Break),startZone:4,endZone:6,snapStrength:55},right:{mode:'vibration',strength:66,curve:clone(presets.Pulse),zoneForces:[0,.42,.66,.66,.66,.66,.66,.66,.66,.66]},haptics:{shape:'square',frequency:95,attack:3,release:135,strength:65,pan:0},inputEffects:{enabled:true,threshold:.78,strength:48}}
  };
  const textures = [
    {name:'Soft tick',type:'Light',settings:{shape:'sine',frequency:175,attack:3,release:80,strength:30,pan:0}},
    {name:'Heartbeat',type:'Warm',settings:{shape:'sine',frequency:65,attack:20,release:270,strength:62,pan:0}},
    {name:'Gravel',type:'Grain',settings:{shape:'noise',frequency:78,attack:9,release:340,strength:47,pan:-20}},
    {name:'Impact',type:'Sharp',settings:{shape:'saw',frequency:95,attack:3,release:190,strength:70,pan:0}},
    {name:'Bow release',type:'Snap',settings:{shape:'saw',frequency:88,attack:2,release:125,strength:52,pan:0}},
    {name:'Mechanical click',type:'Click',settings:{shape:'square',frequency:185,attack:2,release:55,strength:38,pan:0}},
    {name:'Deep rumble',type:'Low',settings:{shape:'sine',frequency:45,attack:35,release:420,strength:67,pan:0}}
  ];
  const hapticFields = [
    {key:'frequency',label:'FREQUENCY',min:40,max:300,unit:'HZ'},
    {key:'attack',label:'ATTACK',min:2,max:200,unit:'MS'},
    {key:'release',label:'RELEASE',min:25,max:650,unit:'MS'},
    {key:'strength',label:'STRENGTH',min:0,max:100,unit:'%'},
    {key:'pan',label:'LEFT / RIGHT',min:-100,max:100,unit:'%'}
  ];
  const state = {
    connected:false,name:'',transport:'',path:'',battery:null,charging:false,
    batteryInfo:null,buttons:Array(names.length).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],
    profileId:'standard',profiles:defaultProfiles.map(normalizeProfile),side:'left',packet:0,
    mouse:false,gestures:false,gyroAim:false,remapArmed:false,pcMode:false,batteryLight:false,temperatureLight:false,screenLight:false,
    xinput:false,xinputStatus:'disabled',xinputMessage:'ViGEm not connected',
    activity:Array(80).fill(0),lastInputAt:0,rate:0,devices:[],selectedPath:'',
    currentView:'live',playUntil:0,playStart:0,playDuration:0,lastExternalExe:'',
    leaseUntil:0,leaseDuration:0,fullReport:false,manualScan:false,
    autoApply:true,coreStatus:'ready',foregroundExe:'',foregroundTitle:'',foregroundPath:'',lastExternalPath:'',autoMatchId:'',autoSuppressed:false,
    sequenceGaps:0,invalidReports:0,stickTest:null,batteryHistory:[],audioBridge:false,bridgeFrames:0,audioRoute:'off',telemetryOn:false,telemetryLast:0,scriptEnabled:false,inputEffectsActive:false,gyroCalibration:null,lowBatteryAlert:false,timerUntil:0,platformDesktopInput:true
  };
  const pcButtons=[0,1,2,3,4,5,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22];
  const defaultPcBindings=()=>[
    {button:0,vk:13,behavior:'tap',sequence:[]},
    {button:1,vk:27,behavior:'tap',sequence:[]},
    {button:2,vk:32,behavior:'tap',sequence:[]},
    {button:3,vk:9,behavior:'tap',sequence:[]},
    {button:4,vk:16,behavior:'hold',sequence:[]},
    {button:5,vk:17,behavior:'hold',sequence:[]},
    {button:12,vk:38,behavior:'hold',sequence:[]},
    {button:13,vk:40,behavior:'hold',sequence:[]},
    {button:14,vk:37,behavior:'hold',sequence:[]},
    {button:15,vk:39,behavior:'hold',sequence:[]}
  ];
  const validAppPath=path=>typeof path==='string'&&path.length<=1024&&!path.includes('\0')&&(/^[a-zA-Z]:\\.*\.exe$/i.test(path)||path.startsWith('/'));
  const validPcKey=vk=>Number.isInteger(vk)&&vk>=1&&vk<=255;
  let pcBindings=defaultPcBindings();
  let pcClickThreshold=.55;
  let pcPointerSensitivity=.7,pcReverseScroll=true;
  const defaultCalibration=()=>({axisCenters:[0,0,0,0],axisMins:[-1,-1,-1,-1],axisMaxs:[1,1,1,1],triggerRests:[0,0],triggerFulls:[1,1],touchMins:[0,0],touchMaxs:[1,1]});
  let inputCalibration=null,calibrationRun=null;
  const validMotorZones=map=>Array.isArray(map)&&map.length===11&&map[0]===0&&map[10]===1&&Array.from({length:11},(_,i)=>Number.isFinite(map[i])&&(i===0||map[i]>map[i-1])).every(Boolean);
  let motorZones={left:null,right:null},zoneRun=null,zoneRenewal=null;
  try{
    const saved=JSON.parse(localStorage.getItem('controller.motorZones')||'null');
    for(const side of ['left','right'])if(validMotorZones(saved?.[side]))motorZones[side]=saved[side];
  }catch{}
  try{
    const saved=JSON.parse(localStorage.getItem('aftertouch.inputCalibration')||'null');
    if(saved&&['axisCenters','axisMins','axisMaxs','triggerRests','triggerFulls','touchMins','touchMaxs'].every(key=>Array.isArray(saved[key])&&saved[key].every(Number.isFinite)))inputCalibration=saved;
  }catch{}
  try{
    const saved=JSON.parse(localStorage.getItem('aftertouch.pcBindings')||'null');
    if(Array.isArray(saved)&&saved.length<=pcButtons.length&&saved.every(b=>pcButtons.includes(b.button)&&(Number.isInteger(b.vk)&&b.vk>=1&&b.vk<=255||b.behavior==='launch'&&b.vk===0)&&['hold','tap','macro','launch'].includes(b.behavior)&&Array.isArray(b.sequence)&&b.sequence.length<=5&&b.sequence.every(vk=>Number.isInteger(vk)&&vk>=1&&vk<=255)&&(!b.chord||Array.isArray(b.chord)&&b.chord.length<=8&&b.chord.every(vk=>Number.isInteger(vk)&&vk>=1&&vk<=255))&&(!b.launchPath||typeof b.launchPath==='string'&&b.launchPath.length<=1024)))pcBindings=saved;
    const click=Number(localStorage.getItem('aftertouch.pcClickThreshold'));
    if(Number.isFinite(click)&&click>=.05&&click<=.95)pcClickThreshold=click;
    const pointer=Number(localStorage.getItem('aftertouch.pcPointerSensitivity'));
    if(Number.isFinite(pointer)&&pointer>=.4&&pointer<=1.5)pcPointerSensitivity=pointer;
    pcReverseScroll=localStorage.getItem('aftertouch.pcReverseScroll')!=='false';
  }catch{}
  let toastTimeout, audioContext, dragging = null, saveTimeout, saveChain=Promise.resolve(), scanTimeout;
  let pcAudioSketch=false;
  try{pcAudioSketch=localStorage.getItem('aftertouch.pcAudioSketch')==='true';}catch{}
  $('#haptic-pc-audio').checked=pcAudioSketch;
  $('#haptic-pc-audio').addEventListener('change',event=>{
    pcAudioSketch=event.target.checked;
    try{localStorage.setItem('aftertouch.pcAudioSketch',String(pcAudioSketch));}catch{}
  });
  let updateChecking=false, updateInstalling=false, availableVersion='';
  let triggerPresets=[],activeWeaponPreset="";
  const profile = () => state.profiles.find(p => p.id === state.profileId) || state.profiles[0];
  const trigger = () => profile().triggers[state.side];
  const timeLabel = () => new Date().toLocaleTimeString('en-US',{hour12:false});
  const modeLabel = {off:'Off',resistance:'Resistance',weapon:'Weapon',vibration:'Vibration',bow:'Bow · experimental'};

  function normalizeProfile(p) {
    const n = clone(p);
    n.matchExe = n.matchExe || n.match_exe || '';
    delete n.match_exe;
    n.matchTitle = n.matchTitle || '';
    n.matchPath = n.matchPath || '';
    n.autoApply = n.autoApply !== false;
    n.gyroAim = Object.assign(defaultGyro(), n.gyroAim || {});
    n.remap = Object.assign(defaultRemap(), n.remap || {});
    n.inputEffects = Object.assign(defaultInputEffects(), n.inputEffects || {});
    n.actionsEnabled=!!n.actionsEnabled;
    n.actions=Array.isArray(n.actions)?n.actions:[];
    n.gameLink=Object.assign({enabled:false,port:6969,format:'json'},n.gameLink||{});
    n.weaponBindings=Array.isArray(n.weaponBindings)?n.weaponBindings:[];
    for(const side of ['left','right']){
      const t=n.triggers[side];
      t.startZone=Number.isInteger(t.startZone)?t.startZone:(t.mode==='weapon'?5:2);
      t.endZone=Number.isInteger(t.endZone)?t.endZone:(t.mode==='weapon'?6:8);
      t.snapStrength=Number.isFinite(t.snapStrength)?t.snapStrength:50;
      t.forceCap=clamp(Math.round(t.forceCap??6),1,8);
      if(!Array.isArray(t.zoneForces)||t.zoneForces.length!==10)t.zoneForces=Array.from({length:10},(_,i)=>(t.curve[i]+t.curve[i+1])/2);
    }
    n.remap.bindings = Array.isArray(n.remap.bindings) ? n.remap.bindings.map(b=>({button:b.button,vk:b.vk,modifier:b.modifier??null,behavior:b.behavior||'hold',sequence:Array.isArray(b.sequence)?b.sequence:[],chord:Array.isArray(b.chord)?b.chord:[]})) : [];
    return n;
  }

  function toast(message) {
    const node = $('#toast'); node.textContent = String(message);
    node.classList.add('is-visible'); clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => node.classList.remove('is-visible'), 3400);
  }
  async function checkForUpdates(manual=false) {
    if(!native || updateChecking || updateInstalling)return;
    updateChecking=true;
    $('#check-update').disabled=true;
    $('#install-update').disabled=true;
    $('#update-status').textContent='Checking for updates…';
    try {
      const update=await invoke('check_update');
      availableVersion=update?.version||'';
      $('#install-update').hidden=!availableVersion;
      $('#install-update').textContent=availableVersion?`Install ${availableVersion}`:'Install update';
      $('#update-status').textContent=availableVersion
        ?`Version ${availableVersion} is ready. The app will restart during installation.`
        :`Version ${update.currentVersion} is current.`;
      if(availableVersion){toast(`Update ${availableVersion} available`);log(`UPDATE / ${availableVersion} available`);}
      else if(manual)toast('App is up to date.');
    } catch(error) {
      $('#update-status').textContent=String(error).includes('404')
        ?'No published update feed yet.'
        :`Update check unavailable: ${error}`;
      if(manual)toast('Could not check for updates.');
    } finally {
      updateChecking=false;
      $('#check-update').disabled=false;
      $('#install-update').disabled=false;
    }
  }
  $('#check-update').addEventListener('click',()=>checkForUpdates(true));
  $('#install-update').addEventListener('click',async()=>{
    if(!availableVersion||updateChecking||updateInstalling)return;
    if(!await commitProfileDraft(false))return;
    updateInstalling=true;
    $('#check-update').disabled=true;
    $('#install-update').disabled=true;
    $('#update-status').textContent=`Downloading ${availableVersion}…`;
    try {
      await invoke('install_update');
      $('#update-status').textContent='Update installed. Restarting…';
    } catch(error) {
      $('#update-status').textContent=`Update failed: ${error}`;
      toast('Update failed. Check Diagnostics.');
      log(`UPDATE ERROR / ${error}`);
      availableVersion='';
      $('#install-update').hidden=true;
    } finally {
      updateInstalling=false;
      $('#check-update').disabled=false;
      $('#install-update').disabled=false;
    }
  });
  function handleUpdateProgress(progress) {
    if(!updateInstalling)return;
    if(progress.finished){$('#update-status').textContent='Download complete. Installing…';return;}
    if(Number.isFinite(progress.total)&&progress.total>0){
      $('#update-status').textContent=`Downloading ${availableVersion}: ${Math.min(100,Math.round(progress.downloaded/progress.total*100))}%`;
    }
  }
  function log(message) {
    const list = $('#event-log'), item = document.createElement('li');
    const time = document.createElement('time'), detail = document.createElement('span');
    time.textContent = timeLabel(); detail.textContent = message;
    item.append(time, detail); list.prepend(item);
    while (list.children.length > 11) list.lastChild.remove();
  }
  async function send(command) {
    if (!native) { toast('Open the desktop app for hardware.'); return false; }
    try { await invoke('send_command',{command}); return true; }
    catch (error) { toast(String(error)); log(`CORE ERROR / ${error}`); return false; }
  }
  function sendPcBindings(){
    if(state.connected)send({action:'setPcBindings',bindings:pcBindings.filter(b=>b.behavior!=='launch'||validAppPath(b.launchPath||'')),click_threshold:pcClickThreshold,pointer_sensitivity:pcPointerSensitivity,reverse_scroll:pcReverseScroll});
  }
  function sendCalibration(){
    if(state.connected)return send({action:'setCalibration',calibration:inputCalibration||defaultCalibration()});
    return Promise.resolve(false);
  }
  const calibrationStages=[
    {label:'1. Capture rest',instruction:'Release both triggers and leave the sticks centered. Capture when the values settle.',action:'Capture rest'},
    {label:'2. Rotate both sticks',instruction:'Rotate each stick around its full rim until left, right, up, and down are all captured.',action:'Continue to triggers'},
    {label:'3. Press and release both triggers',instruction:'Pull L2 and R2 fully, then release each one. The highest reported byte is recorded on release.',action:'Continue to touchpad'},
    {label:'4. Trace the touchpad',instruction:'Touch each marked corner, then slide along the edges. Each corner turns green when captured.',action:'Save calibration'}
  ];
  function restValues(run){
    const count=run.restSamples.length||1;
    const centres=[0,0,0,0],rests=[0,0];
    run.restSamples.forEach(sample=>{sample.axes.forEach((value,i)=>centres[i]+=value);sample.triggers.forEach((value,i)=>rests[i]+=value);});
    return {centres:centres.map(value=>value/count),rests:rests.map(value=>value/count)};
  }
  function calibrationReady(run){
    if(run.stage===0){
      if(run.restSamples.length<30)return false;
      const {centres,rests}=restValues(run);
      return centres.every(value=>Math.abs(value)<=.3)&&rests.every(value=>value<=.2);
    }
    if(run.stage===1)return run.axisMins.every((value,i)=>value<=run.centers[i]-.5)&&run.axisMaxs.every((value,i)=>value>=run.centers[i]+.5);
    if(run.stage===2)return run.triggerFulls.every((value,i)=>value-run.rests[i]>=.5);
    return run.touchCount>=10&&Object.values(run.touchCorners).every(Boolean)&&run.touchMins.every((value,i)=>run.touchMaxs[i]-value>=.5);
  }
  function calibrationFeedback(run){
    if(run.stage===0){
      const {centres,rests}=restValues(run);
      return `Rest samples ${run.restSamples.length}/30 · stick centers ${centres.map(v=>Math.round(v*100)+'%').join(', ')} · L2/R2 released ${rests.map(v=>Math.round(v*100)+'%').join(' / ')}. ${calibrationReady(run)?'Ready to capture.':'Keep the sticks and triggers at rest.'}`;
    }
    if(run.stage===1){
      if(!run.axisCount)return 'Move both sticks around their full rims to start capturing each direction.';
      const axes=['Left X','Left Y','Right X','Right Y'];
      const missing=axes.filter((_,i)=>run.axisMins[i]>run.centers[i]-.5||run.axisMaxs[i]<run.centers[i]+.5);
      const ranges=axes.map((name,i)=>`${name} ${Math.round(run.axisMins[i]*100)} to ${Math.round(run.axisMaxs[i]*100)}%`).join(' · ');
      return `${ranges}. ${missing.length?`Still waiting for full travel: ${missing.join(', ')}.`:'All four stick directions captured. Continue when ready.'}`;
    }
    if(run.stage===2)return `Peak report bytes: L2 ${Math.round(run.triggerFulls[0]*255)}/255, R2 ${Math.round(run.triggerFulls[1]*255)}/255. ${calibrationReady(run)?'Ready to continue.':'Pull and release both triggers.'}`;
    const x=run.touchMaxs[0]-run.touchMins[0],y=run.touchMaxs[1]-run.touchMins[1];
    const corners=Object.values(run.touchCorners).filter(Boolean).length;
    return `Touch corners ${corners}/4 · horizontal ${Math.max(0,Math.round(x*100))}% · vertical ${Math.max(0,Math.round(y*100))}%. ${calibrationReady(run)?'Ready to save.':'Touch each remaining corner.'}`;
  }
  function renderCalibrationStatus(){
    const run=calibrationRun;
    $('#start-calibration').disabled=!state.connected||!!run;
    $('#next-calibration').hidden=!run;
    $('#next-calibration').disabled=!run||!calibrationReady(run);
    $('#cancel-calibration').disabled=!run;
    $('#reset-calibration').disabled=!state.connected||!!run||!inputCalibration;
    if(run){
      const step=calibrationStages[run.stage];
      $('#calibration-step').textContent=step.label;
      $('#calibration-instruction').textContent=step.instruction;
      $('#next-calibration').textContent=step.action;
      $('#calibration-fill').style.width=`${run.stage*25}%`;
      $('#cal-range-feedback').textContent=calibrationFeedback(run);
      $('#calibration-status').textContent=`Step ${run.stage+1} of 4. You choose when to continue.`;
    }
    $('.calibration-work').dataset.stage=run?String(run.stage):'idle';
  }
  function sampleCalibration(input){
    const axes=input.axes||[0,0,0,0],buttons=input.buttons||Array(23).fill(0),point=input.touch?.find(Boolean);
    const run=calibrationRun;
    $('#cal-stick-left').textContent=`${Math.round(axes[0]*100)} / ${Math.round(axes[1]*100)}`;
    $('#cal-stick-right').textContent=`${Math.round(axes[2]*100)} / ${Math.round(axes[3]*100)}`;
    $('#cal-trigger-l2').textContent=run?`${Math.round(buttons[6]*255)}/255`:`${Math.round(buttons[6]*100)}%`;
    $('#cal-trigger-r2').textContent=run?`${Math.round(buttons[7]*255)}/255`:`${Math.round(buttons[7]*100)}%`;
    for(let side=0;side<2;side++){
      const name=side?'right':'left',base=side*2;
      $(`#cal-${name}-dot`).style.left=`${50+clamp(axes[base],-1,1)*42}%`;
      $(`#cal-${name}-dot`).style.top=`${50+clamp(axes[base+1],-1,1)*42}%`;
      const sweep=$(`#cal-${name}-sweep`);
      sweep.classList.toggle('is-active',!!run&&run.stage>=1&&run.axisCount>0);
      if(run&&run.axisCount){sweep.style.left=`${50+run.axisMins[base]*42}%`;sweep.style.top=`${50+run.axisMins[base+1]*42}%`;sweep.style.width=`${(run.axisMaxs[base]-run.axisMins[base])*42}%`;sweep.style.height=`${(run.axisMaxs[base+1]-run.axisMins[base+1])*42}%`;}
      const triggerName=side?'r2':'l2',value=clamp(buttons[6+side]);
      $(`#cal-${triggerName}-fill`).style.width=`${value*100}%`;
      $(`#cal-${triggerName}-peak`).style.left=`${(run?.triggerFulls[side]||0)*100}%`;
      $(`#cal-${triggerName}-adc`).textContent=`${Math.round(value*255)} / 255`;
    }
    $('#cal-touch-readout').textContent=point?`${Math.round(point.x*100)}% / ${Math.round(point.y*100)}%`:'No contact';
    const dot=$('#cal-touch-dot');dot.classList.toggle('is-active',!!point);
    if(point){dot.style.left=`${clamp(point.x)*100}%`;dot.style.top=`${clamp(point.y)*100}%`;}
    if(!run)return;
    if(run.stage===0){
      run.restSamples.push({axes:[...axes],triggers:[buttons[6],buttons[7]]});
      if(run.restSamples.length>100)run.restSamples.shift();
    }else if(run.stage===1){
      run.axisCount++;
      axes.forEach((value,i)=>{run.axisMins[i]=Math.min(run.axisMins[i],value);run.axisMaxs[i]=Math.max(run.axisMaxs[i],value);});
    }else if(run.stage===2){
      [6,7].forEach((button,i)=>{
        const value=buttons[button];
        if(value>run.rests[i]+.25)run.triggerHeld[i]=true;
        if(run.triggerHeld[i])run.triggerPeaks[i]=Math.max(run.triggerPeaks[i],value);
        if(run.triggerHeld[i]&&value<=run.rests[i]+.08){run.triggerFulls[i]=Math.max(run.triggerFulls[i],run.triggerPeaks[i]);run.triggerHeld[i]=false;run.triggerPeaks[i]=0;}
      });
    }else{
      input.touch.forEach(contact=>{if(contact){run.touchMins[0]=Math.min(run.touchMins[0],contact.x);run.touchMins[1]=Math.min(run.touchMins[1],contact.y);run.touchMaxs[0]=Math.max(run.touchMaxs[0],contact.x);run.touchMaxs[1]=Math.max(run.touchMaxs[1],contact.y);run.touchCount++;if(contact.x<.3&&contact.y<.3)run.touchCorners.tl=true;if(contact.x>.7&&contact.y<.3)run.touchCorners.tr=true;if(contact.x<.3&&contact.y>.7)run.touchCorners.bl=true;if(contact.x>.7&&contact.y>.7)run.touchCorners.br=true;}});
    }
    $$('.cal-corner').forEach(marker=>marker.classList.toggle('is-captured',!!run.touchCorners[marker.dataset.calCorner]));
    renderCalibrationStatus();
  }
  async function finishCalibration(){
    const run=calibrationRun;
    const candidate={axisCenters:run.centers,axisMins:run.axisMins,axisMaxs:run.axisMaxs,triggerRests:run.rests,triggerFulls:run.triggerFulls,touchMins:run.touchMins,touchMaxs:run.touchMaxs};
    if(!await send({action:'setCalibration',calibration:candidate})){
      $('#calibration-status').textContent='Could not apply measured ranges. Check Diagnostics and try saving again.';
      return;
    }
    calibrationRun=null;inputCalibration=candidate;
    try{localStorage.setItem('aftertouch.inputCalibration',JSON.stringify(candidate));}catch{}
    $('#calibration-fill').style.width='100%';
    $('#calibration-step').textContent='Calibration saved';
    $('#calibration-instruction').textContent='Measured ranges now shape live input, pointer control, and trigger percentages.';
    $('#cal-range-feedback').textContent=`Sticks, both triggers, and touchpad ranges captured.`;
    $('#calibration-status').textContent='Custom calibration saved locally and applied to this controller.';
    renderCalibrationStatus();
  }
  $('#start-calibration').addEventListener('click',async()=>{
    if(!state.connected||calibrationRun)return;
    if(!await send({action:'setCalibration',calibration:defaultCalibration()}))return;
    calibrationRun={stage:0,restSamples:[],centers:[0,0,0,0],rests:[0,0],axisMins:[1,1,1,1],axisMaxs:[-1,-1,-1,-1],axisCount:0,triggerFulls:[0,0],triggerPeaks:[0,0],triggerHeld:[false,false],touchMins:[1,1],touchMaxs:[0,0],touchCount:0,touchCorners:{tl:false,tr:false,bl:false,br:false}};
    renderCalibrationStatus();
  });
  $('#next-calibration').addEventListener('click',async()=>{
    const run=calibrationRun;if(!run||!calibrationReady(run))return;
    if(run.stage===0){const values=restValues(run);run.centers=values.centres;run.rests=values.rests;}
    if(run.stage===3){await finishCalibration();return;}
    run.stage++;renderCalibrationStatus();
  });
  $('#cancel-calibration').addEventListener('click',()=>{
    calibrationRun=null;sendCalibration();$('#calibration-step').textContent='Calibration canceled';
    $('#calibration-instruction').textContent='Previous ranges restored. Start again whenever you are ready.';
    $('#cal-range-feedback').textContent='No new ranges were saved.';
    $('#calibration-status').textContent='Previous ranges restored.';renderCalibrationStatus();
  });
  $('#reset-calibration').addEventListener('click',()=>{
    inputCalibration=null;try{localStorage.removeItem('aftertouch.inputCalibration');}catch{}
    sendCalibration();$('#calibration-step').textContent='Default ranges restored';
    $('#calibration-status').textContent='No custom calibration saved.';$('#calibration-fill').style.width='0%';renderCalibrationStatus();
  });
  function savePcBindings(){
    try{
      localStorage.setItem('aftertouch.pcBindings',JSON.stringify(pcBindings));
      localStorage.setItem('aftertouch.pcClickThreshold',String(pcClickThreshold));
      localStorage.setItem('aftertouch.pcPointerSensitivity',String(pcPointerSensitivity));
      localStorage.setItem('aftertouch.pcReverseScroll',String(pcReverseScroll));
    }catch{}
    sendPcBindings();
    $('#pc-bindings-status').textContent=pcBindings.some(b=>b.behavior==='launch'&&!validAppPath(b.launchPath||''))?'Enter the full application path to finish the shortcut.':'Saved locally. Shortcuts take effect in PC control while another window is focused.';
  }
  function renderPcBindings(){
    const host=$('#pc-bindings');host.replaceChildren();
    for(const button of pcButtons){
      const binding=pcBindings.find(item=>item.button===button),row=gameNode('div',{class:'pc-binding'}),label=gameNode('span',{text:names[button]});
      const behavior=gameSelect([['','Unassigned'],['hold','Hold key / shortcut'],['tap','Tap key / shortcut'],['macro','Key sequence'],['launch','Open app']],binding?.behavior||'',value=>{
        pcBindings=pcBindings.filter(item=>item.button!==button);
        if(value)pcBindings.push({button,vk:value==='launch'?0:(binding?.vk||13),behavior:value,sequence:value==='macro'?[32]:[],chord:value==='launch'||value==='macro'?[]:(binding?.chord||[]),launchPath:value==='launch'?(binding?.launchPath||''):undefined});
        savePcBindings();renderPcBindings();
      });behavior.setAttribute('aria-label',`${names[button]} action`);row.append(label,behavior);
      if(binding?.behavior==='launch'){
        const launch=gameNode('input',{type:'text',maxLength:1024,value:binding.launchPath||'',placeholder:'Full application path'});launch.setAttribute('aria-label',`${names[button]} app path`);launch.addEventListener('change',()=>{binding.launchPath=launch.value.trim();savePcBindings();});row.append(launch);
      }else if(binding){
        const keys=gameNode('div',{class:'pc-key-editor'}),key=gameSelect([['','Choose key'],...vkChoices.map(k=>[k.vk,k.label])],binding.vk,value=>{if(!value)return;binding.vk=+value;binding.chord=[];savePcBindings();renderPcBindings();});key.setAttribute('aria-label',`${names[button]} key`);
        if(!vkChoices.some(k=>k.vk===binding.vk))key.add(new Option(`Key ${binding.vk}`,binding.vk));key.value=String(binding.vk);
        keys.append(key);
        if(binding.behavior!=='macro'){
          const record=gameNode('button',{class:'quiet-action pc-record',type:'button',text:binding.chord?.length?binding.chord.map(vk=>vkChoices.find(k=>k.vk===vk)?.label||`Key ${vk}`).join(' + '):'Record shortcut'});
          record.setAttribute('aria-label',`${names[button]} record shortcut`);
          record.addEventListener('click',()=>{
            record.textContent='Press shortcut…';let timer;
            const finish=()=>{window.removeEventListener('keydown',capture,true);clearTimeout(timer);renderPcBindings();};
            const capture=event=>{event.preventDefault();event.stopImmediatePropagation();if(['Control','Shift','Alt','Meta'].includes(event.key))return;
              const vk=event.code.startsWith('Key')?event.code.charCodeAt(3):event.code.startsWith('Digit')?event.code.charCodeAt(5):event.keyCode;
              if(!validPcKey(vk))return;
              const chord=[event.ctrlKey?17:0,event.altKey?18:0,event.shiftKey?16:0,event.metaKey?91:0,vk].filter(Boolean);
              binding.vk=vk;binding.chord=[...new Set(chord)];savePcBindings();finish();
            };
            window.addEventListener('keydown',capture,true);timer=setTimeout(finish,10000);
          });keys.append(record);
        }else{
          const sequence=gameNode('input',{type:'text',maxLength:120,placeholder:'Then: E, Space',value:binding.sequence.map(vk=>vkChoices.find(k=>k.vk===vk)?.label||`Key ${vk}`).join(', ')});sequence.setAttribute('aria-label',`${names[button]} following keys`);
          sequence.addEventListener('change',()=>{const values=sequence.value.split(',').map(v=>v.trim()).filter(Boolean).map(label=>vkChoices.find(k=>k.label.toLowerCase()===label.toLowerCase())?.vk||Number(label.replace(/^Key\s+/i,'')));if(values.length<1||values.length>5||!values.every(validPcKey)){toast('Enter one to five key names or key codes, separated by commas.');return;}binding.sequence=values;savePcBindings();});keys.append(sequence);
        }
        row.append(keys);
      }
      host.append(row);
    }
    $('#pc-click-threshold').value=String(Math.round(pcClickThreshold*100));$('#pc-click-value').textContent=`${Math.round(pcClickThreshold*100)}%`;
    $('#pc-pointer-sensitivity').value=String(Math.round(pcPointerSensitivity*100));$('#pc-pointer-value').textContent=`${Math.round(pcPointerSensitivity*100)}%`;$('#pc-reverse-scroll').checked=pcReverseScroll;
  }
  $('#pc-click-threshold').addEventListener('input',event=>{
    pcClickThreshold=+event.target.value/100;
    $('#pc-click-value').textContent=`${event.target.value}%`;
    savePcBindings();
  });
  $('#pc-reset-bindings').addEventListener('click',()=>{
    pcBindings=defaultPcBindings();pcClickThreshold=.55;pcPointerSensitivity=.7;pcReverseScroll=true;renderPcBindings();savePcBindings();
  });
  $('#pc-pointer-sensitivity').addEventListener('input',event=>{
    pcPointerSensitivity=+event.target.value/100;
    $('#pc-pointer-value').textContent=`${event.target.value}%`;
    savePcBindings();
  });
  $('#pc-reverse-scroll').addEventListener('change',event=>{
    pcReverseScroll=event.target.checked;savePcBindings();
  });
  function reportUiFocus(){
    if(native)invoke('send_command',{command:{action:'setUiFocused',focused:document.hasFocus()&&!document.hidden}}).catch(()=>{});
  }
  window.addEventListener('focus',reportUiFocus);
  window.addEventListener('blur',reportUiFocus);
  document.addEventListener('visibilitychange',reportUiFocus);
  function navigate(view) {
    if (!$('#' + view)?.classList.contains('screen')) return;
    if(zoneRun&&view!=='triggers')stopZoneCalibration();
    clearPadFocus();
    state.currentView = view;
    $$('.screen').forEach(s => { const active = s.id === view; s.hidden = !active; s.classList.toggle('is-current',active); });
    $$('.nav-link').forEach(b => { const active = b.dataset.view === view; b.classList.toggle('is-current',active); active ? b.setAttribute('aria-current','page') : b.removeAttribute('aria-current'); });
    if (location.hash !== '#' + view) history.replaceState(null,'','#' + view);
    window.scrollTo({top:0,behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'});
    if (view === 'triggers') renderCurve();
  }
  $('#nav').addEventListener('click',e => { const b = e.target.closest('[data-view]'); if (b) navigate(b.dataset.view); });
  $$('[data-goto]').forEach(b => b.addEventListener('click',() => navigate(b.dataset.goto)));
  window.addEventListener('hashchange',() => navigate(location.hash.slice(1)));

  const padPages = ['live','triggers','haptics','touch','pc-controls','lights','profiles','calibration','settings','diagnostics'];
  async function loadAppSettings(){
    if(!native){$('#app-settings-status').textContent='These settings work in the installed app.';return;}
    try{const settings=await invoke('get_app_settings');$('#close-to-tray').checked=settings.closeToTray;$('#start-on-login').checked=settings.startOnLogin;$('#app-settings-status').textContent='Settings loaded.';}
    catch(error){$('#app-settings-status').textContent=`Could not load settings: ${error}`;}
  }
  for(const [id,command] of [['close-to-tray','set_close_to_tray'],['start-on-login','set_start_on_login']]){
    $(`#${id}`).addEventListener('change',async event=>{
      const enabled=event.target.checked;event.target.disabled=true;
      try{await invoke(command,{enabled});$('#app-settings-status').textContent='Saved.';}
      catch(error){event.target.checked=!enabled;$('#app-settings-status').textContent=`Could not save: ${error}`;}
      finally{event.target.disabled=false;}
    });
  }
  loadAppSettings();
  if(native)invoke('platform_capabilities').then(cap=>{
    if(!cap)return;
    state.platformDesktopInput=!!cap.desktopInput;
    $('#platform-support').textContent=cap.os==='windows'?'Windows: desktop controls, XInput, and system audio capture.':cap.os==='macos'?'macOS: controller features and desktop input. Allow Accessibility access for pointer and keyboard controls. XInput and system audio capture are Windows only.':'Linux: controller features and desktop input through X11 or the Wayland desktop portal. Pure Wayland may need manual profile selection. XInput and system audio capture are Windows only.';
    if(!cap.desktopInput){renderPcMode();renderGyroControls();}
  }).catch(()=>{});
  let padNavPrev = null, padFocus = null;
  function renderPcMode(){
    const enabled=state.pcMode;
    const button=$('#input-mode-toggle');
    button.disabled=!state.connected||!state.platformDesktopInput;
    button.setAttribute('aria-pressed',String(enabled));
    button.classList.toggle('is-active',enabled);
    $('strong',button).textContent=enabled?'PC control':'Game mode';
    $('#input-mode-help').textContent=!state.platformDesktopInput?'Game mode is available. PC control is still Windows only.':enabled
      ?'Touchpad: pointer. Right stick: scroll. L2/R2: clicks. Edit shortcuts under PC controls.'
      :'Game mode: games receive input. PC control: pad controls this app and the desktop.';
    if(!enabled){padNavPrev=null;clearPadFocus();}
  }
  $('#input-mode-toggle').addEventListener('click',()=>send({action:'setPcMode',enabled:!state.pcMode}));
  function clearPadFocus(){
    if(padFocus)padFocus.classList.remove('pad-focused');
    padFocus=null;
  }
  function padControls(){
    const selector='button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex]:not([tabindex="-1"])';
    return [$('#'+state.currentView),$('.masthead'),$('.session-rail')].flatMap(root=>$$(selector,root))
      .filter(el=>el.getClientRects().length && el.getAttribute('type')!=='hidden').sort((a,b)=>Number(!!a.closest('.preset-library'))-Number(!!b.closest('.preset-library')));
  }
  function focusPadControl(el){
    if(!el)return;
    clearPadFocus();padFocus=el;el.classList.add('pad-focused');
    el.focus({preventScroll:true});
    el.scrollIntoView({block:'nearest',behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'});
  }
  function movePadFocus(direction){
    const controls=padControls();if(!controls.length)return;
    const current=controls.indexOf(padFocus);
    focusPadControl(controls[(current+direction+controls.length)%controls.length]);
  }
  function changePadValue(el,direction){
    if(!el)return false;
    if(el.matches('input[type="range"],input[type="number"]')){
      const min=Number(el.min||0),max=Number(el.max||100),step=Number(el.step||1);
      const value=clamp(Number(el.value)+direction*step,min,max);
      if(value!==Number(el.value)){el.value=String(value);el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));}
      return true;
    }
    if(el.matches('select')){
      const options=[...el.options].filter(option=>!option.disabled);
      const current=options.findIndex(option=>option.value===el.value);
      const next=options[clamp(current+direction,0,options.length-1)];
      if(next&&next.value!==el.value){el.value=next.value;el.dispatchEvent(new Event('change',{bubbles:true}));}
      return true;
    }
    return false;
  }
  function handlePadNavigation(buttons){
    if(!state.pcMode)return;
    if(!Array.isArray(buttons)||buttons.length<18)return;
    if(!document.hasFocus()||document.hidden){padNavPrev=null;return;}
    const down=buttons.map(v=>v>.5);
    if(!padNavPrev){padNavPrev=down;return;}
    const previous=padNavPrev;
    const pressed=index=>down[index]&&!previous[index];
    padNavPrev=down;
    if(pressed(4)||pressed(5)){
      const index=padPages.indexOf(state.currentView);
      navigate(padPages[(index+(pressed(5)?1:-1)+padPages.length)%padPages.length]);
      movePadFocus(1);
    }else if(pressed(1)){
      if(state.currentView!=='live')navigate('live');
      movePadFocus(1);
    }else if(pressed(12)||pressed(13)){
      if(padFocus?.matches('[data-point]')){
        const index=Number(padFocus.dataset.point);
        const side=padFocus.dataset.side;
        profile().triggers[side].zoneForces[index]=Math.round(clamp(profile().triggers[side].zoneForces[index]+(pressed(12)?.02:-.02))*100)/100;
        renderCurve();focusPadControl($(`.trigger-curve-svg[data-curve-side="${side}"] [data-point="${index}"]`));saveProfiles();
      }else movePadFocus(pressed(13)?1:-1);
    }else if(pressed(14)||pressed(15)){
      if(!changePadValue(padFocus,pressed(15)?1:-1))movePadFocus(pressed(15)?1:-1);
    }else if(pressed(0)){
      if(!padFocus)movePadFocus(1);
      else if(padFocus.matches('input[type="text"],input[type="search"],input[type="number"],textarea'))padFocus.focus();
      else padFocus.click();
    }
  }
  document.addEventListener('pointerdown',clearPadFocus);
  window.addEventListener('blur',()=>{padNavPrev=null;clearPadFocus();});
  document.addEventListener('visibilitychange',()=>{if(document.hidden){padNavPrev=null;clearPadFocus();}});

  function validProfile(p) {
    const g=p?.gyroAim || defaultGyro(), r=p?.remap || defaultRemap(),c=p?.inputEffects || defaultInputEffects();
    const bindings=Array.isArray(r.bindings)?r.bindings:[];
    return p && typeof p.id==='string' && p.id.length>0 && p.id.length<=80
      && typeof p.name === 'string' && p.name.trim().length>0 && p.name.length<=40
      && typeof p.description==='string' && p.description.length<=180 && typeof p.tag==='string' && p.tag.length<=40
      && (!p.matchExe || (typeof p.matchExe==='string' && p.matchExe.length<=80 && !/[\\/]/.test(p.matchExe)))
      && (!p.match_exe || (typeof p.match_exe==='string' && p.match_exe.length<=80 && !/[\\/]/.test(p.match_exe)))
      && (!p.matchTitle || (typeof p.matchTitle==='string' && p.matchTitle.length<=120 && !p.matchTitle.includes('\0')))
      && (!p.matchPath || (typeof p.matchPath==='string' && p.matchPath.length<=1024 && !p.matchPath.includes('\0')))
      && (!(p.matchTitle||p.matchPath) || !!(p.matchExe||p.match_exe)) && p.triggers && ['left','right'].every(side => {
      const t = p.triggers[side];
      return t && modeLabel[t.mode] && Number.isFinite(t.strength) && t.strength>=0 && t.strength<=100 && Array.isArray(t.curve) && t.curve.length===11 && t.curve.every(n=>Number.isFinite(n)&&n>=0&&n<=1)
        && (t.zoneForces==null || Array.isArray(t.zoneForces)&&t.zoneForces.length===10&&t.zoneForces.every(n=>Number.isFinite(n)&&n>=0&&n<=1))
        && (t.startZone==null || Number.isInteger(t.startZone)&&t.startZone>=0&&t.startZone<=7)
        && (t.endZone==null || Number.isInteger(t.endZone)&&t.endZone>=1&&t.endZone<=9)
        && (t.startZone==null||t.endZone==null||t.endZone>t.startZone)
        && (t.snapStrength==null||Number.isFinite(t.snapStrength)&&t.snapStrength>=0&&t.snapStrength<=100)
        && (t.preloadStrength==null||Number.isFinite(t.preloadStrength)&&t.preloadStrength>=0&&t.preloadStrength<=100)
        && (t.frequencyHz==null||Number.isInteger(t.frequencyHz)&&t.frequencyHz>=1&&t.frequencyHz<=255);
    }) && p.haptics && ['sine','square','saw','noise'].includes(p.haptics.shape)
      && hapticFields.every(f=>Number.isInteger(p.haptics[f.key]) && p.haptics[f.key]>=f.min && p.haptics[f.key]<=f.max)
      && Number.isFinite(g.sensitivity) && g.sensitivity>=.05 && g.sensitivity<=8
      && Number.isFinite(g.yawScale) && g.yawScale>=0 && g.yawScale<=4
      && Number.isFinite(g.pitchScale) && g.pitchScale>=0 && g.pitchScale<=4
      && ['always','l1','r1','l2','r2','touchpad'].includes(g.activation)
      && (!g.bias || (Array.isArray(g.bias)&&g.bias.length===3&&g.bias.every(n=>Number.isInteger(n)&&Math.abs(n)<=4096)))
      && Number.isInteger(g.deadzone??90) && (g.deadzone??90)>=0 && (g.deadzone??90)<=512
      && Number.isFinite(g.smoothing??.18) && (g.smoothing??.18)>=0 && (g.smoothing??.18)<=.85
      && Number.isFinite(c.threshold) && c.threshold>=.2 && c.threshold<=.95
      && Number.isInteger(c.strength) && c.strength>=0 && c.strength<=100
      && ['click','pistol','sniper','burst','smg'].includes(c.kind||'click')
      && Number.isInteger(c.rate??10) && (c.rate??10)>=3 && (c.rate??10)<=20
      && Number.isInteger(c.spacingMs??0) && ((c.spacingMs??0)===0 || c.spacingMs>=50&&c.spacingMs<=1000)
      && ['return','strike','stop'].includes(c.kickStyle||'return')
      && Number.isInteger(c.startZone??2) && (c.startZone??2)>=0 && (c.startZone??2)<=9
      && Number.isInteger(c.endZone??6) && (c.endZone??6)>=0 && (c.endZone??6)<=9
      && ((c.kickStyle||'return')!=='return' || (c.endZone??6)>(c.startZone??2) && (c.endZone??6)<=8)
      && Number.isInteger(c.returnStrength??100) && (c.returnStrength??100)>=1 && (c.returnStrength??100)<=100
      && Number.isInteger(c.hapticStrength??0) && (c.hapticStrength??0)>=0 && (c.hapticStrength??0)<=100
      && Number.isInteger(c.hapticFrequency??90) && (c.hapticFrequency??90)>=20 && (c.hapticFrequency??90)<=250
      && Number.isInteger(c.hapticDurationMs??95) && (c.hapticDurationMs??95)>=20 && (c.hapticDurationMs??95)<=250
      && Number.isInteger(c.kickMs??0) && ((c.kickMs??0)===0 || c.kickMs>=20&&c.kickMs<=250)
      && (c.kind!=='burst'||c.kickStyle==='strike'||!c.kickMs||c.kickMs+20<=(c.spacingMs||Math.floor(1000/(c.rate??10))))
      && bindings.length<=46 && bindings.every(b=>Number.isInteger(b.button)&&b.button>=0&&b.button<23&&Number.isInteger(b.vk)&&b.vk>=1&&b.vk<=255
        && (b.modifier==null||(Number.isInteger(b.modifier)&&b.modifier>=0&&b.modifier<23&&b.modifier!==b.button))
        && ['hold','tap','macro'].includes(b.behavior||'hold') && Array.isArray(b.sequence||[])
        && (b.behavior==='macro'?(b.sequence?.length>=1&&b.sequence.length<=5):!(b.sequence?.length))
        && (b.sequence||[]).every(vk=>Number.isInteger(vk)&&vk>=1&&vk<=255))
      && new Set(bindings.map(b=>`${b.button}:${b.modifier??'base'}`)).size===bindings.length;
  }
  async function loadProfiles() {
    try {
      const data = native ? await invoke('load_profiles') : JSON.parse(localStorage.getItem('aftertouch.profiles.v3') || localStorage.getItem('aftertouch.profiles.v2') || 'null');
      if (data && [1,2,3].includes(data.schemaVersion) && Array.isArray(data.profiles) && data.profiles.length && data.profiles.every(validProfile)) {
        state.profiles = data.profiles.map(normalizeProfile);
        triggerPresets=Array.isArray(data.triggerPresets)?data.triggerPresets:[];
        state.profileId = state.profiles.some(p=>p.id===data.activeId) ? data.activeId : state.profiles[0].id;
        state.autoApply = data.autoApply !== false;
      }
    } catch (error) { log(`PROFILE LOAD / ${error}`); }
    renderAutoApply();
    renderAllProfileControls();
  }
  function persistProfiles() {
    const data={schemaVersion:3,activeId:state.profileId,autoApply:state.autoApply,profiles:clone(state.profiles),triggerPresets:clone(triggerPresets)};
    saveChain=saveChain.catch(()=>{}).then(async()=>{
      if(native) await invoke('save_profiles',{data});
      else localStorage.setItem('aftertouch.profiles.v3',JSON.stringify(data));
    });
    return saveChain.then(()=>true).catch(error=>{toast(`Profile save failed: ${error}`);log(`PROFILE SAVE / ${error}`);return false;});
  }
  function saveProfiles(immediate=false) {
    clearTimeout(saveTimeout);
    if(immediate)return persistProfiles();
    saveTimeout=setTimeout(persistProfiles,180);
  }
  function gameNode(tag,props={},children=[]){const n=document.createElement(tag);for(const [key,value] of Object.entries(props))if(key==='text')n.textContent=value;else if(key==='class')n.className=value;else n[key]=value;for(const child of children)n.append(child);return n;}
  function gameSelect(items,value,onchange){const n=gameNode('select');items.forEach(([v,label])=>n.add(new Option(label,String(v))));n.value=String(value);n.addEventListener('change',()=>onchange(n.value));return n;}
  function gameField(label,input){return gameNode('label',{class:'game-field',text:label},[input]);}
  function gameNumber(value,min,max,change,step=1){const n=gameNode('input',{type:'number',min,max,step,value});n.addEventListener('change',()=>{const value=Number(n.value);if(!Number.isFinite(value)||value<min||value>max){n.value=String(min);toast(`Enter ${min}–${max}`);return;}change(value);saveProfiles();});return n;}
  function libraryOptions(){return [['','No trigger change'],...triggerPresets.map(p=>[p.id,p.name])];}
  function renderTriggerLibrary(){const select=$('#trigger-library'),old=select.value;select.replaceChildren(new Option(triggerPresets.length?'Choose a preset':'No saved presets',''));triggerPresets.forEach(p=>select.add(new Option(p.name,p.id)));select.value=triggerPresets.some(p=>p.id===old)?old:'';for(const id of ['load-trigger-preset','delete-trigger-preset','update-trigger-preset'])$('#'+id).disabled=!select.value;}
  $('#trigger-library').addEventListener('change',()=>{const preset=triggerPresets.find(p=>p.id===$('#trigger-library').value);if(preset)$('#trigger-preset-name').value=preset.name;renderTriggerLibrary();});
  $('#save-trigger-preset').addEventListener('click',async()=>{
    const name=$('#trigger-preset-name').value.trim();if(!name){toast('Name your trigger preset.');return;}if(triggerPresets.length>=100){toast('100 preset limit.');return;}
    const preset={id:`trigger-${crypto.randomUUID()}`,name,triggers:clone(profile().triggers),inputEffects:clone(profile().inputEffects)};
    triggerPresets.push(preset);if(!await saveProfiles(true)){triggerPresets.pop();return;}renderGameProfiles();$('#trigger-library').value=preset.id;renderTriggerLibrary();toast('Trigger preset saved');
  });
  $('#update-trigger-preset').addEventListener('click',async()=>{const p=triggerPresets.find(p=>p.id===$('#trigger-library').value);if(!p)return;const previous=clone(p);Object.assign(p,{name:$('#trigger-preset-name').value.trim()||p.name,triggers:clone(profile().triggers),inputEffects:clone(profile().inputEffects)});if(!await saveProfiles(true))Object.assign(p,previous);renderGameProfiles();});
  $('#load-trigger-preset').addEventListener('click',()=>{const p=triggerPresets.find(p=>p.id===$('#trigger-library').value);if(!p)return;profile().triggers=clone(p.triggers);profile().inputEffects=clone(p.inputEffects);renderTriggerControls();saveProfiles();toast(`Loaded ${p.name}. Press Apply to use it.`);});
  $('#delete-trigger-preset').addEventListener('click',async()=>{const id=$('#trigger-library').value;if(!id)return;const used=state.profiles.some(p=>p.weaponBindings.some(b=>b.presetId===id));if(used){toast('Remove this preset’s weapon mappings before deleting it.');return;}const previous=triggerPresets.slice();triggerPresets=triggerPresets.filter(p=>p.id!==id);if(!await saveProfiles(true))triggerPresets=previous;renderGameProfiles();});
  async function applyGameRules(){
    const p=profile(),active=!state.pcMode&&(!p.matchExe||matchesForeground(p));
    const a=await send({action:'setGameActions',enabled:p.actionsEnabled&&active,rules:clone(p.actions)});
    const b=await send({action:'setGameLink',settings:{...p.gameLink,enabled:p.gameLink.enabled&&active}});
    return a&&b;
  }
  async function useWeapon(name,fromLink=false){
    const p=profile();if(fromLink&&(!p.gameLink.enabled||p.matchExe&&!matchesForeground(p)))return;
    const match=p.weaponBindings.find(b=>b.weapon.toLowerCase()===name.toLowerCase()),preset=triggerPresets.find(t=>t.id===match?.presetId);
    if(!state.connected){toast('Connect a controller.');return;}
    const id=preset?.id||'';if(fromLink&&activeWeaponPreset===id)return;
    if(await send({action:'applyTriggers',...(preset?.triggers||p.triggers)})){
      await send(inputEffectCommand(preset?.inputEffects||p.inputEffects));activeWeaponPreset=id;
      $('#game-link-status').textContent=preset?`${name} · ${preset.name}`:name?`${name} · profile default (no match)`:'Profile default';
    }
  }
  function renderGameProfiles(){
    renderTriggerLibrary();const p=profile();$('#game-actions-enabled').checked=p.actionsEnabled;
    $('#game-link-enabled').checked=p.gameLink.enabled;$('#game-link-port').value=p.gameLink.port;$('#game-link-format').value=p.gameLink.format;
    const host=$('#game-actions-list');host.replaceChildren();
    if(!p.actions.length)host.append(gameNode('p',{class:'pc-note',text:'No action rules yet. Add one, then choose a button and feel.'}));
    p.actions.forEach(rule=>{
      const card=gameNode('div',{class:'game-action-card'}),grid=gameNode('div',{class:'game-form-grid'});
      const name=gameNode('input',{value:rule.name,maxLength:40});name.addEventListener('change',()=>{rule.name=name.value.trim()||'Action';saveProfiles();});
      grid.append(gameField('Action name',name),gameField('When',gameSelect([['press','Press'],['double','Double-tap'],['hold','Hold'],['event','Game event']],rule.source,v=>{rule.source=v;saveProfiles();renderGameProfiles();})));
      if(rule.source==='event'){const e=gameNode('input',{value:rule.event,maxLength:80,placeholder:'dash'});e.addEventListener('change',()=>{rule.event=e.value.trim();saveProfiles();});grid.append(gameField('Event name',e));}
      else grid.append(gameField('Button',gameSelect(names.map((name,i)=>[i,name]),rule.button,v=>{rule.button=+v;saveProfiles();})));
      grid.append(gameField('Tap / hold window · ms',gameNumber(rule.windowMs,80,1000,v=>rule.windowMs=v)),gameField('Duration · ms',gameNumber(rule.durationMs,20,650,v=>rule.durationMs=v)),gameField('Cooldown · ms',gameNumber(rule.cooldownMs,0,10000,v=>rule.cooldownMs=v)));
      const h=rule.haptic||{shape:'sine',strength:45,frequency:85,pan:0};
      grid.append(gameField('Grip texture',gameSelect([['','No grip effect'],['sine','Smooth'],['square','Sharp'],['saw','Snap'],['noise','Rough']],rule.haptic?.shape||'',v=>{rule.haptic=v?{...h,shape:v}:null;saveProfiles();renderGameProfiles();})));
      if(rule.haptic)grid.append(gameField('Grip strength · %',gameNumber(h.strength,0,100,v=>h.strength=v)),gameField('Frequency · Hz',gameNumber(h.frequency,20,300,v=>h.frequency=v)),gameField('Left / right · −100 / 100',gameNumber(h.pan,-100,100,v=>h.pan=v)));
      const snapshots=triggerPresets.find(t=>JSON.stringify(t.triggers.left)===JSON.stringify(rule.leftTrigger)&&JSON.stringify(t.triggers.right)===JSON.stringify(rule.rightTrigger));
      const options=libraryOptions();if(!snapshots&&(rule.leftTrigger||rule.rightTrigger))options.push(['snapshot','Saved action trigger settings']);
      grid.append(gameField('Trigger feel',gameSelect(options,snapshots?.id||(rule.leftTrigger||rule.rightTrigger?'snapshot':''),v=>{const t=triggerPresets.find(p=>p.id===v);rule.leftTrigger=t?clone(t.triggers.left):null;rule.rightTrigger=t?clone(t.triggers.right):null;saveProfiles();})));
      const buttons=gameNode('div',{class:'game-buttons'}),test=gameNode('button',{type:'button',class:'quiet-action',text:'Test feel'}),remove=gameNode('button',{type:'button',class:'quiet-action',text:'Remove'});
      test.addEventListener('click',()=>{if(state.connected)send({action:'testGameAction',rule:clone(rule)});else toast('Connect a controller.');});remove.addEventListener('click',()=>{p.actions=p.actions.filter(r=>r.id!==rule.id);saveProfiles();renderGameProfiles();});buttons.append(test,remove);card.append(grid,buttons);host.append(card);
    });
    const weapons=$('#weapon-bindings');weapons.replaceChildren();const manual=$('#manual-weapon');manual.replaceChildren(new Option('Profile default',''));
    p.weaponBindings.forEach((binding,index)=>{const row=gameNode('div',{class:'weapon-binding'}),name=gameNode('input',{value:binding.weapon,placeholder:'Weapon name, e.g. smg',maxLength:80});name.setAttribute('aria-label','Weapon name');name.addEventListener('change',()=>{binding.weapon=name.value.trim();saveProfiles();renderGameProfiles();});
      const preset=gameSelect(triggerPresets.map(t=>[t.id,t.name]),binding.presetId,v=>{binding.presetId=v;saveProfiles();});preset.setAttribute('aria-label','Weapon trigger preset');
      const remove=gameNode('button',{class:'quiet-action',type:'button',text:'Remove'});remove.addEventListener('click',()=>{p.weaponBindings.splice(index,1);saveProfiles();renderGameProfiles();});row.append(name,preset,remove);weapons.append(row);manual.add(new Option(binding.weapon,binding.weapon));});
  }
  $('#game-actions-enabled').addEventListener('change',e=>{profile().actionsEnabled=e.target.checked;saveProfiles();});
  $('#add-game-action').addEventListener('click',()=>{if(profile().actions.length>=24){toast('24 action limit.');return;}profile().actions.push({id:crypto.randomUUID(),name:'New action',source:'press',button:1,event:'dash',windowMs:280,cooldownMs:180,durationMs:120,haptic:{shape:'sine',frequency:85,strength:45,pan:0},leftTrigger:null,rightTrigger:null});saveProfiles();renderGameProfiles();});
  $('#save-game-actions').addEventListener('click',async()=>{if(await saveProfiles(true)&&state.connected){await applyGameRules();toast('Game rules applied');}else if(!state.connected)toast('Saved. Connect a controller to apply.');});
  for(const id of ['game-link-enabled','game-link-format','game-link-port'])$('#'+id).addEventListener('change',()=>{profile().gameLink={enabled:$('#game-link-enabled').checked,format:$('#game-link-format').value,port:Number($('#game-link-port').value)};saveProfiles();});
  $('#add-weapon-binding').addEventListener('click',()=>{if(!triggerPresets.length){toast('Save a trigger preset on Triggers first.');return;}if(profile().weaponBindings.length>=100)return;let name='Weapon',i=1;while(profile().weaponBindings.some(b=>b.weapon===name))name=`Weapon ${++i}`;profile().weaponBindings.push({weapon:name,presetId:triggerPresets[0].id});saveProfiles();renderGameProfiles();});
  $('#apply-manual-weapon').addEventListener('click',()=>useWeapon($('#manual-weapon').value));

  function renderAllProfileControls() { renderProfiles(); renderTriggerControls(); renderHapticControls(); renderRemapEditor(); renderGyroControls(); renderGameProfiles(); }

  async function applyProfileHardware(announce=true) {
    const p = profile();
    if (!state.connected) {
      if (announce) toast('Connect a controller.');
      return false;
    }
    const ok = await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right});
    if (!ok) return false;
    if (!await send(inputEffectCommand(p.inputEffects))) return false;
    activeWeaponPreset='';
    if(!await applyGameRules())return false;
    if (p.gyroAim) {
      if (!await send({
        action:'setGyroAim',
        enabled: !!p.gyroAim.enabled,
        sensitivity: Number(p.gyroAim.sensitivity)||1,
        yawScale: Number(p.gyroAim.yawScale)||1,
        pitchScale: Number(p.gyroAim.pitchScale)||1,
        invertY: !!p.gyroAim.invertY,
        activation: p.gyroAim.activation || 'always',
        bias: p.gyroAim.bias || [0,0,0],
        deadzone: p.gyroAim.deadzone??90,
        smoothing: p.gyroAim.smoothing??.18
      })) return false;
    }
    if (p.remap) {
      if (!await send({
        action:'setRemap',
        enabled: !!p.remap.enabled,
        bindings: (p.remap.bindings||[]).map(b=>({button:+b.button,vk:+b.vk,modifier:b.modifier??null,behavior:b.behavior||'hold',sequence:b.sequence||[],chord:b.chord||[]}))
      })) return false;
    }
    if (announce) { state.autoSuppressed=false; toast(`Sent ${p.name} settings to controller`); log(`PROFILE / ${p.name} sent`); }
    return true;
  }
  function matchesForeground(p){
    if(!p.matchExe || p.matchExe.toLowerCase()!==state.foregroundExe.toLowerCase())return false;
    if(p.matchTitle && !state.foregroundTitle.toLowerCase().includes(p.matchTitle.toLowerCase()))return false;
    const path=s=>s.replaceAll('/','\\').toLowerCase();
    if(p.matchPath && path(p.matchPath)!==path(state.foregroundPath))return false;
    return true;
  }
  setInterval(async () => {
    if (!state.connected || !state.autoApply || state.autoSuppressed || !state.autoMatchId ||
        state.profileId !== state.autoMatchId || !profile().autoApply ||
        !matchesForeground(profile())) return;
    const currentTriggers=triggerPresets.find(p=>p.id===activeWeaponPreset)?.triggers||profile().triggers;
    if (currentTriggers.left.mode==='off' && currentTriggers.right.mode==='off') return;
    if (state.leaseUntil>performance.now()) await send({action:'renewTriggers'});
    else if(activeWeaponPreset){const preset=triggerPresets.find(p=>p.id===activeWeaponPreset);if(preset){await send({action:'applyTriggers',...preset.triggers});await send(inputEffectCommand(preset.inputEffects));}}else await applyProfileHardware(false);
  }, 5000);

  function renderAutoApply() {
    const b = $('#auto-apply-toggle');
    b.classList.toggle('is-active', state.autoApply);
    b.setAttribute('aria-pressed', String(state.autoApply));
    $('strong', b).textContent = state.autoApply ? 'On' : 'Off';
  }
  $('#auto-apply-toggle').addEventListener('click', () => {
    state.autoApply = !state.autoApply;
    renderAutoApply();
    saveProfiles(true);
    toast(state.autoApply ? 'Auto-apply on' : 'Auto-apply off');
  });

  $('#profile-match').insertAdjacentHTML('afterend','<label class="control-label" for="profile-match-title">Window title contains (optional)</label><input id="profile-match-title" type="text" maxlength="120" placeholder="Racing game"><label class="control-label" for="profile-match-path">Exact executable path (optional)</label><input id="profile-match-path" type="text" maxlength="1024" placeholder="C:\\Games\\game.exe" spellcheck="false"><button class="quiet-action learn-process" id="learn-process-path" type="button">Use last app path</button>');
  function renderProfiles() {
    $('#session-profile-name').textContent=profile().name;
    $('#profile-count').textContent = String(state.profiles.length).padStart(2,'0');
    const list=$('#profile-list'); list.replaceChildren();
    state.profiles.forEach((p,i) => {
      const b=document.createElement('button'); b.className='profile-row'+(p.id===state.profileId?' is-active':''); b.type='button'; b.dataset.profile=p.id;
      b.innerHTML=`<span class="number">${String(i+1).padStart(2,'0')}</span><span><strong></strong><small></small></span><span class="arrow">↗</span>`;
      $('strong',b).textContent=p.name; $('small',b).textContent=p.tag||'Custom'; list.append(b);
    });
    $('#profile-name').value=profile().name; $('#profile-description').value=profile().description||''; $('#profile-match').value=profile().matchExe||'';
    $('#profile-match-title').value=profile().matchTitle||'';$('#profile-match-path').value=profile().matchPath||'';
    $('#profile-auto-apply').checked = profile().autoApply !== false;
    $('#remap-enabled').checked = !!profile().remap?.enabled;
  }
  $('#profile-list').addEventListener('click',async e => {
    const b=e.target.closest('[data-profile]'); if(!b)return;
    state.profileId=b.dataset.profile; renderAllProfileControls(); saveProfiles();
    toast(profile().name);
    if (state.connected && state.autoApply && profile().autoApply !== false) await applyProfileHardware(true);
  });
  $('#new-profile').addEventListener('click',() => {
    if(state.profiles.length>=50){toast('50 profile limit.');return;}
    const p=normalizeProfile(clone(profile())); p.id=`profile-${crypto.randomUUID()}`; p.name='Untitled'; p.description='';p.tag='Custom';p.matchExe='';p.matchTitle='';p.matchPath='';state.profiles.unshift(p); state.profileId=p.id;
    renderAllProfileControls(); saveProfiles(); $('#profile-name').focus(); $('#profile-name').select();
  });
  $('#overwatch-templates').addEventListener('click',event=>{
    const button=event.target.closest('[data-hero-template]');if(!button)return;
    if(state.profiles.length>=50){toast('50 profile limit.');return;}
    const spec=overwatchTemplates[button.dataset.heroTemplate];if(!spec)return;
    const p=normalizeProfile(clone(defaultProfiles[0]));
    p.id=`overwatch-${button.dataset.heroTemplate}-${crypto.randomUUID()}`;
    p.name=spec.name;p.description=spec.description;p.tag='OVERWATCH 2';
    p.matchExe='';p.matchTitle='';p.matchPath='';p.autoApply=false;
    p.triggers={left:clone(spec.left),right:clone(spec.right)};
    p.haptics=clone(spec.haptics);p.inputEffects=clone(spec.inputEffects);
    state.profiles.unshift(normalizeProfile(p));state.profileId=p.id;
    renderAllProfileControls();saveProfiles();toast(`${p.name} template added`);
  });
  $('#duplicate-profile').addEventListener('click',() => {
    if(state.profiles.length>=50){toast('50 profile limit.');return;}
    const p=normalizeProfile(clone(profile())); p.id=`profile-${crypto.randomUUID()}`; p.name=(p.name+' copy').slice(0,40); p.tag='Custom';p.matchExe='';p.matchTitle='';p.matchPath='';state.profiles.unshift(p); state.profileId=p.id;
    renderAllProfileControls(); saveProfiles(); toast('Duplicated');
  });
  $('#delete-profile').addEventListener('click',() => {
    if(state.profiles.length===1){toast('Need one profile.');return;}
    state.profiles=state.profiles.filter(p=>p.id!==state.profileId); state.profileId=state.profiles[0].id; renderAllProfileControls();saveProfiles();toast('Deleted');
  });
  async function commitProfileDraft(announce=false) {
    const name=$('#profile-name').value.trim(); if(!name){toast('Enter a name.');return false;}
    const matchExe=$('#profile-match').value.trim().toLowerCase();
    if(matchExe && (!/^[\w. -]+\.exe$/.test(matchExe)||matchExe.length>80)){toast('Use a name like game.exe.');return false;}
    const matchTitle=$('#profile-match-title').value.trim();
    const matchPath=$('#profile-match-path').value.trim();
    if((matchTitle||matchPath)&&!matchExe){toast('Set an executable name before refining the match.');return false;}
    profile().name=name.slice(0,40); profile().description=$('#profile-description').value.trim().slice(0,180); profile().matchExe=matchExe;profile().matchTitle=matchTitle;profile().matchPath=matchPath;
    profile().autoApply = $('#profile-auto-apply').checked;
    profile().remap.enabled = $('#remap-enabled').checked;
    const row=$('.profile-row.is-active strong');if(row)row.textContent=profile().name;
    $('#session-profile-name').textContent=profile().name;
    $('#save-profile').textContent='Save';
    const saved=await saveProfiles(true);
    if(saved&&announce)toast('Saved');
    return saved;
  }
  $('#save-profile').addEventListener('click',()=>commitProfileDraft(true));
  $('#apply-profile-now').addEventListener('click',async()=>{if(await commitProfileDraft(false))await applyProfileHardware(true);});
  $$('#profile-name,#profile-description,#profile-match,#profile-match-title,#profile-match-path').forEach(el=>{
    el.addEventListener('input',()=>{$('#save-profile').textContent='Save •';});
    el.addEventListener('change',()=>commitProfileDraft(false));
  });
  $('#profile-auto-apply').addEventListener('change',()=>commitProfileDraft(false));
  $('#remap-enabled').addEventListener('change',()=>commitProfileDraft(false));
  $('#learn-process').addEventListener('click',()=>{if(!state.lastExternalExe){toast('Focus the game first.');return;}$('#profile-match').value=state.lastExternalExe;commitProfileDraft(true);});
  $('#learn-process-path').addEventListener('click',()=>{if(!state.lastExternalExe||!state.lastExternalPath){toast('Focus the game first.');return;}$('#profile-match').value=state.lastExternalExe;$('#profile-match-path').value=state.lastExternalPath;commitProfileDraft(true);});
  $('#export-profiles').addEventListener('click',async () => {
    if(!await commitProfileDraft(false))return;
    const data={schemaVersion:3,activeId:state.profileId,autoApply:state.autoApply,profiles:state.profiles,triggerPresets};
    if(native) { try { const path=await invoke('export_profiles',{data}); toast(`Exported to ${path}`); } catch(error){toast(String(error));} }
    else { const blob=new Blob([JSON.stringify(data,null,2)],{type:'application/json'}),url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download='CONTROLLER-profiles.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000); }
  });
  $('#import-profiles').addEventListener('click',()=>$('#import-file').click());
  $('#import-file').addEventListener('change',async e => {
    const file=e.target.files[0]; if(!file)return;
    try { if(file.size>1_000_000)throw Error('File is too large'); const data=JSON.parse(await file.text());
      if(![1,2,3].includes(data.schemaVersion)||!Array.isArray(data.profiles)||!data.profiles.length||data.profiles.length>50||!data.profiles.every(validProfile))throw Error('Unsupported or invalid profile file');
      if(state.profiles.length+data.profiles.length>50)throw Error('50 profile limit');
      const incoming=Array.isArray(data.triggerPresets)?data.triggerPresets:[];
      if(triggerPresets.length+incoming.length>100)throw Error('100 trigger preset limit');
      const presetIds=new Map(incoming.map(p=>[p.id,`trigger-${crypto.randomUUID()}`]));
      const importedPresets=incoming.map(p=>({...p,id:presetIds.get(p.id)}));
      const imported=data.profiles.map(p=>{const c=normalizeProfile(p);c.id=`import-${crypto.randomUUID()}`;c.name=c.name.slice(0,40);c.description=String(c.description||'').slice(0,180);c.tag='IMPORTED';c.weaponBindings=c.weaponBindings.map(b=>({...b,presetId:presetIds.get(b.presetId)||b.presetId}));return c;});
      const previousProfiles=state.profiles.slice(),previousId=state.profileId,previousPresets=triggerPresets.slice();
      triggerPresets.push(...importedPresets);
      state.profiles.unshift(...imported);state.profileId=imported[0].id;renderAllProfileControls();
      if(await saveProfiles(true))toast(`Imported ${imported.length}`);
      else{state.profiles=previousProfiles;state.profileId=previousId;triggerPresets=previousPresets;renderAllProfileControls();}
    } catch(error){toast(`Import failed: ${error.message}`);} e.target.value='';
  });

  function renderRemapEditor() {
    const box = $('#remap-bindings'); box.replaceChildren();
    const bindings = profile().remap.bindings;
    if (!bindings.length) {
      box.innerHTML = '<p class="remap-empty">No bindings.</p>';
      return;
    }
    bindings.forEach((binding, index) => {
      const row = document.createElement('div');
      row.className = 'remap-row';
      const btnSelect = document.createElement('select');
      btnSelect.setAttribute('aria-label',`Binding ${index+1} button`);
      names.forEach((name, i) => {
        const opt = document.createElement('option');
        opt.value = String(i); opt.textContent = name;
        if (i === +binding.button) opt.selected = true;
        btnSelect.append(opt);
      });
      const keySelect = document.createElement('select');
      keySelect.setAttribute('aria-label',`Binding ${index+1} key`);
      vkChoices.forEach(choice => {
        const opt = document.createElement('option');
        opt.value = String(choice.vk); opt.textContent = choice.label;
        if (choice.vk === +binding.vk) opt.selected = true;
        keySelect.append(opt);
      });
      const remove = document.createElement('button');
      remove.type = 'button'; remove.textContent = '✕'; remove.className = 'remap-remove';remove.setAttribute('aria-label',`Remove binding ${index+1}`);
      const modifierSelect=document.createElement('select');modifierSelect.className='remap-modifier';modifierSelect.setAttribute('aria-label',`Binding ${index+1} layer button`);
      modifierSelect.add(new Option('Base layer',''));
      names.forEach((name,i)=>{if(i!==+binding.button)modifierSelect.add(new Option(`While ${name}`,String(i)));});
      modifierSelect.value=binding.modifier==null?'':String(binding.modifier);
      const behaviorSelect=document.createElement('select');behaviorSelect.className='remap-behavior';behaviorSelect.setAttribute('aria-label',`Binding ${index+1} action`);
      [['hold','Hold key'],['tap','Tap key'],['macro','Key sequence']].forEach(([value,label])=>behaviorSelect.add(new Option(label,value)));
      behaviorSelect.value=binding.behavior||'hold';
      const macroInput=document.createElement('input');macroInput.className='remap-macro';macroInput.type='text';macroInput.placeholder='Then: E, Space (up to five)';macroInput.maxLength=60;macroInput.setAttribute('aria-label',`Binding ${index+1} following macro keys`);
      macroInput.value=(binding.sequence||[]).map(vk=>vkChoices.find(choice=>choice.vk===vk)?.label||String(vk)).join(', ');
      macroInput.hidden=behaviorSelect.value!=='macro';
      const pairTaken=(button,modifier)=>bindings.some((b,i)=>i!==index&&b.button===button&&(b.modifier??null)===modifier);
      btnSelect.addEventListener('change', () => {
        const next=+btnSelect.value;
        const modifier=binding.modifier===next?null:(binding.modifier??null);
        if (pairTaken(next,modifier)) { toast('That button and layer already have a binding.'); btnSelect.value=String(binding.button); return; }
        binding.button=next;binding.modifier=modifier;renderRemapEditor();saveProfiles();
      });
      keySelect.addEventListener('change', () => { binding.vk = +keySelect.value; saveProfiles(); });
      modifierSelect.addEventListener('change',()=>{
        const next=modifierSelect.value===''?null:+modifierSelect.value;
        if(pairTaken(binding.button,next)){toast('That button and layer already have a binding.');modifierSelect.value=binding.modifier==null?'':String(binding.modifier);return;}
        binding.modifier=next;saveProfiles();
      });
      behaviorSelect.addEventListener('change',()=>{
        binding.behavior=behaviorSelect.value;
        binding.sequence=binding.behavior==='macro'?(binding.sequence?.length?binding.sequence:[0x45]):[];
        renderRemapEditor();saveProfiles();
      });
      macroInput.addEventListener('change',()=>{
        const labels=macroInput.value.split(',').map(part=>part.trim().toLowerCase()).filter(Boolean);
        const keys=labels.map(label=>vkChoices.find(choice=>choice.label.toLowerCase()===label)?.vk);
        if(!labels.length||labels.length>5||keys.some(vk=>!vk)){toast('Enter one to five listed keys, separated by commas.');macroInput.value=(binding.sequence||[]).map(vk=>vkChoices.find(choice=>choice.vk===vk)?.label||String(vk)).join(', ');return;}
        binding.sequence=keys;saveProfiles();
      });
      remove.addEventListener('click', () => { profile().remap.bindings.splice(index, 1); renderRemapEditor(); saveProfiles(); });
      row.append(btnSelect, keySelect, remove, modifierSelect, behaviorSelect, macroInput);
      box.append(row);
    });
  }
  $('#add-remap').addEventListener('click', () => {
    if (profile().remap.bindings.length >= 46) { toast('46 binding limit.'); return; }
    const used=new Set(profile().remap.bindings.map(b=>`${b.button}:${b.modifier??'base'}`));
    let choice=null;
    for(const modifier of [null,4,5,8,9,17,18]){
      const button=names.findIndex((_,i)=>i!==modifier&&!used.has(`${i}:${modifier??'base'}`));
      if(button>=0){choice={button,modifier};break;}
    }
    if(!choice){toast('No free binding pair.');return;}
    profile().remap.bindings.push({...choice,vk:0x20,behavior:'hold',sequence:[]});
    renderRemapEditor(); saveProfiles();
  });

  function renderTriggerControls() {
    const t=trigger();
    $$('.side-tab').forEach(b=>b.classList.toggle('is-active',b.dataset.side===state.side));
    $('#curve-side-label').textContent=state.side==='left'?'L2':'R2';
    $('#trigger-modes').innerHTML=Object.entries(modeLabel).map(([key,label])=>`<button class="mode-button ${t.mode===key?'is-active':''}" data-mode="${key}" type="button"><span>${label}</span><small>${key}</small></button>`).join('');
    $('#trigger-strength').value=t.strength;renderTriggerStrength(t);fillRange($('#trigger-strength'));renderCurve();
    renderBreakControls(t);
    const c=profile().inputEffects||defaultInputEffects();
    $('.input-click-block > h3').textContent='Trigger response';
    $('.input-click-block > p').textContent=profile().triggers.right.mode==='bow'?'Draw tension clears at the high point. Release R2 fully to reset.':c.kind==='burst'?'Hold R2 past the pull point for three kicks. Release fully to rearm.':c.kind==='sniper'?'Full-force firing stop. Push through the break, then release to reset.':'Pull R2 past the selected point for recoil feedback. Release to rearm.';
    $('#click-enabled').checked=!!c.enabled;
    $('#weapon-kind').value=c.kind||'click';
    $('#weapon-rate-value').textContent=`${c.rate??10} shots/s`;
    $('#weapon-rate-gap').textContent=`${Math.round(1000/(c.rate??10))} ms between shots`;
    $('#weapon-rate-row').hidden=c.kind!=='smg';
    $('#burst-timing').hidden=c.kind!=='burst';
    $('#burst-kick-style').value=c.kickStyle||'return';
    $('#burst-style-help').textContent=({return:'Adds a return stroke after the stop. Experimental firmware effect.',strike:`Three uninterrupted motor cycles. ${Math.round(1000/(c.spacingMs||1000/(c.rate??10)))} Hz, ${(1000/Math.round(1000/(c.spacingMs||1000/(c.rate??10)))).toFixed(1)} ms per cycle. The controller uses whole Hz.`,stop:'Briefly applies a firm wall, then clears it.'})[c.kickStyle||'return'];
    $('#burst-kick-control').hidden=c.kickStyle==='strike';
    $('#burst-return-control').hidden=(c.kickStyle||'return')!=='return';
    $('#burst-end-control').hidden=(c.kickStyle||'return')!=='return';
    $('#burst-zone-start').max=(c.kickStyle||'return')==='return'?8:10;
    $('#burst-zone-start').value=(c.startZone??2)+1;$('#burst-zone-end').value=(c.endZone??6)+1;
    $('#burst-zone-end').min=(c.startZone??2)+2;$('#burst-return-strength').value=c.returnStrength??100;
    $('#burst-grip-strength').value=c.hapticStrength??0;$('#burst-grip-frequency').value=c.hapticFrequency??90;$('#burst-grip-duration').value=c.hapticDurationMs??95;
    const spacing=c.spacingMs||Math.round(1000/(c.rate??10));
    $('#burst-spacing').value=spacing;$('#burst-kick').value=c.kickMs||Math.min(95,spacing-20);
    $('#click-threshold-number').value=Math.round(c.threshold*1000)/10;$('#click-strength-number').value=c.strength;
    $('#click-threshold').value=Math.round(c.threshold*1000)/10;$('#click-threshold-value').textContent=`${Math.round(c.threshold*1000)/10}%`;
    $('#click-strength').value=c.strength;$('#click-strength-value').textContent=`${c.strength}%`;
    fillRange($('#click-threshold'));fillRange($('#click-strength'));
  }
  function renderTriggerStrength(t){
    const cap=clamp(t.forceCap??6,1,8);
    $('#trigger-strength-input').value=t.strength;
    $('#trigger-force-step').max=cap;$('#trigger-force-step').value=t.strength?Math.max(1,Math.round(t.strength*cap/100)):0;
    $('#trigger-frequency-control').hidden=t.mode!=='vibration';$('#trigger-frequency').value=t.frequencyHz??45;
    $('#trigger-strength-label').textContent=t.mode==='bow'?'Draw force':'Strength';
    $('#trigger-strength-number').textContent=t.mode==='bow'||t.mode==='weapon'
      ?`${t.strength}% · ${t.strength?Math.max(1,Math.round(t.strength*cap/100)):0} / ${cap}`
      :`${t.strength}%`;
    $('.ceiling-note').textContent=t.mode==='bow'||t.mode==='weapon'
      ?`Force range: ${cap} of 8 motor levels.`
      :'Force points apply to Resistance and Vibration.';
  }
  function renderBreakControls(t) {
    const positioned=t.mode==='weapon'||t.mode==='bow';
    const cap=clamp(t.forceCap??6,1,8),map=motorZones[state.side];
    const measured=validMotorZones(map);
    $('.trigger-break-controls').dataset.mode=t.mode;
    $('#break-side-label').textContent=state.side==='left'?'L2':'R2';
    $('#break-start').min=t.mode==='weapon'?2:0;
    $('#break-end').max=t.mode==='weapon'?8:9;
    t.startZone=clamp(t.startZone??(t.mode==='weapon'?5:2),+$('#break-start').min,7);
    t.endZone=clamp(t.endZone??(t.mode==='weapon'?6:8),t.startZone+1,+$('#break-end').max);
    $('#break-end').min=t.startZone+1;
    const snapLevel=clamp(Math.round((t.snapStrength??50)*cap/100),1,cap);
    $('#break-snap').max=cap;
    $('#break-force-cap').value=String(cap);
    $('#break-force-cap').disabled=t.mode==='off';
    $('#break-start').value=t.startZone;$('#break-end').value=t.endZone;$('#break-snap').value=snapLevel;
    $('#break-start-value').textContent=`Zone ${t.startZone+1}`;
    $('#break-end-value').textContent=`Zone ${t.endZone+1}`;
    $('#break-snap-value').textContent=`${snapLevel} / ${cap}`;
    $$('#break-start,#break-end').forEach(input=>input.disabled=!positioned);
    $('#break-snap').disabled=t.mode!=='bow';
    $('#break-snap-control').hidden=true;
    $('#break-preload-control').hidden=t.mode!=='weapon';
    $('#break-preload').value=t.preloadStrength??0;$('#break-preload-number').value=t.preloadStrength??0;
    $('#break-preload-value').textContent=`${t.preloadStrength??0}%`;fillRange($('#break-preload'));
    for(const key of ['start','end']){const range=$(`#break-${key}`),number=$(`#break-${key}-number`);number.min=+range.min+1;number.max=+range.max+1;number.value=+range.value+1;number.disabled=!positioned;}
    $('label[for="break-end"]').firstChild.textContent=t.mode==='bow'?'Tension clears at ':'Break point ';
    $('#break-title').textContent=t.mode==='bow'?'Bow draw':'Trigger break';
    $('#break-guide').textContent=positioned
      ?measured?`Measured pull: start near ${Math.round(map[t.startZone]*100)}%, break near ${Math.round(map[t.endZone]*100)}%.`:'Motor zones are not equal parts of trigger travel. Measure them below to align the marker.'
      :'Choose Bow or Weapon mode to edit these points.';
    const track=$('#break-preview-track');
    track.innerHTML=Array.from({length:10},(_,i)=>`<span class="${i<t.startZone?'':i<t.endZone?'is-draw':i===t.endZone?'is-break':'is-after'}" aria-hidden="true"></span>`).join('');
    track.classList.toggle('is-uncalibrated',!measured);
    track.style.gridTemplateColumns=measured?Array.from({length:10},(_,i)=>`${(map[i+1]-map[i])*1000}fr`).join(' '):'';
    $('#break-preview').setAttribute('aria-label',measured?'Measured motor zones and live trigger pull':'Motor zone order; positions need measurement');
    for(const button of $$('[data-break-step]')){
      const [key,direction]=button.dataset.breakStep.split(':');
      const input=$(`#break-${key}`);
      button.disabled=!positioned||(key==='snap'&&t.mode!=='bow')||(direction==='-1'?+input.value<=+input.min:+input.value>=+input.max);
    }
    $$('#break-start,#break-end,#break-snap').forEach(fillRange);
    updateBreakPull();
    renderZoneCalibration();
  }
  function renderZoneCalibration(){
    const active=!!zoneRun,side=active?zoneRun.side:state.side,map=motorZones[side];
    $('#zone-cal-start').hidden=active;
    $('#zone-cal-start').disabled=!state.connected;
    $('#zone-cal-start').textContent=validMotorZones(map)?`Measure ${side==='left'?'L2':'R2'} again`:`Measure ${side==='left'?'L2':'R2'} zones`;
    $('#zone-cal-capture').hidden=!active;
    $('#zone-cal-cancel').hidden=!active;
    if(active){
      const pull=Math.round(clamp(state.buttons[side==='left'?6:7]||0)*100);
      $('#zone-cal-capture').textContent=`Capture zone ${zoneRun.zone+1}`;
      $('#zone-cal-status').textContent=zoneRun.message||`Step ${zoneRun.zone} of 9. Release, then pull slowly to the wall. Hold there and capture. Reported pull: ${pull}%. No timer.`;
    }else $('#zone-cal-status').textContent=validMotorZones(map)
      ?`${side==='left'?'L2':'R2'} motor zones measured. The green marker now follows your reported pull.`
      :'Zone widths are schematic. Measure them with a trigger wall for an accurate live marker.';
  }
  async function applyZoneWall(run){
    if(!run||!state.connected)return false;
    const wall={mode:'resistance',strength:100,forceCap:6,curve:Array(11).fill(0),zoneForces:Array.from({length:10},(_,i)=>i>=run.zone?1:0)};
    const p=profile();
    return send({action:'applyTriggers',left:run.side==='left'?wall:p.triggers.left,right:run.side==='right'?wall:p.triggers.right});
  }
  async function stopZoneCalibration(restore=true){
    if(!zoneRun)return;
    clearInterval(zoneRenewal);zoneRenewal=null;zoneRun=null;
    if(restore&&state.connected){
      const p=profile(),c=p.inputEffects||defaultInputEffects();
      await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right});
      await send(inputEffectCommand(c));
    }
    renderBreakControls(trigger());
  }
  $('#zone-cal-start').addEventListener('click',async()=>{
    if(!state.connected||zoneRun)return;
    const run={side:state.side,zone:1,positions:[0],message:''};
    zoneRun=run;renderZoneCalibration();
    await send(inputEffectCommand({enabled:false,strength:0}));
    if(!await applyZoneWall(run)){await stopZoneCalibration();return;}
    zoneRenewal=setInterval(()=>{if(zoneRun===run)applyZoneWall(run);},10000);
    renderZoneCalibration();
  });
  $('#zone-cal-capture').addEventListener('click',async()=>{
    const run=zoneRun;if(!run)return;
    const pull=clamp(state.buttons[run.side==='left'?6:7]||0);
    if(pull<=run.positions[run.zone-1]+.005||pull>=.995){
      run.message='Hold at the new wall before capturing. Release and pull slowly if the reading has not moved.';
      renderZoneCalibration();return;
    }
    run.positions[run.zone]=Math.round(pull*1000)/1000;
    run.message='';
    if(run.zone===9){
      run.positions[10]=1;
      if(!validMotorZones(run.positions)){run.message='The zone positions crossed. Repeat this zone.';renderZoneCalibration();return;}
      motorZones[run.side]=run.positions;
      try{localStorage.setItem('controller.motorZones',JSON.stringify(motorZones));}catch{}
      await stopZoneCalibration();
      toast(`${run.side==='left'?'L2':'R2'} motor zones saved`);
    }else{
      run.zone++;
      await applyZoneWall(run);
      renderZoneCalibration();
    }
  });
  $('#zone-cal-cancel').addEventListener('click',()=>stopZoneCalibration());
  $$('.side-tab').forEach(b=>b.addEventListener('click',async()=>{if(zoneRun)await stopZoneCalibration();state.side=b.dataset.side;renderTriggerControls();}));
  $('#trigger-modes').addEventListener('click',async e=>{const b=e.target.closest('[data-mode]');if(!b)return;if(zoneRun)await stopZoneCalibration();const t=trigger(),was=t.mode;t.mode=b.dataset.mode;if(t.mode==='bow'&&was!=='bow'){t.strength=Math.max(85,t.strength);t.forceCap=8;t.startZone=0;t.endZone=9;t.snapStrength=35;if(state.side==='right')profile().inputEffects.enabled=false;}renderTriggerControls();saveProfiles();});
  $('#break-start').addEventListener('input',event=>{
    const t=trigger();t.startZone=+event.target.value;
    if(t.endZone<=t.startZone)t.endZone=t.startZone+1;
    renderBreakControls(t);saveProfiles();
  });
  $('#break-end').addEventListener('input',event=>{
    const t=trigger();t.endZone=Math.max(t.startZone+1,+event.target.value);
    renderBreakControls(t);saveProfiles();
  });
  $('#break-snap').addEventListener('input',event=>{
    trigger().snapStrength=Math.round(+event.target.value*100/(trigger().forceCap||6));renderBreakControls(trigger());saveProfiles();
  });
  $('#break-force-cap').addEventListener('change',event=>{
    const t=trigger();t.forceCap=+event.target.value;renderTriggerStrength(t);renderBreakControls(t);saveProfiles();
  });
  $('.break-control-grid').addEventListener('click',event=>{
    const button=event.target.closest('[data-break-step]');if(!button||button.disabled)return;
    const [key,direction]=button.dataset.breakStep.split(':');
    const input=$(`#break-${key}`);
    input.value=clamp(+input.value+Number(direction),+input.min,+input.max);
    input.dispatchEvent(new Event('input',{bubbles:true}));
  });
  function fillRange(el){const f=(+el.value-(+el.min||0))/((+el.max||100)-(+el.min||0))*100;el.style.setProperty('--fill',`${f}%`);}
  $('#trigger-strength').addEventListener('input',e=>{trigger().strength=+e.target.value;renderTriggerStrength(trigger());fillRange(e.target);saveProfiles();});
  function bindRangeNumber(rangeId,numberId,offset=0){
    const range=$(rangeId),number=$(numberId);
    number.addEventListener('change',()=>{if(!number.value||!Number.isFinite(number.valueAsNumber)){number.value=+range.value+offset;return;}range.value=clamp((Math.round((number.valueAsNumber-offset)/(+range.step||1))*(+range.step||1)),+range.min,+range.max);range.dispatchEvent(new Event('input',{bubbles:true}));number.value=+range.value+offset;});
    range.addEventListener('input',()=>number.value=+range.value+offset);
  }
  bindRangeNumber('#trigger-strength','#trigger-strength-input');
  bindRangeNumber('#break-start','#break-start-number',1);bindRangeNumber('#break-end','#break-end-number',1);
  $('#trigger-force-step').addEventListener('change',e=>{if(!e.target.value)return;const step=clamp(Math.round(+e.target.value),0,clamp(trigger().forceCap??6,1,8));$('#trigger-strength').value=Math.round(step*100/(clamp(trigger().forceCap??6,1,8)));$('#trigger-strength').dispatchEvent(new Event('input',{bubbles:true}));});
  $('#break-preload').addEventListener('input',e=>{trigger().preloadStrength=+e.target.value;renderBreakControls(trigger());saveProfiles();});
  bindRangeNumber('#break-preload','#break-preload-number');
  $('#trigger-frequency').addEventListener('change',e=>{trigger().frequencyHz=clamp(Math.round(+e.target.value||45),1,255);e.target.value=trigger().frequencyHz;saveProfiles();});
  $('#curve-presets').innerHTML=Object.keys(presets).map(n=>`<button type="button" data-preset="${n}">${n}</button>`).join('');
  $('#curve-presets').addEventListener('click',e=>{const b=e.target.closest('[data-preset]');if(!b||b.disabled)return;trigger().curve=clone(presets[b.dataset.preset]);trigger().zoneForces=Array.from({length:10},(_,i)=>(trigger().curve[i]+trigger().curve[i+1])/2);renderCurve();saveProfiles();toast(`${state.side==='left'?'L2':'R2'} ${b.dataset.preset}`);});
  function renderCurve(){
    for(const side of ['left','right']){
      const setting=profile().triggers[side],svg=$(`.trigger-curve-svg[data-curve-side="${side}"]`),zones=setting.zoneForces,grid=svg.querySelector('.curve-grid');
      const active=setting.mode==='resistance'||setting.mode==='vibration';
      if(!grid.childElementCount){let g='';for(let i=0;i<=10;i++){const x=45+i*67;g+=`<line x1="${x}" y1="18" x2="${x}" y2="205" class="grid-line"/>`;}for(let i=0;i<=5;i++){const y=205-i*35;g+=`<line x1="45" y1="${y}" x2="715" y2="${y}" class="grid-line"/>`;}grid.innerHTML=g;}
      const xy=(v,i)=>`${78.5+i*67},${205-v*175}`;
      svg.querySelector('.curve-line').setAttribute('d',zones.map((v,i)=>(i?'L':'M')+xy(v,i)).join(' '));
      svg.querySelector('.curve-zone-steps').setAttribute('d',zones.map((v,i)=>`M${45+i*67},${205-v*175} L${45+(i+1)*67},${205-v*175}`).join(' '));
      svg.querySelector('.curve-points').innerHTML=zones.map((v,i)=>`<circle data-point="${i}" data-side="${side}" cx="${78.5+i*67}" cy="${205-v*175}" r="8" tabindex="0" aria-label="${side==='left'?'L2':'R2'} hardware zone ${i+1}, force ${Math.round(v*100)} percent"/>`).join('');
      const card=$(`[data-curve-card="${side}"]`);
      card.classList.toggle('is-selected',side===state.side);
      card.classList.toggle('is-mode-inactive',!active);
      card.querySelector('.curve-card-heading span').textContent=active?'Drag the ten force points':setting.mode==='bow'?'Use Bow draw below':setting.mode==='weapon'?'Use Trigger break below':'Trigger effect off';
      svg.querySelectorAll('[data-point]').forEach(point=>point.setAttribute('tabindex',active?'0':'-1'));
    }
    $$('#curve-presets button').forEach(button=>button.disabled=!['resistance','vibration'].includes(trigger().mode));
    updatePlayhead();
  }
  function updatePlayhead(){
    for(const [side,index] of [['left',6],['right',7]]){
      const x=45+(state.buttons[index]||0)*670,line=$(`.trigger-curve-svg[data-curve-side="${side}"] .curve-playhead`);
      line.setAttribute('x1',x);line.setAttribute('x2',x);
    }
    updateBreakPull();
  }
  function updateBreakPull(){
    if($('#burst-live-meter')){const pull=clamp(state.buttons[7]||0);$('#burst-live-meter').value=pull;$('#burst-live-pull').textContent=`R2: ${(pull*100).toFixed(1)}%`;}
    const pull=clamp(state.buttons[state.side==='left'?6:7]||0)*100;
    $('#break-preview-track').style.setProperty('--pull',`${pull}%`);
    $('#break-pull-label').textContent=`Reported pull ${Math.round(pull)}%`;
    if(zoneRun)renderZoneCalibration();
  }
  $$('.trigger-curve-svg').forEach(svg=>{
    svg.addEventListener('pointerdown',e=>{const p=e.target.closest('[data-point]');if(!p||!['resistance','vibration'].includes(profile().triggers[svg.dataset.curveSide].mode))return;state.side=svg.dataset.curveSide;dragging={side:state.side,index:+p.dataset.point};renderTriggerControls();svg.setPointerCapture(e.pointerId);e.preventDefault();});
    svg.addEventListener('pointermove',e=>{if(!dragging||dragging.side!==svg.dataset.curveSide)return;const rect=svg.getBoundingClientRect(),svgY=(e.clientY-rect.top)*250/rect.height;profile().triggers[dragging.side].zoneForces[dragging.index]=Math.round(clamp((205-svgY)/175)*100)/100;renderCurve();});
    const finishCurve=()=>{if(dragging){dragging=null;saveProfiles();}};
    svg.addEventListener('pointerup',finishCurve);svg.addEventListener('pointercancel',finishCurve);
    svg.addEventListener('keydown',e=>{const p=e.target.closest('[data-point]');if(!p||!['ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();const side=svg.dataset.curveSide,i=+p.dataset.point,t=profile().triggers[side];if(!['resistance','vibration'].includes(t.mode))return;state.side=side;t.zoneForces[i]=Math.round(clamp(t.zoneForces[i]+(e.key==='ArrowUp'?.02:-.02))*100)/100;renderTriggerControls();svg.querySelector(`[data-point="${i}"]`).focus();saveProfiles();});
  });
  $('#click-enabled').closest('.input-click-block').insertAdjacentHTML('afterbegin',`<div class="weapon-library"><h3>Weapon cues</h3><p>R2 creates a haptic recoil cue from your pull. Game events are needed for exact shot timing.</p><div class="weapon-preset-list"><button type="button" data-weapon-preset="pistol">Pistol</button><button type="button" data-weapon-preset="sniper">Sniper</button><button type="button" data-weapon-preset="burst">Burst</button><button type="button" data-weapon-preset="smg">SMG</button></div><label class="control-label" for="weapon-kind">Recoil pattern</label><select id="weapon-kind"><option value="click">Click</option><option value="pistol">Pistol</option><option value="sniper">Sniper</option><option value="burst">Three shot burst</option><option value="smg">Automatic</option></select><div id="weapon-rate-row" class="weapon-rate-row"><span>Rate</span><div class="weapon-rate-stepper"><button type="button" data-rate-step="-1" aria-label="Slower firing rate">−</button><b id="weapon-rate-value">10 shots/s</b><button type="button" data-rate-step="1" aria-label="Faster firing rate">+</button></div><small id="weapon-rate-gap">100 ms between shots</small></div><div id="burst-timing" class="burst-timing" hidden><div class="burst-live"><output id="burst-live-pull">R2: 0%</output><progress id="burst-live-meter" max="1" value="0"></progress></div><label class="control-label" for="burst-kick-style">Kick style</label><select id="burst-kick-style"><option value="strike">Motor recoil</option><option value="return">Return kick (experimental)</option><option value="stop">Firm stop</option></select><small id="burst-style-help"></small><label class="number-control">Spacing <span><input id="burst-spacing" type="number" min="50" max="1000" step="1" value="250"> ms</span></label><label id="burst-kick-control" class="number-control">Kick duration <span><input id="burst-kick" type="number" min="20" max="250" step="1" value="95"> ms</span></label><label class="number-control">Start zone <input id="burst-zone-start" type="number" min="1" max="10" step="1" value="3"></label><label id="burst-end-control" class="number-control">End zone <input id="burst-zone-end" type="number" min="2" max="9" step="1" value="7"></label><label id="burst-return-control" class="number-control">Return force <span><input id="burst-return-strength" type="number" min="1" max="100" step="1" value="100"> %</span></label><h4>Grip feedback per shot</h4><label class="number-control">Strength <span><input id="burst-grip-strength" type="number" min="0" max="100" step="1" value="85"> %</span></label><label class="number-control">Frequency <span><input id="burst-grip-frequency" type="number" min="20" max="250" step="1" value="90"> Hz</span></label><label class="number-control">Duration <span><input id="burst-grip-duration" type="number" min="20" max="250" step="1" value="95"> ms</span></label><small>Set grip strength to 0 for trigger feedback only. Shorter grip pulses keep the three shots separate.</small></div></div>`);
  $('#click-enabled').closest('.input-click-block').querySelector(':scope > h3').textContent='R2 cue';
  $('#click-enabled').closest('.input-click-block').querySelector(':scope > p').textContent='The cue starts when R2 crosses the pull point. Release to rearm single shots.';
  const weaponPresets={pistol:{kind:'pistol',threshold:.72,strength:62,rate:6,mode:'weapon',startZone:4,endZone:6},sniper:{kind:'sniper',threshold:.84,strength:100,triggerStrength:100,preloadStrength:15,inputEnabled:false,rate:3,mode:'weapon',forceCap:8,startZone:5,endZone:6},burst:{kind:'burst',threshold:.329,strength:100,rate:4,spacingMs:220,kickMs:95,kickStyle:'strike',startZone:1,endZone:6,returnStrength:100,hapticStrength:100,hapticFrequency:100,hapticDurationMs:78,mode:'off',triggerStrength:0,forceCap:8},smg:{kind:'smg',threshold:.40,strength:82,triggerStrength:40,rate:12,mode:'resistance',curve:[0,0,.1,.2,.25,.28,.3,.3,.3,.3,.3]}};
  $('.weapon-preset-list').addEventListener('click',async e=>{const button=e.target.closest('[data-weapon-preset]');if(!button)return;if(zoneRun)await stopZoneCalibration();const spec=weaponPresets[button.dataset.weaponPreset];if(!spec)return;const cue=inputEffectCommand(spec);delete cue.action;cue.enabled=spec.inputEnabled??true;profile().inputEffects=cue;const t=profile().triggers.right;t.mode=spec.mode;t.preloadStrength=spec.preloadStrength??0;t.strength=spec.triggerStrength??spec.strength;t.startZone=spec.startZone;t.endZone=spec.endZone;t.snapStrength=spec.snapStrength??t.snapStrength;t.forceCap=spec.forceCap??6;t.curve=clone(spec.curve??presets.Break);t.zoneForces=Array.from({length:10},(_,i)=>(t.curve[i]+t.curve[i+1])/2);state.side='right';renderTriggerControls();saveProfiles();toast(`${button.textContent} loaded · Apply to feel it`);});
  $('#weapon-kind').addEventListener('change',e=>{profile().inputEffects.kind=e.target.value;renderTriggerControls();saveProfiles();});
  $('#weapon-rate-row').addEventListener('click',e=>{const button=e.target.closest('[data-rate-step]');if(!button)return;profile().inputEffects.rate=clamp((profile().inputEffects.rate??10)+Number(button.dataset.rateStep),3,20);renderTriggerControls();saveProfiles();});
  $('#burst-kick-style').addEventListener('change',e=>{const c=profile().inputEffects;c.kickStyle=e.target.value;if(c.kickStyle==='return'){c.startZone=clamp(c.startZone??2,0,7);c.endZone=clamp(c.endZone??6,c.startZone+1,8);}renderTriggerControls();saveProfiles();});
  for(const [id,key,min,max,fallback] of [['burst-zone-start','startZone',0,9,2],['burst-zone-end','endZone',1,8,6],['burst-return-strength','returnStrength',1,100,100],['burst-grip-strength','hapticStrength',0,100,85],['burst-grip-frequency','hapticFrequency',20,250,90],['burst-grip-duration','hapticDurationMs',20,250,95]]){
    $(`#${id}`).addEventListener('change',e=>{const c=profile().inputEffects,zone=key==='startZone'||key==='endZone';c[key]=clamp(Math.round(e.target.valueAsNumber-(zone?1:0)),min,max);if(!Number.isFinite(c[key]))c[key]=fallback;if(c.kickStyle==='return'){c.startZone=clamp(c.startZone??2,0,7);c.endZone=clamp(c.endZone??6,c.startZone+1,8);}renderTriggerControls();saveProfiles();});
  }
  $('#burst-spacing').addEventListener('change',e=>{const c=profile().inputEffects;c.spacingMs=clamp(Math.round(+e.target.value||251),50,1000);c.kickMs=Math.min(c.kickMs||95,c.spacingMs-20);renderTriggerControls();saveProfiles();});
  $('#burst-kick').addEventListener('change',e=>{const c=profile().inputEffects;c.kickMs=clamp(Math.round(+e.target.value||95),20,250);c.spacingMs=Math.max(c.spacingMs||Math.round(1000/(c.rate??10)),c.kickMs+20);renderTriggerControls();saveProfiles();});
  bindRangeNumber('#click-threshold','#click-threshold-number');bindRangeNumber('#click-strength','#click-strength-number');
  $('#click-enabled').addEventListener('change',e=>{profile().inputEffects.enabled=e.target.checked;saveProfiles();});
  $('#click-threshold').addEventListener('input',e=>{profile().inputEffects.threshold=+e.target.value/100;$('#click-threshold-value').textContent=`${e.target.value}%`;fillRange(e.target);saveProfiles();});
  $('#click-strength').addEventListener('input',e=>{profile().inputEffects.strength=+e.target.value;$('#click-strength-value').textContent=`${e.target.value}%`;fillRange(e.target);saveProfiles();});
  $('#apply-triggers').addEventListener('click',async()=>{if(zoneRun)await stopZoneCalibration();const p=profile();for(const side of ['left','right']){const t=p.triggers[side],map=motorZones[side];if(t.mode==='bow')t.releasePoint=validMotorZones(map)?map[t.endZone??9]:clamp((t.endZone??9)/10,.2,.97);}if(await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right}))send(inputEffectCommand(p.inputEffects));});
  $('#reset-effects').addEventListener('click',async()=>{if(zoneRun)await stopZoneCalibration(false);send({action:'reset'});});

  function renderHapticControls(){
    const h=profile().haptics;
    $('#wave-shapes').innerHTML=['sine','square','saw','noise'].map(s=>`<button type="button" data-shape="${s}" class="${s===h.shape?'is-active':''}">${s}</button>`).join('');
    $('#haptic-sliders').innerHTML=hapticFields.map(f=>`<div class="haptic-slider"><label for="haptic-${f.key}">${f.label} <b id="haptic-${f.key}-value">${h[f.key]}${f.unit}</b></label><input id="haptic-${f.key}" data-field="${f.key}" type="range" min="${f.min}" max="${f.max}" value="${h[f.key]}"></div>`).join('');
    $$('#haptic-sliders input').forEach(fillRange);drawWave(performance.now());
  }
  $('#wave-shapes').addEventListener('click',e=>{const b=e.target.closest('[data-shape]');if(!b)return;profile().haptics.shape=b.dataset.shape;renderHapticControls();saveProfiles();});
  $('#haptic-sliders').addEventListener('input',e=>{const el=e.target.closest('[data-field]');if(!el)return;profile().haptics[el.dataset.field]=+el.value;$('#haptic-'+el.dataset.field+'-value').textContent=el.value+hapticFields.find(f=>f.key===el.dataset.field).unit;fillRange(el);drawWave(performance.now());saveProfiles();});
  $('#texture-list').innerHTML=textures.map((t,i)=>`<button type="button" data-texture="${i}"><small>${t.type}</small><strong>${t.name}</strong><i>Load</i></button>`).join('');
  $('#texture-list').addEventListener('click',e=>{const b=e.target.closest('[data-texture]');if(!b)return;Object.assign(profile().haptics,textures[+b.dataset.texture].settings);renderHapticControls();saveProfiles();playHaptic();});
  function waveSample(shape,p){const v=p%1;if(shape==='square')return Math.sin(p*Math.PI*2)>0?1:-1;if(shape==='saw')return 2*v-1;if(shape==='noise')return Math.sin(p*39.5)*Math.sin(p*61.3);return Math.sin(p*Math.PI*2);}
  function drawWave(now){
    const canvas=$('#wave-canvas'),ctx=canvas.getContext('2d'),h=profile().haptics,w=canvas.width,ht=canvas.height,ms=h.attack+h.release;
    ctx.clearRect(0,0,w,ht);ctx.beginPath();ctx.moveTo(0,ht/2);
    const cycles=Math.min(75,h.frequency*ms/1000);
    for(let x=0;x<=w;x+=3){const t=x/w,attack=h.attack/ms,env=t<attack?t/attack:(1-t)/(1-attack),amp=clamp(env)*Math.min(125,h.strength*1.25);ctx.lineTo(x,ht/2-waveSample(h.shape,t*cycles)*amp);}
    ctx.lineWidth=3;ctx.strokeStyle='#7ec9bc';ctx.stroke();ctx.strokeStyle='#d4a84b';ctx.lineWidth=1;ctx.beginPath();ctx.moveTo(0,ht/2);ctx.lineTo(w,ht/2);ctx.stroke();
    if(now<state.playUntil){const x=clamp((now-state.playStart)/state.playDuration)*w;ctx.fillStyle='rgba(212,168,75,.2)';ctx.fillRect(0,0,x,ht);ctx.fillStyle='#d4a84b';ctx.fillRect(x-2,0,4,ht);}
    $('#wave-formula').textContent=`${h.shape} / ${h.frequency} Hz`;$('#wave-duration').textContent=`${ms} MS`;
  }
  async function playHaptic(){
    const h=profile().haptics,duration=h.attack+h.release;
    if(state.connected) {
      send({action:'playHaptic',shape:h.shape,frequency:h.frequency,attack:h.attack,release:h.release,strength:h.strength,pan:h.pan});
    }
    if(!state.connected||pcAudioSketch)try{
      audioContext=audioContext||new(window.AudioContext||window.webkitAudioContext)();await audioContext.resume();const now=audioContext.currentTime;
      const gain=audioContext.createGain(),pan=audioContext.createStereoPanner();pan.pan.value=h.pan/100;
      gain.gain.setValueAtTime(.0001,now);gain.gain.linearRampToValueAtTime(Math.min(.08,h.strength/100*.08),now+h.attack/1000);gain.gain.linearRampToValueAtTime(.0001,now+duration/1000);
      let source;if(h.shape==='noise'){const buf=audioContext.createBuffer(1,Math.ceil(audioContext.sampleRate*duration/1000),audioContext.sampleRate),data=buf.getChannelData(0);for(let i=0;i<data.length;i++)data[i]=Math.random()*2-1;source=audioContext.createBufferSource();source.buffer=buf;}
      else{source=audioContext.createOscillator();source.type=h.shape==='saw'?'sawtooth':h.shape;source.frequency.value=h.frequency;}
      source.connect(gain).connect(pan).connect(audioContext.destination);source.start(now);source.stop(now+duration/1000);
    }catch(error){log(`AUDIO SKETCH / ${error}`);}
    state.playStart=performance.now();state.playDuration=duration;state.playUntil=state.playStart+duration;
  }
  $('#play-haptic').addEventListener('click',playHaptic);

  function renderBridge(){
    const available=state.connected&&state.transport==='USB';
    const toggle=$('#bridge-toggle'),status=$('#bridge-status');
    toggle.disabled=!available;
    toggle.textContent=state.audioBridge?'Stop bridge':'Start bridge';
    toggle.setAttribute('aria-pressed',String(state.audioBridge));
    status.classList.toggle('is-live',state.audioBridge);
    if(!state.connected)status.textContent='Connect a controller to start the bridge.';
    else if(!available)status.textContent='USB is required for system audio haptics. Haptic previews use motor rumble over Bluetooth.';
    else if(!state.audioBridge)status.textContent='Bridge off';
    $('#haptic-route').textContent=!state.connected?'Route: PC audio preview':state.audioRoute==='voiceCoil'?'Route: USB voice coils':state.audioRoute==='motor'?'Route: motor fallback':state.transport==='Bluetooth'?'Route: Bluetooth motors':'Route: USB voice coils if available';
  }
  $('#bridge-toggle').addEventListener('click',()=>send({action:'setAudioBridge',enabled:!state.audioBridge,gain:+$('#bridge-gain').value/100,preset:$('#bridge-preset').value}));
  $('#bridge-gain').addEventListener('input',e=>{$('#bridge-gain-value').textContent=`${e.target.value}%`;fillRange(e.target);});
  $('#bridge-gain').addEventListener('change',()=>{if(state.audioBridge)send({action:'setAudioBridge',enabled:true,gain:+$('#bridge-gain').value/100,preset:$('#bridge-preset').value});});
  $('#bridge-preset').addEventListener('change',()=>{if(state.audioBridge)send({action:'setAudioBridge',enabled:true,gain:+$('#bridge-gain').value/100,preset:$('#bridge-preset').value});});

  function renderAlerts(){
    const battery=$('#alert-low-battery');battery.textContent=`Low battery cue: ${state.lowBatteryAlert?'on':'off'}`;
    battery.setAttribute('aria-pressed',String(state.lowBatteryAlert));battery.classList.toggle('is-active',state.lowBatteryAlert);
    const remaining=Math.max(0,state.timerUntil-performance.now());
    if(!remaining)state.timerUntil=0;
    const seconds=Math.ceil(remaining/1000);
    $('#alert-timer-status').textContent=remaining>0?`Timer · ${Math.floor(seconds/60)} min ${String(seconds%60).padStart(2,'0')} sec left`:'Timer off';
    $('#alert-timer-cancel').disabled=!state.connected||remaining<=0;
  }
  $('#alert-ping').addEventListener('click',()=>send({action:'playHaptic',shape:'sine',frequency:125,attack:5,release:160,strength:26,pan:0}));
  $('#alert-ping').insertAdjacentHTML('afterend','<button id="speaker-chirp" class="quiet-action" type="button" disabled title="Play a test tone through the controller speaker over USB">Speaker test · USB</button>');
  $('.alert-body > div > p').textContent='Low battery and timer alerts tap the grips. Find my pad taps them now. Speaker chirp uses the controller speaker over USB. Reset, disconnect, or exit to stop alerts.';
  let speakerSound=null,speakerDecoded=null,speakerTrackDuration=0,speakerClipDuration=0,speakerClipStart=0,speakerStartedAt=0,speakerPending=false,speakerNextChunkStart=0,speakerChunkPending=false,speakerRoute='speaker',speakerScale=1,speakerGeneration=0,speakerPaused=false,speakerElapsed=0,speakerScrubbing=false;
  function renderSpeakerTransport(){const button=$('#speaker-file-play');button.textContent=speakerPending?'Starting…':speakerStartedAt?'Pause':speakerPaused?'Resume':'Play';button.disabled=speakerPending||!speakerDecoded||!state.connected||state.transport!=='USB';}
  function stopSpeakerPlayback(){speakerGeneration++;speakerStartedAt=0;speakerPending=false;speakerChunkPending=false;speakerPaused=false;speakerElapsed=0;$('#speaker-start').disabled=!speakerDecoded;$('#speaker-file-stop').disabled=true;renderSpeakerTransport();send({action:'stopSpeakerSound'});}
  async function testSpeaker(){stopSpeakerPlayback();$('#speaker-file-status').textContent='Playing speaker test';await send({action:'playSpeakerChirp'});}
  $('#speaker-chirp').addEventListener('click',testSpeaker);
  $('#speaker-panel-test').addEventListener('click',testSpeaker);
  $('#speaker-panel-test').insertAdjacentHTML('afterend','<button id="controller-mic" class="quiet-action" type="button" disabled aria-pressed="false">Controller mic: live</button>');
  let controllerMicMuted=false;
  $('#controller-mic').addEventListener('click',()=>send({action:'setMicrophoneMute',enabled:!controllerMicMuted}));
  const soundTime=seconds=>`${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,'0')}`;
  function renderSpeakerProgress(now=performance.now()){
    const elapsed=Math.min(speakerClipDuration,speakerElapsed+(speakerStartedAt?(now-speakerStartedAt)/1000:0));
    const position=speakerClipStart+elapsed;
    $('#speaker-time').textContent=`${soundTime(position)} / ${soundTime(speakerTrackDuration)}`;
    $('#speaker-progress').max=Math.max(speakerTrackDuration,1);
    $('#speaker-progress').value=position;
    if(!speakerScrubbing){$('#speaker-start').value=String(position);$('#speaker-start-value').textContent=soundTime(position);}
    if(speakerStartedAt&&elapsed>=speakerClipDuration){speakerStartedAt=0;$('#speaker-start').disabled=+$('#speaker-start').max<=0;$('#speaker-file-stop').disabled=true;$('#speaker-file-status').textContent='Finished';}
    else if(speakerStartedAt&&speakerNextChunkStart<speakerTrackDuration&&position>=speakerNextChunkStart-6)queueSpeakerChunk();
    renderSpeakerTransport();
  }
  function prepareSpeakerSound(){
    if(!speakerDecoded)return;
    const decoded=speakerDecoded, count=Math.min(144_000,Math.ceil((decoded.duration-speakerClipStart)*48_000));
    if(!count)throw Error('The selected section has no audio.');
    const channels=Array.from({length:Math.min(2,decoded.numberOfChannels)},(_,i)=>decoded.getChannelData(i));
    const mono=new Float32Array(count);let peak=0;
    for(let i=0;i<count;i++){
      const position=(speakerClipStart+i/48_000)*decoded.sampleRate,index=Math.floor(position),mix=position-index;
      let sample=0;
      for(const data of channels)sample+=(data[index]||0)*(1-mix)+(data[index+1]||0)*mix;
      sample/=channels.length;mono[i]=sample;peak=Math.max(peak,Math.abs(sample));
    }
    const scale=.85;speakerScale=scale;
    const bytes=new Uint8Array(count*2),view=new DataView(bytes.buffer);
    for(let i=0;i<count;i++)view.setInt16(i*2,Math.round(clamp(mono[i]*scale,-1,1)*32767),true);
    let binary='';for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));
    speakerSound=btoa(binary);speakerClipDuration=speakerTrackDuration-speakerClipStart;speakerNextChunkStart=speakerClipStart+count/48_000;
    $('#speaker-file-status').textContent=peak<.005?'This section starts quietly.':'Ready to play';
    $('#speaker-file-play').disabled=!state.connected||state.transport!=='USB';
    renderSpeakerProgress();
  }
  function encodeSpeakerChunk(start){
    const decoded=speakerDecoded,count=Math.min(144_000,Math.ceil((decoded.duration-start)*48_000));
    if(count<=0)return null;
    const channels=Array.from({length:Math.min(2,decoded.numberOfChannels)},(_,i)=>decoded.getChannelData(i));
    const bytes=new Uint8Array(count*2),view=new DataView(bytes.buffer);
    for(let i=0;i<count;i++){
      const position=(start+i/48_000)*decoded.sampleRate,index=Math.floor(position),mix=position-index;
      let sample=0;
      for(const data of channels)sample+=(data[index]||0)*(1-mix)+(data[index+1]||0)*mix;
      view.setInt16(i*2,Math.round(clamp(sample/channels.length*speakerScale,-1,1)*32767),true);
    }
    let binary='';for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));
    return {sound:btoa(binary),duration:count/48_000};
  }
  async function queueSpeakerChunk(){
    if(speakerChunkPending||!speakerStartedAt||speakerNextChunkStart>=speakerTrackDuration)return;
    speakerChunkPending=true;
    const generation=speakerGeneration;
    const next=encodeSpeakerChunk(speakerNextChunkStart);
    if(next&&await send({action:'playSpeakerSound',pcmBase64:next.sound,route:speakerRoute,append:true})){if(generation===speakerGeneration)speakerNextChunkStart+=next.duration;}
    else {speakerStartedAt=0;$('#speaker-file-status').textContent='Playback stopped';}
    speakerChunkPending=false;
  }
  $('#speaker-file').addEventListener('change',async event=>{
    if(speakerStartedAt||speakerPending)stopSpeakerPlayback();
    speakerSound=null;speakerDecoded=null;speakerStartedAt=0;$('#speaker-file-play').disabled=true;
    const file=event.target.files[0];if(!file)return;
    if(file.size>64_000_000){$('#speaker-file-status').textContent='Choose a file under 64 MB.';return;}
    try{
      audioContext=audioContext||new(window.AudioContext||window.webkitAudioContext)();
      speakerDecoded=await audioContext.decodeAudioData(await file.arrayBuffer());
      speakerTrackDuration=speakerDecoded.duration;speakerClipStart=0;
      if(!speakerTrackDuration)throw Error('The file has no audio.');
      if(speakerTrackDuration>600)throw Error('Choose a track under ten minutes.');
      $('#speaker-start').max=String(Math.max(0,speakerTrackDuration-.1));
      $('#speaker-start').value='0';$('#speaker-start').disabled=speakerTrackDuration<=.1;
      $('#speaker-start-value').textContent='0:00';
      prepareSpeakerSound();
      $('#speaker-file-status').title=file.name;
    }catch(error){$('#speaker-file-status').textContent=`Could not read sound: ${error.message}`;}
  });
  $('#speaker-start').addEventListener('pointerdown',()=>speakerScrubbing=true);
  $('#speaker-start').addEventListener('keydown',()=>speakerScrubbing=true);
  $('#speaker-start').addEventListener('blur',()=>speakerScrubbing=false);
  window.addEventListener('pointerup',()=>speakerScrubbing=false);
  $('#speaker-start').addEventListener('input',e=>{$('#speaker-start-value').textContent=soundTime(+e.target.value);});
  $('#speaker-start').addEventListener('change',e=>{speakerScrubbing=false;const playing=!!speakerStartedAt,position=+e.target.value;stopSpeakerPlayback();speakerClipStart=position;prepareSpeakerSound();if(playing)$('#speaker-file-play').click();});
  $('#speaker-file-play').addEventListener('click',async()=>{if(!speakerDecoded)return;if(speakerStartedAt||speakerPaused){await send({action:'setSpeakerPaused',paused:!speakerPaused});return;}stopSpeakerPlayback();prepareSpeakerSound();speakerPending=true;speakerRoute=$('#speaker-route').value;$('#speaker-file-stop').disabled=false;renderSpeakerTransport();if(!await send({action:'playSpeakerSound',pcmBase64:speakerSound,route:speakerRoute}))speakerPending=false;renderSpeakerTransport();});
  $('#speaker-file-stop').addEventListener('click',()=>{stopSpeakerPlayback();$('#speaker-file-status').textContent='Stopped';renderSpeakerProgress();});
  $('#speaker-route').addEventListener('change',()=>{if(speakerStartedAt||speakerPaused||speakerPending){stopSpeakerPlayback();$('#speaker-file-status').textContent='Output changed. Press Play.';}});
  setInterval(()=>{if(speakerStartedAt)renderSpeakerProgress();},400);
  $('#alert-low-battery').addEventListener('click',()=>send({action:'setLowBatteryAlert',enabled:!state.lowBatteryAlert}));
  $('#alert-timer-start').addEventListener('click',()=>send({action:'setHapticTimer',seconds:Number($('#alert-timer-minutes').value)*60}));
  $('#alert-timer-cancel').addEventListener('click',()=>send({action:'setHapticTimer',seconds:0}));

  const lightMap=$('.controller-map').cloneNode(true);
  lightMap.setAttribute('aria-label','Front view of DualSense showing diffused light around the touchpad');
  lightMap.querySelectorAll('[id]').forEach(node=>{node.id=`light-${node.id}`;});
  lightMap.querySelectorAll('*').forEach(node=>{
    for(const attribute of [...node.attributes]){
      if(attribute.value.includes('url(#'))node.setAttribute(attribute.name,attribute.value.replace(/url\(#([^)]*)\)/g,'url(#light-$1)'));
      if(attribute.name==='href'&&attribute.value.startsWith('#'))node.setAttribute('href',`#light-${attribute.value.slice(1)}`);
    }
    node.removeAttribute('data-button');
    node.classList.remove('is-pressed');
  });
  lightMap.querySelectorAll('.map-touch-contact').forEach(node=>node.remove());
  const ns='http://www.w3.org/2000/svg';
  const plastic=document.createElementNS(ns,'path');plastic.setAttribute('class','map-light-plastic');plastic.setAttribute('d','M493 167 Q734 139 974 167 Q1027 171 1041 232 L997 413 Q988 463 942 468 H525 Q479 463 470 413 L426 232 Q440 171 493 167 Z');
  lightMap.querySelector('.map-touchpad').before(plastic);
  const lightStrip=lightMap.querySelector('.map-light-strip');
  const halo=lightStrip.cloneNode(true);halo.setAttribute('class','map-light-halo');lightStrip.before(halo);
  const playerIndicators=document.createElementNS(ns,'g');playerIndicators.setAttribute('class','map-player-indicators');
  for(let i=0;i<5;i++){const dot=document.createElementNS(ns,'circle');dot.setAttribute('cx',String(680+i*27));dot.setAttribute('cy','500');dot.setAttribute('r','8');dot.setAttribute('data-player-preview',String(i));playerIndicators.append(dot);}
  lightMap.append(playerIndicators);
  $('#light-controller-map').append(lightMap);
  const lightColumn=document.createElement('div');
  lightColumn.className='light-left-column';
  $('.light-layout').prepend(lightColumn);
  lightColumn.append($('.light-display'),$('#lights .telemetry-panel'));
  let playerLedMask=0;
  function renderPlayerLeds(){
    $$('#player-leds [data-player-led]').forEach(button=>{const on=!!(playerLedMask&(1<<Number(button.dataset.playerLed)));button.setAttribute('aria-pressed',String(on));button.classList.toggle('is-active',on);});
    $$('.map-player-indicators [data-player-preview]').forEach(dot=>dot.classList.toggle('is-active',!!(playerLedMask&(1<<Number(dot.dataset.playerPreview)))));
  }
  $('#player-leds').addEventListener('click',event=>{const button=event.target.closest('[data-player-led]');if(!button)return;playerLedMask^=1<<Number(button.dataset.playerLed);renderPlayerLeds();});
  $('#set-player-leds').addEventListener('click',()=>send({action:'setPlayerLeds',mask:playerLedMask}));
  renderPlayerLeds();
  const validHex = value => /^#[0-9a-f]{6}$/i.test(value);
  function setLightColor(hex){
    if(!validHex(hex))return false;
    hex=hex.toLowerCase();$('#light-color').value=hex;$('#light-hex-input').value=hex.toUpperCase();
    $('#light-hex').textContent=hex.toUpperCase();$('#light-arcs').style.setProperty('--light',hex);
    $('#light-hex-error').textContent='';return true;
  }
  const lightPalettes={
    'Classic':['#d4a84b','#3d9b8c','#f1f0e8','#3d74d8','#dc4565','#212c51'],
    'Neon':['#ff3da1','#9c57ff','#37d9ff','#42e47e','#ffb12b','#fa513a'],
    'Soft':['#e9a8c0','#b9a7e8','#93c8df','#9dd7c1','#f1d198','#d9b7a0'],
    'Warm':['#ff642e','#ec324f','#ff9e45','#f5c73d','#c77b4b','#ad556b']
  };
  const colorButton=c=>`<button type="button" data-color="${c}" style="--color:${c}" aria-label="Choose ${c}"></button>`;
  $('#light-presets').innerHTML=Object.entries(lightPalettes).map(([name,colors])=>`<div class="light-palette-row"><span>${name}</span><div>${colors.map(colorButton).join('')}</div></div>`).join('');
  let savedColors=[];
  try{const saved=JSON.parse(localStorage.getItem('aftertouch.lightColors')||'[]');if(Array.isArray(saved))savedColors=saved.filter(validHex).slice(-12);}catch{}
  function renderSavedColors(){
    $('#light-favorites').innerHTML=savedColors.length?savedColors.map(colorButton).join(''):'<span class="light-empty">No saved colors yet.</span>';
  }
  renderSavedColors();
  $$('#light-presets,#light-favorites').forEach(host=>host.addEventListener('click',e=>{const b=e.target.closest('[data-color]');if(b)setLightColor(b.dataset.color);}));
  $('#light-color').addEventListener('input',e=>setLightColor(e.target.value));
  $('#light-hex-input').addEventListener('change',e=>{if(!setLightColor(e.target.value.trim()))$('#light-hex-error').textContent='Use a six-digit hex color, for example #42A5F5.';});
  $('#save-light-color').addEventListener('click',()=>{
    const color=$('#light-color').value.toLowerCase();savedColors=[...savedColors.filter(item=>item!==color),color].slice(-12);
    try{localStorage.setItem('aftertouch.lightColors',JSON.stringify(savedColors));}catch{}
    renderSavedColors();toast('Color saved locally.');
  });
  $('#send-light').addEventListener('click',()=>{const h=$('#light-color').value;send({action:'setLightbar',rgb:[1,3,5].map(i=>parseInt(h.slice(i,i+2),16))});});
  $('#battery-light').addEventListener('click',()=>send({action:'batteryLightbar',enabled:!state.batteryLight}));
  $('#battery-light').insertAdjacentHTML('afterend','<button class="quiet-action" id="screen-light" type="button" aria-pressed="false" disabled>Follow screen color: off</button><small class="screen-light-status" id="screen-light-status" role="status">Samples the primary display locally only while enabled.</small>');
  $('#battery-light').insertAdjacentHTML('afterend','<button class="quiet-action" id="temperature-light" type="button" aria-pressed="false" disabled>Sensor color: off</button><small class="screen-light-status" id="temperature-light-status" role="status">Maps an uncalibrated raw byte to color.</small>');
  $('#temperature-light').addEventListener('click',()=>send({action:'setTemperatureLightbar',enabled:!state.temperatureLight}));
  $('#screen-light').addEventListener('click',()=>send({action:'setScreenLightbar',enabled:!state.screenLight}));
  $('#lights .page-heading p').textContent='Set a color or follow battery, race data, sensor, or screen color.';
  $('#release-light').addEventListener('click',()=>send({action:'releaseLightbar'}));
  document.querySelector('label[for="telemetry-port"]').insertAdjacentHTML('beforebegin','<label for="telemetry-format">Input format</label><select id="telemetry-format"><option value="forza">Forza Data Out · Dash</option><option value="json">Local JSON adapter</option></select><details id="telemetry-json-help" class="telemetry-schema" hidden><summary>JSON packet format</summary><code>{"rpm":6100,"maxRpm":8000,"speedKmh":108,"gear":4,"brake":150,"slip":1.4,"frontSlip":1.3,"kerb":[true,false,false,false]}</code><p>Send one UTF-8 JSON object per UDP datagram. Keep each under 512 bytes. Fields after maxRpm are optional.</p></details>');
  function renderTelemetryFormat(){
    const json=$('#telemetry-format').value==='json';
    $('.telemetry-panel .panel-cap span').textContent=json?'Local adapter':'Forza Data Out';
    $('#telemetry-json-help').hidden=!json;
    $('.telemetry-intro p').textContent=json
      ?'Send local JSON telemetry to drive light color and short grip cues. Enable LAN only for a trusted sender.'
      :"Set Forza Data Out to Dash. Motorsport 2023 sends to 127.0.0.1. For Horizon 4/5 or a console, enable LAN and use this PC's IPv4 address. Revs set light color; gear changes play a short grip cue. Brake slip is estimated from wheel slip.";
  }
  $('#telemetry-format').addEventListener('change',()=>{renderTelemetryFormat();renderTelemetry();if(state.telemetryOn)send({action:'setTelemetry',enabled:true,port:Number($('#telemetry-port').value),lan:$('#telemetry-lan').checked,format:$('#telemetry-format').value});});
  renderTelemetryFormat();
  $('#telemetry-toggle').addEventListener('click',()=>{
    const port=Number($('#telemetry-port').value);
    if(!Number.isInteger(port)||port<1024||port>65535){toast('Choose a UDP port from 1024 to 65535.');return;}
    send({action:'setTelemetry',enabled:!state.telemetryOn,port,lan:$('#telemetry-lan').checked,format:$('#telemetry-format').value});
  });
  $('#telemetry-port').addEventListener('change',()=>{
    const port=Number($('#telemetry-port').value);
    if(state.telemetryOn&&Number.isInteger(port)&&port>=1024&&port<=65535)send({action:'setTelemetry',enabled:true,port,lan:$('#telemetry-lan').checked,format:$('#telemetry-format').value});
  });
  $('#telemetry-lan').addEventListener('change',()=>{if(state.telemetryOn)send({action:'setTelemetry',enabled:true,port:Number($('#telemetry-port').value),lan:$('#telemetry-lan').checked,format:$('#telemetry-format').value});});
  $('.light-layout').insertAdjacentHTML('afterend',`<div class="script-panel framed"><div class="panel-cap"><span>Telemetry rules</span><span>LOCAL SCRIPT</span></div><div class="script-body"><div class="script-intro"><span class="script-kicker">01 / Rule</span><h2>Edit telemetry rule</h2><p>Use race data to set a light color or grip cue. Runs locally up to 10 Hz.</p><div class="script-signals"><strong>Signals</strong><span>rpm · max_rpm · speed_kmh · gear · brake · slip · front_slip · kerb_left · kerb_right</span></div><div class="script-signals"><strong>Outputs</strong><span>light: [red, green, blue] · cue: "pulse", "grit", or "shift"</span></div><p class="script-boundary">Rules can set lights and grip cues only. They cannot access files, the network, keys, or triggers. Invalid rules stop automatically.</p></div><div class="script-editor"><label for="script-source">Effect rule <small>Rhai · 4 KB max</small></label><textarea id="script-source" maxlength="4096" spellcheck="false" rows="8" aria-describedby="script-status"></textarea><div class="script-actions"><button id="script-example" class="quiet-action" type="button">Load example</button><button id="script-toggle" class="main-action" type="button" aria-pressed="false">Arm rule</button><button id="telemetry-demo" class="quiet-action" type="button" disabled>Send test lap</button></div><p id="script-status" role="status">Off · start telemetry to see a response</p><div class="script-monitor"><span>Last output</span><strong id="script-output">Waiting for a signal</strong></div></div></div></div>`);
  const scriptExample='if rpm / max_rpm > 0.8 { #{ light: [220, 40, 20], cue: "pulse" } } else { #{ light: [40, 145, 130] } }';
  try{$('#script-source').value=localStorage.getItem('aftertouch.script.v1')||scriptExample;}catch{$('#script-source').value=scriptExample;}
  $('#script-source').addEventListener('input',()=>{try{localStorage.setItem('aftertouch.script.v1',$('#script-source').value);}catch{}});
  $('#script-example').addEventListener('click',()=>{$('#script-source').value=scriptExample;$('#script-source').dispatchEvent(new Event('input'));});
  $('#script-toggle').addEventListener('click',()=>send({action:'setScript',enabled:!state.scriptEnabled,source:$('#script-source').value}));
  $('#telemetry-demo').addEventListener('click',async()=>{
    if(!native||!state.telemetryOn||$('#telemetry-format').value!=='json')return;
    try{await invoke('send_demo_telemetry',{port:Number($('#telemetry-port').value)});}
    catch(error){toast(`Test lap failed: ${error}`);}
  });
  function renderScript(){const button=$('#script-toggle');button.textContent=state.scriptEnabled?'Disarm rule':'Arm rule';button.setAttribute('aria-pressed',String(state.scriptEnabled));button.classList.toggle('is-armed',state.scriptEnabled);$('#script-source').readOnly=state.scriptEnabled;$('#script-example').disabled=state.scriptEnabled;}
  function renderTelemetry(){
    $('#telemetry-demo').disabled=!native||!state.telemetryOn||$('#telemetry-format').value!=='json';
    $('#telemetry-toggle').textContent=state.telemetryOn?'Stop listening':'Start listening';
    $('#telemetry-toggle').setAttribute('aria-pressed',String(state.telemetryOn));
    if(!state.telemetryOn){$('#telemetry-status').textContent='Off';$('#race-speed').textContent='—';$('#race-gear').textContent='—';$('#race-rpm').textContent='—';$('#race-source').textContent='Waiting for telemetry';$('#race-slip').textContent='Brake slip —';$('#rev-fill').style.width='0%';}
  }
  function renderBatteryLight(){const b=$('#battery-light');b.textContent=`Battery color: ${state.batteryLight?'on':'off'}`;b.setAttribute('aria-pressed',String(state.batteryLight));b.classList.toggle('is-active',state.batteryLight);}
  function renderTemperatureLight(){const b=$('#temperature-light');b.textContent=`Sensor color: ${state.temperatureLight?'on':'off'}`;b.setAttribute('aria-pressed',String(state.temperatureLight));b.classList.toggle('is-active',state.temperatureLight);$('#temperature-light-status').textContent=state.temperatureLight?'Waiting for the raw sensor byte.':'Maps an uncalibrated raw byte to color.';}
  function renderScreenLight(){const b=$('#screen-light');b.textContent=`Follow screen color: ${state.screenLight?'on':'off'}`;b.setAttribute('aria-pressed',String(state.screenLight));b.classList.toggle('is-active',state.screenLight);$('#screen-light-status').textContent=state.screenLight?'Sampling the primary display locally every 400 ms.':'Samples the primary display locally only while enabled.';}
  $('#touch-mouse-toggle').addEventListener('click',()=>send({action:'setTouchMouse',enabled:!state.mouse}));
  function renderMouseSwitch(){const b=$('#touch-mouse-toggle');b.classList.toggle('is-active',state.mouse);b.setAttribute('aria-pressed',String(state.mouse));b.textContent=state.mouse?'On':'Off';renderGestureSwitch();}
  $('#touch-gesture-toggle').addEventListener('click',()=>send({action:'setTouchGestures',enabled:!state.gestures}));
  $('#touch-gesture-toggle').parentElement.querySelector('strong').textContent='Edge and circle gestures';
  $('#touch-gesture-toggle').parentElement.querySelector('p').textContent='Swipe inward for an arrow key. Rotate two fingers for circle scrolling. Requires Touchpad mouse.';
  function renderGestureSwitch(){const b=$('#touch-gesture-toggle');b.classList.toggle('is-active',state.gestures&&state.mouse);b.setAttribute('aria-pressed',String(state.gestures));b.disabled=!state.connected||!state.mouse;b.textContent=state.gestures?(state.mouse?'On':'Paused'):'Off';}

  $('#gyro-sens').insertAdjacentHTML('afterend',`<label class="control-label" for="gyro-deadzone"><span>Noise floor</span><b id="gyro-deadzone-value">90</b></label><input id="gyro-deadzone" type="range" min="0" max="512" step="16" value="90"><label class="control-label" for="gyro-smoothing"><span>Smoothing</span><b id="gyro-smoothing-value">18%</b></label><input id="gyro-smoothing" type="range" min="0" max="85" value="18">`);
  function syncGyroCommand() {
    const g = profile().gyroAim;
    g.sensitivity = (+$('#gyro-sens').value) / 10;
    g.activation = $('#gyro-activation').value;
    g.invertY = $('#gyro-invert').checked;
    g.deadzone = +$('#gyro-deadzone').value;
    g.smoothing = +$('#gyro-smoothing').value/100;
    g.enabled = state.gyroAim;
    $('#gyro-sens-value').textContent = `${g.sensitivity.toFixed(1)}×`;
    fillRange($('#gyro-sens'));
    saveProfiles();
    if (state.connected) {
      send({
        action:'setGyroAim',
        enabled: state.gyroAim,
        sensitivity: g.sensitivity,
        yawScale: g.yawScale || 1,
        pitchScale: g.pitchScale || 1,
        invertY: g.invertY,
        activation: g.activation,
        bias: g.bias || [0,0,0],deadzone:g.deadzone,smoothing:g.smoothing
      });
    }
  }
  function renderGyroControls() {
    const g = profile().gyroAim || defaultGyro();
    $('#gyro-sens').value = Math.round((g.sensitivity || 1) * 10);
    $('#gyro-sens-value').textContent = `${Number(g.sensitivity||1).toFixed(1)}×`;
    $('#gyro-activation').value = g.activation || 'always';
    $('#gyro-invert').checked = !!g.invertY;
    $('#gyro-deadzone').value=g.deadzone??90;
    $('#gyro-deadzone-value').textContent=String(g.deadzone??90);
    $('#gyro-smoothing').value=Math.round((g.smoothing??.18)*100);
    $('#gyro-smoothing-value').textContent=`${Math.round((g.smoothing??.18)*100)}%`;
    fillRange($('#gyro-sens'));
    fillRange($('#gyro-deadzone'));fillRange($('#gyro-smoothing'));
    const b = $('#gyro-aim-toggle');
    b.disabled=!state.connected||!state.fullReport||!state.platformDesktopInput;
    b.classList.toggle('is-active', state.gyroAim);
    b.setAttribute('aria-pressed', String(state.gyroAim));
    b.textContent = state.gyroAim ? 'On' : 'Off';
    $('#gyro-calibrate').disabled=!state.connected||!state.fullReport||!!state.gyroCalibration;
    if(!state.gyroCalibration)$('#gyro-calibration-status').textContent=(g.bias||[]).some(n=>n!==0)?`Saved still bias: ${(g.bias||[]).join(' / ')}. Calibrate again if the pad drifts.`:'Set the pad down for two seconds. Saves to this profile.';
  }
  $('#gyro-aim-toggle').addEventListener('click', () => {
    if (!state.connected) { toast('Connect a controller.'); return; }
    state.gyroAim = !state.gyroAim;
    syncGyroCommand();
    renderGyroControls();
  });
  $('#gyro-sens').addEventListener('input', () => { fillRange($('#gyro-sens')); $('#gyro-sens-value').textContent = `${(+$('#gyro-sens').value/10).toFixed(1)}×`; });
  $('#gyro-sens').addEventListener('change', syncGyroCommand);
  for(const [id,label] of [['gyro-deadzone','gyro-deadzone-value'],['gyro-smoothing','gyro-smoothing-value']]){$(`#${id}`).addEventListener('input',()=>{$(`#${label}`).textContent=$(`#${id}`).value+(id==='gyro-smoothing'?'%':'');fillRange($(`#${id}`));});$(`#${id}`).addEventListener('change',syncGyroCommand);}
  $('#gyro-activation').addEventListener('change', syncGyroCommand);
  $('#gyro-invert').addEventListener('change', syncGyroCommand);
  $('#gyro-calibrate').addEventListener('click',()=>{
    if(!state.connected||!state.fullReport){toast('Connect a controller with full motion reports first.');return;}
    if(state.gyroCalibration)return;
    const calibration={count:0,sum:[0,0,0],sumSq:[0,0,0],timer:null};
    state.gyroCalibration=calibration;
    $('#gyro-calibration-status').textContent='Keep the controller still… sampling for two seconds.';
    $('#gyro-calibrate').disabled=true;
    calibration.timer=setTimeout(()=>{
      if(state.gyroCalibration!==calibration)return;
      state.gyroCalibration=null;
      $('#gyro-calibrate').disabled=!state.connected||!state.fullReport;
      if(calibration.count<25){$('#gyro-calibration-status').textContent='Too few motion reports. Try again with the controller connected.';return;}
      const bias=calibration.sum.map(n=>Math.round(n/calibration.count));
      const moving=calibration.sumSq.some((n,i)=>Math.sqrt(Math.max(0,n/calibration.count-(calibration.sum[i]/calibration.count)**2))>250);
      if(moving||bias.some(n=>Math.abs(n)>4096)){$('#gyro-calibration-status').textContent='The controller moved during calibration. Set it down and retry.';return;}
      profile().gyroAim.bias=bias;
      saveProfiles();syncGyroCommand();
      $('#gyro-calibration-status').textContent=`Calibrated · bias ${bias.join(' / ')} saved to this profile.`;
    },2200);
  });

  function renderXinput() {
    const b = $('#xinput-toggle');
    b.classList.toggle('is-active', state.xinput);
    b.setAttribute('aria-pressed', String(state.xinput));
    b.textContent = state.xinput ? 'On' : 'Off';
    b.disabled = !state.connected;
    $('#xinput-status').textContent = state.xinputMessage || 'ViGEm not connected';
    $('#diag-xinput').textContent = state.xinput ? 'Active' : (state.xinputStatus === 'unavailable' ? 'No ViGEm' : 'Off');
  }
  $('#xinput-toggle').addEventListener('click', () => {
    if (!state.connected) { toast('Connect a controller.'); return; }
    send({action:'setXinput', enabled: !state.xinput});
  });
  $('.xinput-block').insertAdjacentHTML('beforeend','<div class="driver-actions"><button id="setup-vigem" class="quiet-action" type="button">Set up XInput driver</button><button id="setup-hidhide" class="quiet-action" type="button">HidHide download</button><small>ViGEmBus creates the virtual Xbox pad. Use HidHide if a game sees both pads.</small></div>');
  for(const [id,kind] of [['setup-vigem','vigem'],['setup-hidhide','hidhide']]){
    $(`#${id}`).addEventListener('click',async()=>{
      if(!native){toast('Open the Windows app to set up drivers.');return;}
      try{toast(await invoke('open_driver_setup',{kind}));}
      catch(error){toast(String(error));log(`DRIVER SETUP / ${error}`);}
    });
  }

  $('#diag-reset').addEventListener('click',()=>send({action:'reset'}));
  $('#quick-reset').addEventListener('click',()=>send({action:'reset'}));
  function finishScan(){clearTimeout(scanTimeout);state.manualScan=false;$('#scan-button').setAttribute('aria-busy','false');$('#scan-button').setAttribute('aria-label','Scan for controllers');}
  async function manualScan(){
    if(state.manualScan)return;
    state.manualScan=true;$('#scan-button').setAttribute('aria-busy','true');$('#scan-button').setAttribute('aria-label','Scanning for controllers');
    scanTimeout=setTimeout(finishScan,4000);
    if(!await send({action:'listDevices'}))finishScan();
  }
  $('#diag-scan').addEventListener('click',manualScan);
  $('#scan-button').addEventListener('click',manualScan);
  $('#guide-scan').addEventListener('click',manualScan);
  $('#copy-diagnostics').addEventListener('click',async()=>{
    const report=[
      'CONTROLLER diagnostics',
      `Device: ${state.connected?`${state.name} (${state.transport})`:'Disconnected'}`,
      `Firmware: ${$('#diag-firmware').textContent}`,
      `Input: ${$('#session-signal').textContent}`,
      `Rate: ${state.connected?`${state.rate}/s`:'—'} · gaps ${state.sequenceGaps} · invalid ${state.invalidReports}`,
      `Battery: ${$('#diag-battery').textContent} · ${$('#diag-charge').textContent}`,
      `Sensor byte: ${$('#diag-temp').textContent}`,
      `XInput: ${$('#diag-xinput').textContent}`,
      `Conflict: ${$('#conflict-message').textContent}`
    ].join('\n');
    try {
      if(navigator.clipboard?.writeText) await navigator.clipboard.writeText(report);
      else {
        const field=document.createElement('textarea');field.value=report;field.style.position='fixed';field.style.opacity='0';document.body.append(field);field.select();
        const copied=document.execCommand('copy');field.remove();
        if(!copied)throw new Error('Clipboard unavailable');
      }
      toast('Diagnostics copied.');
    } catch { toast('Could not copy diagnostics.'); }
  });
  $('#button-matrix').innerHTML=names.map((n,i)=>`<span data-matrix="${i}">${n}</span>`).join('');

  function renderDeviceList() {
    $('#device-count').textContent = String(state.devices.length).padStart(2,'0');
    const list = $('#device-list'); list.replaceChildren();
    if (!state.devices.length) {
      list.innerHTML = '<p class="device-empty">No pads. Plug in or pair, then scan.</p>';
      return;
    }
    state.devices.forEach((device, i) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'device-row' + (device.path === state.selectedPath || device.path === state.path ? ' is-active' : '');
      b.innerHTML = `<span class="number">${String(i+1).padStart(2,'0')}</span><span><strong></strong><small></small></span><span class="arrow">↗</span>`;
      $('strong', b).textContent = device.name || 'DualSense';
      $('small', b).textContent = `${device.transport}${device.isEdge ? ' • EDGE' : ''} / ${device.productId || ''}`;
      b.addEventListener('click', () => send({action:'selectDevice', path: device.path}));
      list.append(b);
    });
  }

  function setConnected(connected,name='',transport='',firmware='',path=''){
    const wasCalibrating=!!calibrationRun;
    state.connected=connected;state.name=name;state.transport=transport;state.path=connected?path:'';
    if(!connected&&zoneRun)stopZoneCalibration(false);
    $('#side-dot').parentElement.classList.toggle('is-connected',connected);
    $('#side-state').textContent=connected?'Connected':'Searching';
    $('#top-transport').textContent=connected?transport:'No device';
    $('.mast-link').classList.toggle('is-live',connected);
    $('#photo-status').textContent=connected?`${transport} live`:'Waiting';
    $('#device-name').textContent=connected?name:'No controller';
    $('#device-subtitle').textContent=connected?`${transport} stream`:'USB or Bluetooth';
    $('#connection-guide').hidden=connected;
    $('#diag-device-name').textContent=connected?name:'Waiting';
    $('#diag-transport').textContent=connected?`${transport} / 054C`:'USB / Bluetooth';
    $('#diag-firmware').textContent=connected?(firmware||'Firmware unknown'):'Firmware —';
    $('#owner-status').textContent=connected?'Core writer':'No pad';
    $('#owner-square').classList.toggle('is-live',connected);
    $$('#apply-triggers,#reset-effects,#touch-mouse-toggle,#gyro-aim-toggle,#xinput-toggle,#send-light,#set-player-leds,#release-light,#temperature-light,#screen-light,#diag-reset,#quick-reset,#apply-profile-now,#alert-ping,#alert-low-battery,#alert-timer-start').forEach(b=>b.disabled=!connected);
    $('#speaker-chirp').disabled=!connected||transport!=='USB';
    $('#speaker-panel-test').disabled=!connected||transport!=='USB';
    $('#controller-mic').disabled=!connected;
    $('#speaker-file-play').disabled=!connected||transport!=='USB'||!speakerSound;
    if(!connected){clearPadFocus();calibrationRun=null;if(state.gyroCalibration)clearTimeout(state.gyroCalibration.timer);state.gyroCalibration=null;state.buttons.fill(0);state.axes.fill(0);state.touch=[null,null];state.battery=null;state.batteryInfo=null;state.charging=false;state.mouse=false;state.gestures=false;state.gyroAim=false;state.remapArmed=false;state.pcMode=false;state.inputEffectsActive=false;$('#click-status').textContent='Not armed';state.batteryLight=false;state.temperatureLight=false;state.screenLight=false;state.audioBridge=false;state.audioRoute='off';state.xinput=false;state.xinputStatus='disabled';state.xinputMessage='ViGEm not connected';state.leaseUntil=0;state.lastInputAt=0;state.fullReport=false;state.activity.fill(0);state.stickTest=null;$('#diag-rate').textContent='—';$('#diag-gaps').textContent='—';$('#diag-invalid').textContent='—';renderMouseSwitch();renderGyroControls();renderXinput();renderBatteryLight();renderTemperatureLight();renderScreenLight();renderInput();}
    renderPcMode();
    if(!connected){state.lowBatteryAlert=false;state.timerUntil=0;}
    renderAlerts();renderBridge();
    renderStickButtons();
    renderCalibrationStatus();
    renderZoneCalibration();
    if(!connected&&wasCalibrating){
      $('#calibration-step').textContent='Controller disconnected';
      $('#calibration-instruction').textContent='Reconnect and start the guide again. No incomplete ranges were saved.';
      $('#calibration-status').textContent='Calibration stopped.';
      $('#calibration-fill').style.width='0%';
    }
    renderSession();
  }

  function renderSession(){
    const remaining=Math.max(0,state.leaseUntil-performance.now());
    $('#session-lease').textContent=remaining>0?`${(remaining/1000).toFixed(1)}s`:'Off';
    $('#lease-fill').style.width=remaining>0?`${remaining/state.leaseDuration*100}%`:'0%';
    $('#session-lease').parentElement.classList.toggle('is-active',remaining>0);
    const age=performance.now()-state.lastInputAt;
    const signal=!state.connected?'Waiting':!state.lastInputAt?'Connecting':age>1500?'Stalled':state.fullReport?'Full report':'Basic';
    $('#session-signal').textContent=signal;
    $('#session-signal-dot').classList.toggle('is-live',state.connected&&age<=1500&&!!state.lastInputAt);
  }
  function renderBatteryHistory(){
    const host=$('#battery-history');host.replaceChildren();
    host.parentElement.classList.toggle('is-empty',!state.batteryHistory.length);
    if(!state.batteryHistory.length)return;
    state.batteryHistory.forEach(({time,level,charging})=>{
      const bar=document.createElement('span');bar.className='battery-history-bar'+(charging?' is-charging':'');
      bar.style.height=`${Math.max(8,level)}%`;
      bar.title=`${new Date(time).toLocaleString()} — ${level}%${charging?' (charging)':''}`;
      bar.setAttribute('aria-label',bar.title);host.append(bar);
    });
  }
  try{
    const saved=JSON.parse(localStorage.getItem('aftertouch.batteryHistory')||'[]');
    if(Array.isArray(saved))state.batteryHistory=saved.filter(v=>v&&Number.isFinite(v.time)&&Number.isInteger(v.level)&&v.level>=0&&v.level<=100&&typeof v.charging==='boolean').slice(-24);
  }catch{}
  renderBatteryHistory();
  function recordBattery(){
    if(state.battery==null)return;
    const last=state.batteryHistory.at(-1),now=Date.now();
    if(last&&last.level===state.battery&&last.charging===state.charging&&now-last.time<60*60*1000)return;
    state.batteryHistory.push({time:now,level:state.battery,charging:state.charging});
    state.batteryHistory=state.batteryHistory.slice(-24);
    try{localStorage.setItem('aftertouch.batteryHistory',JSON.stringify(state.batteryHistory));}catch{}
    renderBatteryHistory();
  }
  function chargeRateLabel(){
    if(!state.connected || state.battery==null)return 'Charge rate —';
    if(state.batteryInfo?.chargeState==='full')return 'Fully charged';
    if(!state.charging)return 'Charge rate available while charging';
    const samples=state.batteryHistory;
    let first=samples.length-1;
    while(first>0 && samples[first-1].charging && samples[first].time-samples[first-1].time<6*60*60*1000)first--;
    const run=samples.slice(first).filter(item=>item.charging);
    const start=run.find(item=>item.level<state.battery);
    const end=run.at(-1);
    if(!start||!end)return 'Estimating charge rate · waiting for a level change';
    const hours=(end.time-start.time)/3600000;
    if(hours<.08)return 'Estimating charge rate · more time needed';
    const rate=(end.level-start.level)/hours;
    return rate>0&&rate<120?`Estimated +${Math.round(rate)}%/h · 10% battery steps`:'Estimating charge rate · more readings needed';
  }
  function renderStickButtons(){
    $$('#start-drift,#start-circularity').forEach(b=>b.disabled=!state.connected||!!state.stickTest);
  }
  function startStickTest(kind){
    if(!state.connected||state.stickTest)return;
    const duration=kind==='drift'?5000:10000;
    state.stickTest={kind,until:performance.now()+duration,count:0,sum:[0,0,0,0],max:[0,0],sectors:[Array(12).fill(0),Array(12).fill(0)]};
    $(`#${kind}-result`).textContent=kind==='drift'?'Leave both sticks untouched for five seconds.':'Rotate both sticks around their outer edge for ten seconds.';
    renderStickButtons();
  }
  $('#start-drift').addEventListener('click',()=>startStickTest('drift'));
  $('#start-circularity').addEventListener('click',()=>startStickTest('circularity'));
  function sampleStickTest(){
    const t=state.stickTest;if(!t)return;
    const a=state.axes;t.count++;
    if(t.kind==='drift'){
      for(let i=0;i<4;i++)t.sum[i]+=a[i];
      for(let i=0;i<2;i++)t.max[i]=Math.max(t.max[i],Math.hypot(a[2*i],a[2*i+1]));
    }else{
      for(let i=0;i<2;i++){
        const radius=Math.hypot(a[2*i],a[2*i+1]);
        if(radius<.25)continue;
        const sector=Math.floor((Math.atan2(a[2*i+1],a[2*i])+Math.PI)/(2*Math.PI)*12)%12;
        t.sectors[i][sector]=Math.max(t.sectors[i][sector],radius);
      }
    }
  }
  function updateStickTest(now){
    const t=state.stickTest;if(!t)return;
    const target=$(`#${t.kind}-result`);
    if(now<t.until){target.textContent=`Measuring… ${Math.ceil((t.until-now)/1000)} seconds left. ${t.count} readings collected.`;return;}
    if(t.count<30)target.textContent='Too few reports arrived to complete the test. Check the connection and try again.';
    else if(t.kind==='drift'){
      target.textContent=[0,1].map(i=>{
        const center=Math.hypot(t.sum[i*2]/t.count,t.sum[i*2+1]/t.count)*100;
        return `${i?'Right':'Left'}: center ${center.toFixed(1)}%, largest movement ${(t.max[i]*100).toFixed(1)}%`;
      }).join(' · ') + '. A game may apply its own deadzone.';
    }else{
      target.textContent=t.sectors.map((sectors,i)=>{
        const reached=sectors.filter(v=>v>=.6),spread=reached.length?100*(Math.max(...reached)-Math.min(...reached)):0;
        return `${i?'Right':'Left'}: ${reached.length}/12 directions, ${spread.toFixed(1)}% outer-travel variation`;
      }).join(' · ') + '. Fewer than ten directions usually means the stick needs another full rotation.';
    }
    state.stickTest=null;renderStickButtons();
  }
  function formatAxis(n){return(Math.abs(n)<.005?0:n).toFixed(2);}
  function renderInput(){
    const b=state.buttons,a=state.axes,l=Math.round((b[6]||0)*100),r=Math.round((b[7]||0)*100);
    $$('.map-control,.map-stick').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.button]||0)>.08));
    $$('.button-matrix span').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.matrix]||0)>.5));
    const active=names.filter((_,i)=>b[i]>.5);$('#active-buttons').textContent=active.length?active.join(' · '):'None';
    $('#live-l2').textContent=String(l).padStart(2,'0')+'%';$('#live-r2').textContent=String(r).padStart(2,'0')+'%';
    $('#live-l2-fill').style.width=l+'%';$('#live-r2-fill').style.width=r+'%';
    $('#left-axis').textContent=`${formatAxis(a[0])} / ${formatAxis(a[1])}`;$('#right-axis').textContent=`${formatAxis(a[2])} / ${formatAxis(a[3])}`;
    $('#diag-l2').textContent=(b[6]||0).toFixed(2);$('#diag-r2').textContent=(b[7]||0).toFixed(2);
    $('#left-stick-cap').setAttribute('transform',`translate(${a[0]*38} ${a[1]*38})`);
    $('#right-stick-cap').setAttribute('transform',`translate(${a[2]*38} ${a[3]*38})`);
    $('#map-l2 .map-trigger-motion').setAttribute('transform',`translate(0 ${l*.18})`);
    $('#map-r2 .map-trigger-motion').setAttribute('transform',`translate(0 ${r*.18})`);
    state.touch.forEach((point,i)=>{
      const mark=$(`#map-touch-${i}`);mark.classList.toggle('is-active',!!point);
      if(point){const y=clamp(point.y),inset=y*36;mark.setAttribute('cx',String(480+inset+clamp(point.x)*(507-2*inset)));mark.setAttribute('cy',String(195+y*230));}
    });
    const info=state.batteryInfo;
    const chargeNames={discharging:'Discharging',charging:'Charging',full:'Full',voltageOrTempFault:'Voltage / temp fault',temperatureFault:'Temperature fault',chargingError:'Charging error',unknown:'Unknown'};
    const chargeLabel=info?.chargeState ? (chargeNames[info.chargeState]||info.chargeState) : null;
    const fault=['voltageOrTempFault','temperatureFault','chargingError'].includes(info?.chargeState);
    $('#battery-value').textContent=state.battery!=null?`${state.battery}%`:(fault?'Fault':'—');
    $('#diag-battery').textContent=state.battery!=null?`${state.battery}%`:(fault?'Fault':'—');
    $('#battery-detail').textContent=!info?'—':state.charging?'Charging':(chargeLabel||'Battery');
    $('#battery-charge-rate').textContent=fault?'Charge fault':chargeRateLabel();
    $('#diag-health').textContent=fault?'Charge fault':'Not reported';
    $('#diag-temp').textContent=info?.sensorByte!=null?String(info.sensorByte):'—';
    $('#diag-charge').textContent=chargeLabel||'—';
    const jackParts=[];
    if(info?.headphoneJack)jackParts.push('Headphone');
    if(info?.microphoneJack)jackParts.push('Mic');
    $('#diag-jack').textContent=info?`Jack: ${jackParts.length?jackParts.join(' · '):'None'}`:'Jack: —';
    $('#battery-band')?.classList.toggle('is-fault', !!fault);
    $('#battery-band')?.classList.toggle('is-charging', !!state.charging);
    $('#battery-light').disabled=!state.connected||state.battery==null;
    $$('#battery-segments i').forEach((el,i)=>{
      const full=state.battery!=null&&state.battery>(i*10);
      el.classList.toggle('is-full', full);
      el.classList.toggle('is-charging', full && !!state.charging);
      el.classList.toggle('is-fault', fault);
    });
    state.touch.forEach((point,i)=>{const el=$(`#contact-${i}`);el.classList.toggle('is-active',!!point);if(point){el.style.left=`clamp(15px, ${clamp(point.x)*100}%, calc(100% - 15px))`;el.style.top=`clamp(15px, ${clamp(point.y)*100}%, calc(100% - 15px))`;} $(`#touch-${i+1}-pos`).textContent=point?`${point.x.toFixed(3)} / ${point.y.toFixed(3)}`:'—';});
    state.gyro.forEach((n,i)=>{const k=['x','y','z'][i];$(`#gyro-${k}`).textContent=String(n);const f=clamp(Math.abs(n)/16000)*50;const el=$(`#gyro-${k}-fill`);el.style.width=`${f}%`;el.style.left=n<0?`${50-f}%`:'50%';});
    const activity=clamp(active.length/8+Math.abs(a[0])/4+Math.abs(a[1])/4+Math.abs(a[2])/4+Math.abs(a[3])/4+(b[6]+b[7])/4);
    state.activity.push(activity);state.activity.shift();
    $('#activity-scope').setAttribute('aria-label',`Input activity ${Math.round(activity*100)} percent`);
    updatePlayhead();$('#diag-packet').textContent=`#${String(state.packet%10000).padStart(4,'0')}`;
  }
  $('#battery-segments').innerHTML=Array.from({length:10},()=>'<i></i>').join('');
  function drawActivity(){const canvas=$('#activity-scope'),ctx=canvas.getContext('2d'),w=canvas.width,h=canvas.height;ctx.clearRect(0,0,w,h);ctx.beginPath();state.activity.forEach((v,i)=>{const x=i/(state.activity.length-1)*w,y=h-8-v*(h-16);if(i)ctx.lineTo(x,y);else ctx.moveTo(x,y);});ctx.lineWidth=3;ctx.strokeStyle='#d4a84b';ctx.stroke();ctx.fillStyle='#7ec9bc';const last=state.activity.at(-1);ctx.fillRect(w-5,h-8-last*(h-16)-4,8,8);}
  function handleEvent(event){
    if(event.type==='device'){
      const changed=state.connected!==!!event.connected || (event.connected&&(state.name!==event.name||state.transport!==event.transport||state.path!==(event.path||'')));
      if(changed)padNavPrev=null;
      setConnected(!!event.connected,event.name||'',event.transport||'',event.firmware||'',event.path||'');
      if(changed && event.connected){sendPcBindings();sendCalibration();applyGameRules();}
      if(event.path) state.selectedPath = event.path;
      if(changed||state.manualScan){const msg=event.connected?`${event.name} · ${event.transport}`:(event.message||'Disconnected');log(msg);toast(msg);}
      if(state.manualScan)finishScan();
      renderDeviceList();
    }else if(event.type==='devices'){
      state.devices = Array.isArray(event.items) ? event.items : [];
      state.selectedPath = event.selected || state.selectedPath;
      renderDeviceList();
      if(state.manualScan)finishScan();
    }else if(event.type==='input'){
      sampleCalibration(event.input);
      handlePadNavigation(event.input.buttons);
      state.buttons=event.input.buttons;state.axes=event.input.axes;state.touch=event.input.touch;state.gyro=event.input.gyro;
      if(state.gyroCalibration&&event.input.fullReport&&Array.isArray(event.input.gyro)){
        const c=state.gyroCalibration;c.count++;
        event.input.gyro.forEach((n,i)=>{c.sum[i]+=n;c.sumSq[i]+=n*n;});
      }
      state.battery=event.input.battery;state.charging=event.input.charging;state.batteryInfo=event.input.batteryInfo||null;
      recordBattery();sampleStickTest();
      const reportChanged=state.fullReport!==!!event.input.fullReport;
      state.fullReport=!!event.input.fullReport;
      if(reportChanged)renderGyroControls();
      $('#photo-status').textContent=event.input.fullReport?`${state.transport} live`:'Bluetooth basic';
      state.packet++;state.lastInputAt=performance.now();renderInput();renderSession();
    }else if(event.type==='xinput'){
      state.xinput=!!event.enabled && event.status==='active';
      state.xinputStatus=event.status||'disabled';
      state.xinputMessage=event.message||event.error||'ViGEm not connected';
      renderXinput();
    }else if(event.type==='audioStatus'){
      state.audioRoute=event.route||'off';
      if(typeof event.bridge==='boolean')state.audioBridge=event.bridge;
      const previousBridgeFrames=state.bridgeFrames;
      if(!state.audioBridge)state.bridgeFrames=0;
      else if(Number.isInteger(event.bridgeFrames))state.bridgeFrames=event.bridgeFrames;
      renderBridge();
      if(event.message)$('#bridge-status').textContent=event.message;
      else if(event.reason)$('#bridge-status').textContent=`Motor fallback · ${event.reason}`;
      else if(event.bridge)$('#bridge-status').textContent=event.glitches?`Bridge on · ${event.glitches} audio glitch${event.glitches===1?'':'es'}`:state.bridgeFrames>previousBridgeFrames?'PC audio detected · sending bass to grips':'Bridge on · play audio through your PC speakers';
    }else if(event.type==='microphoneStatus'){
      controllerMicMuted=!!event.muted;
      $('#controller-mic').textContent=`Controller mic: ${controllerMicMuted?'muted':'live'}`;
      $('#controller-mic').setAttribute('aria-pressed',String(controllerMicMuted));
    }else if(event.type==='speakerStatus'){
      log(event.message||'Controller speaker chirp playing');
      if(event.source==='chirp'){$('#speaker-file-status').textContent='Speaker test playing';setTimeout(()=>{if(!speakerStartedAt)$('#speaker-file-status').textContent='Speaker test finished';},1000);}
      if(event.source==='transport'){speakerPaused=!!event.paused;speakerElapsed=event.position||0;speakerStartedAt=speakerPaused?0:performance.now();$('#speaker-file-status').textContent=speakerPaused?'Paused':'Playing';renderSpeakerProgress();}
      if(speakerPending&&event.source==='file'){speakerPending=false;speakerElapsed=0;speakerPaused=false;speakerStartedAt=performance.now();$('#speaker-start').disabled=false;$('#speaker-file-status').textContent='Playing';renderSpeakerProgress();}
    }else if(event.type==='inputEffects'){
      state.inputEffectsActive=!!event.enabled;
      $('#click-status').textContent=event.enabled?'Armed · release R2 to rearm':'Not armed';
    }else if(event.type==='hapticAlertStatus'){
      if(typeof event.lowBattery==='boolean')state.lowBatteryAlert=event.lowBattery;
      if(typeof event.timerSeconds==='number')state.timerUntil=event.timerSeconds?performance.now()+event.timerSeconds*1000:0;
      renderAlerts();
    }else if(event.type==='hapticAlert'){
      toast(event.message||'Haptic alert');log(event.message||'Haptic alert');
    }else if(event.type==='gameAction'){
      $('#game-action-status').textContent=`Played ${event.name}`;
    }else if(event.type==='gameLinkStatus'||event.type==='gameLinkError'){
      $('#game-link-status').textContent=event.message;
    }else if(event.type==='gameEvent'){
      if(event.weapon!=null&&state.connected)useWeapon(event.weapon,true);
      if(event.events?.length)$('#game-action-status').textContent=`Game event: ${event.events.join(', ')}`;
    }else if(event.type==='telemetryStatus'){
      state.telemetryOn=!!event.enabled;
      if(event.port)$('#telemetry-port').value=event.port;
      if(typeof event.lan==='boolean')$('#telemetry-lan').checked=event.lan;
      if(event.enabled&&['forza','json'].includes(event.format)){$('#telemetry-format').value=event.format;renderTelemetryFormat();}
      renderTelemetry();
      $('#telemetry-status').textContent=event.message||'Waiting for telemetry';
    }else if(event.type==='scriptStatus'){
      state.scriptEnabled=!!event.enabled;renderScript();
      $('#script-status').textContent=event.message||'Script off';
      if(!event.enabled)$('#script-output').textContent='Waiting for a signal';
    }else if(event.type==='scriptOutput'){
      const light=Array.isArray(event.light)?'#'+event.light.map(value=>Math.max(0,Math.min(255,value)).toString(16).padStart(2,'0')).join('').toUpperCase():null;
      $('#script-output').textContent=[light,event.cue].filter(Boolean).join(' · ')||'No change';
      if(light){$('#light-arcs').style.setProperty('--light',light);$('#light-hex').textContent=light;}
    }else if(event.type==='telemetry'){
      state.telemetryLast=performance.now();
      $('#telemetry-status').textContent=`Live · ${event.source||'Data Out'}`;
      $('#race-speed').textContent=event.speedKmh==null?'—':Math.round(event.speedKmh);
      $('#race-gear').textContent=event.gear==null?'—':event.gear===0?'R':String(event.gear);
      $('#race-rpm').textContent=Math.round(event.rpm||0).toLocaleString();
      $('#race-source').textContent=event.source||'Data Out';
      $('#race-slip').textContent=event.estimatedBrakeSlip?'Brake slip estimated':'Brake slip clear';
      $('#rev-fill').style.width=`${clamp((event.rpm||0)/(event.maxRpm||1))*100}%`;
      if(Array.isArray(event.revColor)){
        const hex='#'+event.revColor.map(v=>Math.round(v).toString(16).padStart(2,'0')).join('');
        $('#light-arcs').style.setProperty('--light',hex);
        $('#light-hex').textContent=hex.toUpperCase();
      }
    }else if(event.type==='rate'){
      state.rate=event.reportsPerSecond||0;$('#diag-rate').textContent=state.connected?`${state.rate} / S`:'—';
      state.sequenceGaps=event.sequenceGaps||0;state.invalidReports=event.invalidReports||0;
      $('#diag-gaps').textContent=state.connected?String(state.sequenceGaps):'—';
      $('#diag-invalid').textContent=state.connected?String(state.invalidReports):'—';
    }else if(event.type==='conflict'){
      $('#conflict-message').textContent=event.message||'No conflicting tools.';
      $('#conflict-message').style.color=event.detected?'#d4a84b':'';
      if(event.detected) log(`OUTPUT CONFLICT / ${event.message}`);
    }else if(event.type==='foreground'){
      const previous=state.foregroundExe;
      const previousMatch=state.autoMatchId;
      state.foregroundExe=event.exe||'';
      state.foregroundTitle=event.title||'';
      state.foregroundPath=event.path||'';
      if(state.foregroundExe){state.lastExternalExe=state.foregroundExe;state.lastExternalPath=state.foregroundPath;}
      $('#detected-app').textContent=`Last app: ${state.lastExternalExe||'none'}`;
      const match=state.profiles.filter(matchesForeground).sort((a,b)=>(!!b.matchPath+!!b.matchTitle)-(!!a.matchPath+!!a.matchTitle))[0];
      state.autoMatchId=match?.id||'';
      if(previous!==state.foregroundExe||previousMatch!==state.autoMatchId)state.autoSuppressed=false;
      if(match && match.id!==state.profileId){
        state.profileId=match.id;renderAllProfileControls();saveProfiles();
        log(`AUTO LOAD / ${match.name} FOR ${state.foregroundExe}`);
        toast(match.name);
        if (state.autoApply && match.autoApply !== false) applyProfileHardware(false);
      } else if (match && previousMatch!==match.id && state.autoApply && match.autoApply !== false) {
        applyProfileHardware(false);
      }
      if(state.connected)applyGameRules();
    }else if(event.type==='mouseMode'){
      state.mouse=!!event.enabled;renderMouseSwitch();
    }else if(event.type==='pcMode'){
      state.pcMode=!!event.enabled;renderPcMode();if(state.connected)applyGameRules();
    }else if(event.type==='touchGestures'){
      state.gestures=!!event.enabled;renderGestureSwitch();
    }else if(event.type==='gyroAim'){
      state.gyroAim=!!event.enabled;renderGyroControls();
    }else if(event.type==='remap'){
      state.remapArmed=!!event.enabled;
    }else if(event.type==='lightMode'){
      state.batteryLight=!!event.battery;state.temperatureLight=!!event.sensor;state.screenLight=!!event.screen;renderBatteryLight();renderTemperatureLight();renderScreenLight();
    }else if(event.type==='sensorColor'){
      if(Array.isArray(event.rgb)){
        const hex='#'+event.rgb.map(value=>Math.max(0,Math.min(255,value)).toString(16).padStart(2,'0')).join('').toUpperCase();
        $('#light-arcs').style.setProperty('--light',hex);$('#light-hex').textContent=hex;
        $('#temperature-light-status').textContent=`Raw sensor byte ${event.sensorByte} · ${hex}`;
      }
    }else if(event.type==='screenColor'){
      if(Array.isArray(event.rgb)){
        const hex='#'+event.rgb.map(value=>Math.max(0,Math.min(255,value)).toString(16).padStart(2,'0')).join('').toUpperCase();
        $('#light-arcs').style.setProperty('--light',hex);$('#light-hex').textContent=hex;
        $('#screen-light-status').textContent=`Live primary display color · ${hex}`;
      }
    }else if(event.type==='triggerLease'){
      state.leaseDuration=event.active?event.durationMs:0;
      state.leaseUntil=event.active?performance.now()+event.durationMs:0;
      renderSession();
    }else if(event.type==='safetyReset'){
      state.autoSuppressed=true;
      state.pcMode=false;renderPcMode();
      state.leaseUntil=0;
      state.audioBridge=false;state.audioRoute='off';renderBridge();
      state.inputEffectsActive=false;$('#click-status').textContent='Not armed';
      state.lowBatteryAlert=false;state.timerUntil=0;renderAlerts();
      state.telemetryOn=false;renderTelemetry();
      state.temperatureLight=false;state.screenLight=false;renderTemperatureLight();renderScreenLight();
      state.scriptEnabled=false;renderScript();$('#script-status').textContent='Script off';$('#script-output').textContent='Waiting for a signal';
      renderSession();
    }else if(event.type==='core'){
      state.coreStatus = event.status || 'ready';
      if(event.status==='restarting'||event.status==='stopped'){
        state.pcMode=false;renderPcMode();
        state.scriptEnabled=false;renderScript();$('#script-status').textContent='Core restarting · arm the rule again when ready';$('#script-output').textContent='Waiting for a signal';
        state.temperatureLight=false;state.screenLight=false;renderTemperatureLight();renderScreenLight();
      }
      $('#core-status').textContent = event.status === 'restarting' ? 'Core: restarting'
        : event.status === 'stopped' ? 'Core: stopped'
        : 'Core: ready';
      $('#core-status').classList.toggle('is-warn', event.status === 'restarting' || event.status === 'stopped');
      if (event.message) log(event.message);
      if(event.status==='ready')reportUiFocus();
    }else if(event.type==='notice'||event.type==='error'){
      toast(event.message);log(event.message);
    }
  }
  let lastCanvas=0;function frame(now){if(now-lastCanvas>35){drawActivity();if(state.currentView==='haptics'||now<state.playUntil)drawWave(now);renderSession();if(state.currentView==='haptics'&&state.timerUntil)renderAlerts();if(state.currentView==='haptics'&&speakerStartedAt)renderSpeakerProgress(now);updateStickTest(now);if(state.telemetryOn&&state.telemetryLast&&now-state.telemetryLast>1500)$('#telemetry-status').textContent='Listening · waiting for telemetry';lastCanvas=now;}requestAnimationFrame(frame);}requestAnimationFrame(frame);
  const keyMap={j:2,k:0,l:1,i:3,q:4,e:5,z:6,c:7,ArrowUp:12,ArrowDown:13,ArrowLeft:14,ArrowRight:15};
  const held=new Set();window.addEventListener('keydown',e=>{if(state.connected||e.target.matches('input,textarea,button,select'))return;if(!(e.key in keyMap))return;held.add(e.key);state.buttons[keyMap[e.key]]=1;renderInput();});
  window.addEventListener('keyup',e=>{if(!(e.key in keyMap)||state.connected)return;held.delete(e.key);state.buttons[keyMap[e.key]]=0;renderInput();});
  window.addEventListener('blur',()=>{if(state.connected)return;held.clear();state.buttons.fill(0);renderInput();});

  async function start(){
    renderAllProfileControls();renderPcBindings();setLightColor('#d4a84b');setConnected(false);renderInput();renderDeviceList();renderGestureSwitch();renderXinput();navigate(location.hash.slice(1)||'live');
    await loadProfiles();
    if(native){
      try{await window.__TAURI__.event.listen('core-event',e=>handleEvent(e.payload));await window.__TAURI__.event.listen('update-progress',e=>handleUpdateProgress(e.payload));await send({action:'listDevices'});reportUiFocus();log('Core ready');$('#core-status').textContent='Core: ready';}
      catch(error){toast(`Core connection failed: ${error}`);log(String(error));$('#core-status').textContent='Core: error';$('#core-status').classList.add('is-warn');}
      setTimeout(()=>checkForUpdates(),2500);
      setInterval(()=>checkForUpdates(),12*60*60*1000);
    }else{$('#core-status').textContent='Core: browser preview';$('#update-status').textContent='Updates are available in the installed desktop app.';$('#check-update').disabled=true;log('Browser preview');}
  }
  start();
})();
