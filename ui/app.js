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
    {vk: 0x20, label: 'Space'}, {vk: 0x0D, label: 'Enter'}, {vk: 0x1B, label: 'Esc'},
    {vk: 0x25, label: 'Left'}, {vk: 0x26, label: 'Up'}, {vk: 0x27, label: 'Right'}, {vk: 0x28, label: 'Down'},
    {vk: 0x70, label: 'F1'}, {vk: 0x71, label: 'F2'}, {vk: 0x72, label: 'F3'}, {vk: 0x73, label: 'F4'},
    {vk: 0x41, label: 'A'}, {vk: 0x42, label: 'B'}, {vk: 0x43, label: 'C'}, {vk: 0x45, label: 'E'},
    {vk: 0x51, label: 'Q'}, {vk: 0x52, label: 'R'}, {vk: 0x56, label: 'V'}, {vk: 0x58, label: 'X'}, {vk: 0x5A, label: 'Z'}
  ];
  const presets = {
    Linear: [0,.1,.2,.3,.4,.5,.6,.7,.8,.9,1],
    Wall: [0,0,.02,.04,.1,.84,.96,1,1,1,1],
    Break: [0,.02,.05,.08,.14,.22,.92,.19,.2,.2,.2],
    Swell: [0,.02,.06,.13,.22,.34,.48,.65,.8,.92,1],
    Pulse: [.04,.21,.67,.16,.74,.22,.77,.27,.78,.31,.8]
  };
  const defaultGyro = () => ({enabled:false,sensitivity:1,yawScale:1,pitchScale:1,invertY:false,activation:'always',bias:[0,0,0]});
  const defaultRemap = () => ({enabled:false,bindings:[]});
  const defaultInputEffects = () => ({enabled:false,threshold:.82,strength:55});
  const defaultProfiles = [
    {id:'standard',name:'Standard',description:'A gentle starting point: L2 builds resistance evenly, while R2 rises more gradually. Use it to get a feel for the trigger editor before making a game profile.',tag:'GENERAL',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:40,curve:clone(presets.Linear)},right:{mode:'resistance',strength:55,curve:clone(presets.Swell)}},haptics:{shape:'sine',frequency:90,attack:12,release:220,strength:44,pan:0},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()},
    {id:'rally',name:'Rally',description:'L2 has a firm wall for braking. R2 uses a pulsing response for throttle experiments; tune its strength to suit your hands and the game.',tag:'RACING',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:73,curve:clone(presets.Wall)},right:{mode:'vibration',strength:54,curve:clone(presets.Pulse)}},haptics:{shape:'saw',frequency:65,attack:8,release:240,strength:55,pan:-10},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()},
    {id:'arcade',name:'Arcade',description:'Both triggers break sharply for a punchy action feel. This preset is deliberately bold; lower the strength if it feels tiring during longer sessions.',tag:'ACTION',matchExe:'',autoApply:true,triggers:{left:{mode:'weapon',strength:57,curve:clone(presets.Break)},right:{mode:'weapon',strength:70,curve:clone(presets.Break)}},haptics:{shape:'square',frequency:180,attack:3,release:110,strength:39,pan:0},gyroAim:defaultGyro(),remap:defaultRemap(),inputEffects:defaultInputEffects()}
  ];
  const textures = [
    {name:'Soft tick',type:'Light',settings:{shape:'sine',frequency:175,attack:3,release:80,strength:30,pan:0}},
    {name:'Heartbeat',type:'Warm',settings:{shape:'sine',frequency:65,attack:20,release:270,strength:62,pan:0}},
    {name:'Gravel',type:'Grain',settings:{shape:'noise',frequency:78,attack:9,release:340,strength:47,pan:-20}},
    {name:'Impact',type:'Sharp',settings:{shape:'saw',frequency:95,attack:3,release:190,strength:70,pan:0}}
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
    profileId:'standard',profiles:clone(defaultProfiles),side:'left',packet:0,
    mouse:false,gestures:false,gyroAim:false,remapArmed:false,batteryLight:false,
    xinput:false,xinputStatus:'disabled',xinputMessage:'ViGEm not connected',
    activity:Array(80).fill(0),lastInputAt:0,rate:0,devices:[],selectedPath:'',
    currentView:'live',playUntil:0,playStart:0,playDuration:0,lastExternalExe:'',
    leaseUntil:0,leaseDuration:0,fullReport:false,manualScan:false,
    autoApply:true,coreStatus:'ready',foregroundExe:'',foregroundTitle:'',foregroundPath:'',lastExternalPath:'',autoMatchId:'',autoSuppressed:false,
    sequenceGaps:0,invalidReports:0,stickTest:null,batteryHistory:[],audioBridge:false,audioRoute:'off',telemetryOn:false,telemetryLast:0,inputEffectsActive:false,gyroCalibration:null,lowBatteryAlert:false,timerUntil:0
  };
  let toastTimeout, audioContext, dragging = -1, saveTimeout, saveChain=Promise.resolve(), scanTimeout;
  const palettes=['dusk','cinder','tide','verdigris'];
  function setPalette(palette){
    const choice=palettes.includes(palette)?palette:'dusk';
    document.body.dataset.theme=choice;
    $('#theme-select').value=choice;
    try{localStorage.setItem('aftertouch.palette',choice);}catch{}
  }
  try{setPalette(localStorage.getItem('aftertouch.palette')||'dusk');}catch{setPalette('dusk');}
  $('#theme-select').addEventListener('change',e=>setPalette(e.target.value));

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
    n.remap.bindings = Array.isArray(n.remap.bindings) ? n.remap.bindings.map(b=>({button:b.button,vk:b.vk,modifier:b.modifier??null,behavior:b.behavior||'hold',sequence:Array.isArray(b.sequence)?b.sequence:[]})) : [];
    return n;
  }

  function toast(message) {
    const node = $('#toast'); node.textContent = String(message);
    node.classList.add('is-visible'); clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => node.classList.remove('is-visible'), 3400);
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
  function reportUiFocus(){
    if(native)invoke('send_command',{command:{action:'setUiFocused',focused:document.hasFocus()&&!document.hidden}}).catch(()=>{});
  }
  window.addEventListener('focus',reportUiFocus);
  window.addEventListener('blur',reportUiFocus);
  document.addEventListener('visibilitychange',reportUiFocus);
  function navigate(view) {
    if (!$('#' + view)?.classList.contains('screen')) return;
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
  setInterval(() => $('#clock').textContent = timeLabel(),1000); $('#clock').textContent = timeLabel();

  const padPages = ['live','triggers','haptics','touch','lights','profiles','diagnostics'];
  let padNavPrev = null, padFocus = null;
  function clearPadFocus(){
    if(padFocus)padFocus.classList.remove('pad-focused');
    padFocus=null;
  }
  function padControls(){
    const selector='button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex]:not([tabindex="-1"])';
    return [$('#'+state.currentView),$('.masthead'),$('.session-rail')].flatMap(root=>$$(selector,root))
      .filter(el=>el.getClientRects().length && el.getAttribute('type')!=='hidden');
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
        trigger().curve[index]=Math.round(clamp(trigger().curve[index]+(pressed(12)?.02:-.02))*100)/100;
        renderCurve();focusPadControl($(`#curve-points [data-point="${index}"]`));saveProfiles();
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
      return t && modeLabel[t.mode] && Number.isFinite(t.strength) && t.strength>=0 && t.strength<=100 && Array.isArray(t.curve) && t.curve.length===11 && t.curve.every(n=>Number.isFinite(n)&&n>=0&&n<=1);
    }) && p.haptics && ['sine','square','saw','noise'].includes(p.haptics.shape)
      && hapticFields.every(f=>Number.isInteger(p.haptics[f.key]) && p.haptics[f.key]>=f.min && p.haptics[f.key]<=f.max)
      && Number.isFinite(g.sensitivity) && g.sensitivity>=.05 && g.sensitivity<=8
      && Number.isFinite(g.yawScale) && g.yawScale>=0 && g.yawScale<=4
      && Number.isFinite(g.pitchScale) && g.pitchScale>=0 && g.pitchScale<=4
      && ['always','l1','r1','l2','r2','touchpad'].includes(g.activation)
      && (!g.bias || (Array.isArray(g.bias)&&g.bias.length===3&&g.bias.every(n=>Number.isInteger(n)&&Math.abs(n)<=4096)))
      && Number.isFinite(c.threshold) && c.threshold>=.2 && c.threshold<=.95
      && Number.isInteger(c.strength) && c.strength>=0 && c.strength<=100
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
        state.profileId = state.profiles.some(p=>p.id===data.activeId) ? data.activeId : state.profiles[0].id;
        state.autoApply = data.autoApply !== false;
      }
    } catch (error) { log(`PROFILE LOAD / ${error}`); }
    renderAutoApply();
    renderAllProfileControls();
  }
  function persistProfiles() {
    const data={schemaVersion:3,activeId:state.profileId,autoApply:state.autoApply,profiles:clone(state.profiles)};
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
  function renderAllProfileControls() { renderProfiles(); renderTriggerControls(); renderHapticControls(); renderRemapEditor(); renderGyroControls(); }

  async function applyProfileHardware(announce=true) {
    const p = profile();
    if (!state.connected) {
      if (announce) toast('Connect a controller.');
      return false;
    }
    const ok = await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right});
    if (!ok) return false;
    if (!await send({action:'setInputEffects',enabled:!!p.inputEffects?.enabled,threshold:p.inputEffects?.threshold??.82,strength:p.inputEffects?.strength??55})) return false;
    if (p.gyroAim) {
      if (!await send({
        action:'setGyroAim',
        enabled: !!p.gyroAim.enabled,
        sensitivity: Number(p.gyroAim.sensitivity)||1,
        yawScale: Number(p.gyroAim.yawScale)||1,
        pitchScale: Number(p.gyroAim.pitchScale)||1,
        invertY: !!p.gyroAim.invertY,
        activation: p.gyroAim.activation || 'always',
        bias: p.gyroAim.bias || [0,0,0]
      })) return false;
    }
    if (p.remap) {
      if (!await send({
        action:'setRemap',
        enabled: !!p.remap.enabled,
        bindings: (p.remap.bindings||[]).map(b=>({button:+b.button,vk:+b.vk,modifier:b.modifier??null,behavior:b.behavior||'hold',sequence:b.sequence||[]}))
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
    if (profile().triggers.left.mode==='off' && profile().triggers.right.mode==='off') return;
    if (state.leaseUntil>performance.now()) await send({action:'renewTriggers'});
    else await applyProfileHardware(false);
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
    if (state.autoApply && profile().autoApply !== false) await applyProfileHardware(true);
  });
  $('#new-profile').addEventListener('click',() => {
    if(state.profiles.length>=50){toast('50 profile limit.');return;}
    const p=normalizeProfile(clone(profile())); p.id=`profile-${crypto.randomUUID()}`; p.name='Untitled'; p.description='';p.tag='Custom';p.matchExe='';p.matchTitle='';p.matchPath='';state.profiles.unshift(p); state.profileId=p.id;
    renderAllProfileControls(); saveProfiles(); $('#profile-name').focus(); $('#profile-name').select();
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
    const data={schemaVersion:3,activeId:state.profileId,autoApply:state.autoApply,profiles:state.profiles};
    if(native) { try { const path=await invoke('export_profiles',{data}); toast(`Exported to ${path}`); } catch(error){toast(String(error));} }
    else { const blob=new Blob([JSON.stringify(data,null,2)],{type:'application/json'}),url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download='aftertouch-profiles.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000); }
  });
  $('#import-profiles').addEventListener('click',()=>$('#import-file').click());
  $('#import-file').addEventListener('change',async e => {
    const file=e.target.files[0]; if(!file)return;
    try { if(file.size>1_000_000)throw Error('File is too large'); const data=JSON.parse(await file.text());
      if(![1,2,3].includes(data.schemaVersion)||!Array.isArray(data.profiles)||!data.profiles.length||data.profiles.length>50||!data.profiles.every(validProfile))throw Error('Unsupported or invalid profile file');
      if(state.profiles.length+data.profiles.length>50)throw Error('50 profile limit');
      const imported=data.profiles.map(p=>{const c=normalizeProfile(p);c.id=`import-${crypto.randomUUID()}`;c.name=c.name.slice(0,40);c.description=String(c.description||'').slice(0,180);c.tag='IMPORTED';return c;});
      state.profiles.unshift(...imported);state.profileId=imported[0].id;renderAllProfileControls();saveProfiles();toast(`Imported ${imported.length}`);
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
    $('#trigger-strength').value=t.strength; $('#trigger-strength-number').textContent=`${t.strength}%`; fillRange($('#trigger-strength'));renderCurve();
    const c=profile().inputEffects||defaultInputEffects();
    $('#click-enabled').checked=!!c.enabled;
    $('#click-threshold').value=Math.round(c.threshold*100);$('#click-threshold-value').textContent=`${Math.round(c.threshold*100)}%`;
    $('#click-strength').value=c.strength;$('#click-strength-value').textContent=`${c.strength}%`;
    fillRange($('#click-threshold'));fillRange($('#click-strength'));
  }
  $$('.side-tab').forEach(b=>b.addEventListener('click',()=>{state.side=b.dataset.side;renderTriggerControls();}));
  $('#trigger-modes').addEventListener('click',e=>{const b=e.target.closest('[data-mode]');if(!b)return;trigger().mode=b.dataset.mode;renderTriggerControls();saveProfiles();});
  function fillRange(el){const f=(+el.value-(+el.min||0))/((+el.max||100)-(+el.min||0))*100;el.style.setProperty('--fill',`${f}%`);}
  $('#trigger-strength').addEventListener('input',e=>{trigger().strength=+e.target.value;$('#trigger-strength-number').textContent=`${trigger().strength}%`;fillRange(e.target);saveProfiles();});
  $('#curve-presets').innerHTML=Object.keys(presets).map(n=>`<button type="button" data-preset="${n}">${n}</button>`).join('');
  $('#curve-presets').addEventListener('click',e=>{const b=e.target.closest('[data-preset]');if(!b)return;trigger().curve=clone(presets[b.dataset.preset]);renderCurve();saveProfiles();toast(b.dataset.preset);});
  function renderCurve(){
    const svg=$('#curve-svg'),curve=trigger().curve,grid=$('#curve-grid');
    if(!grid.childElementCount){let g='';for(let i=0;i<=10;i++){let x=45+i*67;g+=`<line x1="${x}" y1="20" x2="${x}" y2="315" class="grid-line"/>`;}for(let i=0;i<=5;i++){let y=315-i*56;g+=`<line x1="45" y1="${y}" x2="715" y2="${y}" class="grid-line"/>`;}g+='<line x1="45" y1="20" x2="45" y2="315" class="axis-line"/>';grid.innerHTML=g;}
    const xy=(v,i)=>`${45+i*67},${315-v*280}`;
    const line=curve.map((v,i)=>(i?'L':'M')+xy(v,i)).join(' ');
    $('#curve-line').setAttribute('d',line);$('#curve-fill').setAttribute('d',line+' L715,315 L45,315 Z');
    let steps='';for(let i=0;i<10;i++){const y=315-(curve[i]+curve[i+1])/2*280;steps+=`${i===0?'M':'L'}${45+i*67},${y} L${45+(i+1)*67},${y} `;}
    $('#curve-steps').setAttribute('d',steps);
    $('#curve-points').innerHTML=curve.map((v,i)=>`<circle data-point="${i}" cx="${45+i*67}" cy="${315-v*280}" r="7" tabindex="0" aria-label="Zone ${i+1} force ${Math.round(v*100)} percent"/>`).join('');
    updatePlayhead();
  }
  function updatePlayhead(){const travel=state.buttons[state.side==='left'?6:7]||0;const x=45+travel*670;$('#curve-playhead').setAttribute('x1',x);$('#curve-playhead').setAttribute('x2',x);}
  $('#curve-svg').addEventListener('pointerdown',e=>{const p=e.target.closest('[data-point]');if(!p)return;dragging=+p.dataset.point;$('#curve-svg').setPointerCapture(e.pointerId);e.preventDefault();});
  $('#curve-svg').addEventListener('pointermove',e=>{if(dragging<0)return;const rect=$('#curve-svg').getBoundingClientRect();const svgY=(e.clientY-rect.top)*380/rect.height;trigger().curve[dragging]=Math.round(clamp((315-svgY)/280)*100)/100;renderCurve();});
  function finishCurve(){if(dragging>=0){dragging=-1;saveProfiles();}}$('#curve-svg').addEventListener('pointerup',finishCurve);$('#curve-svg').addEventListener('pointercancel',finishCurve);
  $('#curve-svg').addEventListener('keydown',e=>{const p=e.target.closest('[data-point]');if(!p||!['ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();const i=+p.dataset.point;trigger().curve[i]=Math.round(clamp(trigger().curve[i]+(e.key==='ArrowUp'?.02:-.02))*100)/100;renderCurve();$(`#curve-points [data-point="${i}"]`).focus();saveProfiles();});
  $('#click-enabled').addEventListener('change',e=>{profile().inputEffects.enabled=e.target.checked;saveProfiles();});
  $('#click-threshold').addEventListener('input',e=>{profile().inputEffects.threshold=+e.target.value/100;$('#click-threshold-value').textContent=`${e.target.value}%`;fillRange(e.target);saveProfiles();});
  $('#click-strength').addEventListener('input',e=>{profile().inputEffects.strength=+e.target.value;$('#click-strength-value').textContent=`${e.target.value}%`;fillRange(e.target);saveProfiles();});
  $('#apply-triggers').addEventListener('click',async()=>{const p=profile();if(await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right}))send({action:'setInputEffects',enabled:!!p.inputEffects.enabled,threshold:p.inputEffects.threshold,strength:p.inputEffects.strength});});
  $('#reset-effects').addEventListener('click',()=>send({action:'reset'}));

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
    } else if(native) toast('Connect a controller.');
    try{
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
    else if(!state.audioBridge)status.textContent='Bridge off · USB voice-coil output available on first playback.';
    $('#haptic-route').textContent=!state.connected?'Route: connect a controller':state.audioRoute==='voiceCoil'?'Route: USB voice coils':state.transport==='Bluetooth'?'Route: Bluetooth motors':'Route: USB voice coils if available';
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
  $('#alert-low-battery').addEventListener('click',()=>send({action:'setLowBatteryAlert',enabled:!state.lowBatteryAlert}));
  $('#alert-timer-start').addEventListener('click',()=>send({action:'setHapticTimer',seconds:Number($('#alert-timer-minutes').value)*60}));
  $('#alert-timer-cancel').addEventListener('click',()=>send({action:'setHapticTimer',seconds:0}));

  function setLightColor(hex){$('#light-color').value=hex;$('#light-hex').textContent=hex.toUpperCase();$('#light-arcs').style.setProperty('--light',hex);}
  const lightColors=['#d4a84b','#3d9b8c','#efd9a0','#5b8c7a','#c45c3e','#7ec9bc'];
  $('#light-presets').innerHTML=lightColors.map(c=>`<button type="button" data-color="${c}" style="--color:${c}" aria-label="Color ${c}"></button>`).join('');
  $('#light-presets').addEventListener('click',e=>{const b=e.target.closest('[data-color]');if(b)setLightColor(b.dataset.color);});
  $('#light-color').addEventListener('input',e=>setLightColor(e.target.value));
  $('#send-light').addEventListener('click',()=>{const h=$('#light-color').value;send({action:'setLightbar',rgb:[1,3,5].map(i=>parseInt(h.slice(i,i+2),16))});});
  $('#battery-light').addEventListener('click',()=>send({action:'batteryLightbar',enabled:!state.batteryLight}));
  $('#release-light').addEventListener('click',()=>send({action:'releaseLightbar'}));
  document.querySelector('label[for="telemetry-port"]').insertAdjacentHTML('beforebegin','<label for="telemetry-format">Input format</label><select id="telemetry-format"><option value="forza">Forza Data Out · Dash</option><option value="json">Local JSON adapter</option></select><details id="telemetry-json-help" class="telemetry-schema" hidden><summary>JSON packet format</summary><code>{"rpm":6100,"maxRpm":8000,"speedKmh":108,"gear":4,"brake":150,"slip":1.4,"frontSlip":1.3,"kerb":[true,false,false,false]}</code><p>Send one UTF-8 JSON object per UDP datagram. Keep each under 512 bytes. Fields after maxRpm are optional.</p></details>');
  function renderTelemetryFormat(){
    const json=$('#telemetry-format').value==='json';
    $('.telemetry-panel .panel-cap span').textContent=json?'Local adapter':'Forza Data Out';
    $('#telemetry-json-help').hidden=!json;
    $('.telemetry-intro p').textContent=json
      ?'Receive bounded game signals from a local adapter. Revs drive the lightbar; gear changes, braking slip, tyre slip, and kerbs make short grip cues. Use localhost unless another trusted device sends the packets.'
      :"Set Forza's Data Out format to Dash. Motorsport 2023 can send to 127.0.0.1; for Horizon 4/5 or a console, enable LAN listening and use this PC's IPv4 address in the game. Engine revs color the lightbar; gear changes make a short grip cue. The brake slip reading is an estimate from wheel slip, not a direct ABS signal.";
  }
  $('#telemetry-format').addEventListener('change',()=>{renderTelemetryFormat();if(state.telemetryOn)send({action:'setTelemetry',enabled:true,port:Number($('#telemetry-port').value),lan:$('#telemetry-lan').checked,format:$('#telemetry-format').value});});
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
  function renderTelemetry(){
    $('#telemetry-toggle').textContent=state.telemetryOn?'Stop listening':'Start listening';
    $('#telemetry-toggle').setAttribute('aria-pressed',String(state.telemetryOn));
    if(!state.telemetryOn){$('#telemetry-status').textContent='Off';$('#race-speed').textContent='—';$('#race-gear').textContent='—';$('#race-rpm').textContent='—';$('#race-source').textContent='Waiting for telemetry';$('#race-slip').textContent='Brake slip —';$('#rev-fill').style.width='0%';}
  }
  function renderBatteryLight(){const b=$('#battery-light');b.textContent=`Battery color: ${state.batteryLight?'on':'off'}`;b.setAttribute('aria-pressed',String(state.batteryLight));b.classList.toggle('is-active',state.batteryLight);}
  $('#touch-mouse-toggle').addEventListener('click',()=>send({action:'setTouchMouse',enabled:!state.mouse}));
  function renderMouseSwitch(){const b=$('#touch-mouse-toggle');b.classList.toggle('is-active',state.mouse);b.setAttribute('aria-pressed',String(state.mouse));b.textContent=state.mouse?'On':'Off';renderGestureSwitch();}
  $('#touch-gesture-toggle').addEventListener('click',()=>send({action:'setTouchGestures',enabled:!state.gestures}));
  function renderGestureSwitch(){const b=$('#touch-gesture-toggle');b.classList.toggle('is-active',state.gestures&&state.mouse);b.setAttribute('aria-pressed',String(state.gestures));b.disabled=!state.connected||!state.mouse;b.textContent=state.gestures?(state.mouse?'On':'Paused'):'Off';}

  function syncGyroCommand() {
    const g = profile().gyroAim;
    g.sensitivity = (+$('#gyro-sens').value) / 10;
    g.activation = $('#gyro-activation').value;
    g.invertY = $('#gyro-invert').checked;
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
        bias: g.bias || [0,0,0]
      });
    }
  }
  function renderGyroControls() {
    const g = profile().gyroAim || defaultGyro();
    $('#gyro-sens').value = Math.round((g.sensitivity || 1) * 10);
    $('#gyro-sens-value').textContent = `${Number(g.sensitivity||1).toFixed(1)}×`;
    $('#gyro-activation').value = g.activation || 'always';
    $('#gyro-invert').checked = !!g.invertY;
    fillRange($('#gyro-sens'));
    const b = $('#gyro-aim-toggle');
    b.disabled=!state.connected||!state.fullReport;
    b.classList.toggle('is-active', state.gyroAim);
    b.setAttribute('aria-pressed', String(state.gyroAim));
    b.textContent = state.gyroAim ? 'On' : 'Off';
    $('#gyro-calibrate').disabled=!state.connected||!state.fullReport||!!state.gyroCalibration;
    if(!state.gyroCalibration)$('#gyro-calibration-status').textContent=(g.bias||[]).some(n=>n!==0)?`Saved still bias: ${(g.bias||[]).join(' / ')}. Calibrate again if the pad drifts.`:'Place the pad on a steady surface for two seconds. Calibration is saved with this profile.';
  }
  $('#gyro-aim-toggle').addEventListener('click', () => {
    if (!state.connected) { toast('Connect a controller.'); return; }
    state.gyroAim = !state.gyroAim;
    syncGyroCommand();
    renderGyroControls();
  });
  $('#gyro-sens').addEventListener('input', () => { fillRange($('#gyro-sens')); $('#gyro-sens-value').textContent = `${(+$('#gyro-sens').value/10).toFixed(1)}×`; });
  $('#gyro-sens').addEventListener('change', syncGyroCommand);
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

  $('#diag-reset').addEventListener('click',()=>send({action:'reset'}));
  $('#quick-reset').addEventListener('click',()=>send({action:'reset'}));
  function finishScan(){clearTimeout(scanTimeout);state.manualScan=false;$('#scan-button').disabled=false;$('#scan-button').textContent='Scan';}
  async function manualScan(){
    state.manualScan=true;$('#scan-button').disabled=true;$('#scan-button').textContent='Scanning…';
    scanTimeout=setTimeout(finishScan,4000);
    if(!await send({action:'listDevices'}))finishScan();
  }
  $('#diag-scan').addEventListener('click',manualScan);
  $('#scan-button').addEventListener('click',manualScan);
  $('#guide-scan').addEventListener('click',manualScan);
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
    state.connected=connected;state.name=name;state.transport=transport;state.path=connected?path:'';
    $('#side-dot').parentElement.classList.toggle('is-connected',connected);
    $('#side-state').textContent=connected?'Connected':'Searching';
    $('#top-transport').textContent=connected?transport:'No device';
    $('#photo-status').textContent=connected?`${transport} live`:'Waiting';
    $('#device-name').textContent=connected?name:'No controller';
    $('#device-subtitle').textContent=connected?`${transport} stream`:'USB or Bluetooth';
    $('#connection-guide').hidden=connected;
    $('#diag-device-name').textContent=connected?name:'Waiting';
    $('#diag-transport').textContent=connected?`${transport} / 054C`:'USB / Bluetooth';
    $('#diag-firmware').textContent=connected?(firmware||'Firmware unknown'):'Firmware —';
    $('#owner-status').textContent=connected?'Core writer':'No pad';
    $('#owner-square').classList.toggle('is-live',connected);
    $$('#apply-triggers,#reset-effects,#touch-mouse-toggle,#gyro-aim-toggle,#xinput-toggle,#send-light,#release-light,#diag-reset,#quick-reset,#apply-profile-now,#alert-ping,#alert-low-battery,#alert-timer-start').forEach(b=>b.disabled=!connected);
    if(!connected){clearPadFocus();if(state.gyroCalibration)clearTimeout(state.gyroCalibration.timer);state.gyroCalibration=null;state.buttons.fill(0);state.axes.fill(0);state.touch=[null,null];state.battery=null;state.batteryInfo=null;state.charging=false;state.mouse=false;state.gestures=false;state.gyroAim=false;state.remapArmed=false;state.inputEffectsActive=false;$('#click-status').textContent='Not armed';state.batteryLight=false;state.audioBridge=false;state.audioRoute='off';state.xinput=false;state.xinputStatus='disabled';state.xinputMessage='ViGEm not connected';state.leaseUntil=0;state.lastInputAt=0;state.fullReport=false;state.activity.fill(0);state.stickTest=null;$('#diag-rate').textContent='—';$('#diag-gaps').textContent='—';$('#diag-invalid').textContent='—';renderMouseSwitch();renderGyroControls();renderXinput();renderBatteryLight();renderInput();}
    if(!connected){state.lowBatteryAlert=false;state.timerUntil=0;}
    renderAlerts();renderBridge();
    renderStickButtons();
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
    if(!state.batteryHistory.length){host.textContent='Readings appear here after the controller reports battery level.';return;}
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
    $$('.hotspot,.shoulder-mark').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.button]||0)>.08));
    $$('.button-matrix span').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.matrix]||0)>.5));
    const active=names.filter((_,i)=>b[i]>.5);$('#active-buttons').textContent=active.length?active.join(' · '):'None';
    $('#live-l2').textContent=String(l).padStart(2,'0')+'%';$('#live-r2').textContent=String(r).padStart(2,'0')+'%';
    $('#live-l2-fill').style.width=l+'%';$('#live-r2-fill').style.width=r+'%';
    $('#left-axis').textContent=`${formatAxis(a[0])} / ${formatAxis(a[1])}`;$('#right-axis').textContent=`${formatAxis(a[2])} / ${formatAxis(a[3])}`;
    $('#diag-l2').textContent=(b[6]||0).toFixed(2);$('#diag-r2').textContent=(b[7]||0).toFixed(2);
    $('#left-stick-mark i').style.transform=`translate(${a[0]*8}px,${a[1]*8}px)`;
    $('#right-stick-mark i').style.transform=`translate(${a[2]*8}px,${a[3]*8}px)`;
    const info=state.batteryInfo;
    const healthNames={good:'Good',warm:'Warm',hot:'Hot',cold:'Cold',fault:'Fault',unknown:'Unknown'};
    const chargeNames={discharging:'Discharging',charging:'Charging',full:'Full',voltageOrTempFault:'Voltage / temp fault',temperatureFault:'Temperature fault',chargingError:'Charging error',unknown:'Unknown'};
    const healthLabel=info?.health ? (healthNames[info.health]||info.health) : null;
    const chargeLabel=info?.chargeState ? (chargeNames[info.chargeState]||info.chargeState) : null;
    const fault=info?.health==='fault';
    $('#battery-value').textContent=state.battery!=null?`${state.battery}%`:(fault?'Fault':'—');
    $('#diag-battery').textContent=state.battery!=null?`${state.battery}%`:(fault?'Fault':'—');
    $('#battery-detail').textContent=!info?'—':fault?(info.healthDetail||'Charge fault'):state.charging?'Charging':(chargeLabel||'Battery');
    $('#battery-health').textContent=healthLabel?`Condition ${healthLabel}`:'Condition —';
    $('#battery-health').dataset.health=info?.health||'unknown';
    $('#battery-temp').textContent=info?.temperatureC!=null?`Temp ${info.temperatureC} °C`:'Temp —';
    $('#diag-health').textContent=healthLabel||'—';
    $('#diag-temp').textContent=info?.temperatureC!=null?`${info.temperatureC}°`:'—';
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
    state.touch.forEach((point,i)=>{const el=$(`#contact-${i}`);el.classList.toggle('is-active',!!point);if(point){el.style.left=`${point.x*100}%`;el.style.top=`${point.y*100}%`;} $(`#touch-${i+1}-pos`).textContent=point?`${point.x.toFixed(3)} / ${point.y.toFixed(3)}`:'—';});
    state.gyro.forEach((n,i)=>{const k=['x','y','z'][i];$(`#gyro-${k}`).textContent=String(n);const f=clamp(Math.abs(n)/16000)*50;const el=$(`#gyro-${k}-fill`);el.style.width=`${f}%`;el.style.left=n<0?`${50-f}%`:'50%';});
    const activity=(active.length/8+Math.abs(a[0])/4+Math.abs(a[1])/4+(b[6]+b[7])/4);state.activity.push(clamp(activity));state.activity.shift();
    updatePlayhead();$('#packet-counter').textContent=String(state.packet%10000).padStart(4,'0');$('#diag-packet').textContent=`#${String(state.packet%10000).padStart(4,'0')}`;
  }
  $('#battery-segments').innerHTML=Array.from({length:10},()=>'<i></i>').join('');
  function drawActivity(){const canvas=$('#activity-scope'),ctx=canvas.getContext('2d'),w=canvas.width,h=canvas.height;ctx.clearRect(0,0,w,h);ctx.beginPath();state.activity.forEach((v,i)=>{const x=i/(state.activity.length-1)*w,y=h-8-v*(h-16);if(i)ctx.lineTo(x,y);else ctx.moveTo(x,y);});ctx.lineWidth=3;ctx.strokeStyle='#d4a84b';ctx.stroke();ctx.fillStyle='#7ec9bc';const last=state.activity.at(-1);ctx.fillRect(w-5,h-8-last*(h-16)-4,8,8);}
  function handleEvent(event){
    if(event.type==='device'){
      const changed=state.connected!==!!event.connected || (event.connected&&(state.name!==event.name||state.transport!==event.transport||state.path!==(event.path||'')));
      if(changed)padNavPrev=null;
      setConnected(!!event.connected,event.name||'',event.transport||'',event.firmware||'',event.path||'');
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
      renderBridge();
      if(event.message)$('#bridge-status').textContent=event.message;
      else if(event.reason)$('#bridge-status').textContent=`Motor fallback · ${event.reason}`;
    }else if(event.type==='inputEffects'){
      state.inputEffectsActive=!!event.enabled;
      $('#click-status').textContent=event.enabled?'Armed · release R2 to rearm':'Not armed';
    }else if(event.type==='hapticAlertStatus'){
      if(typeof event.lowBattery==='boolean')state.lowBatteryAlert=event.lowBattery;
      if(typeof event.timerSeconds==='number')state.timerUntil=event.timerSeconds?performance.now()+event.timerSeconds*1000:0;
      renderAlerts();
    }else if(event.type==='hapticAlert'){
      toast(event.message||'Haptic alert');log(event.message||'Haptic alert');
    }else if(event.type==='telemetryStatus'){
      state.telemetryOn=!!event.enabled;
      if(event.port)$('#telemetry-port').value=event.port;
      if(typeof event.lan==='boolean')$('#telemetry-lan').checked=event.lan;
      if(event.enabled&&['forza','json'].includes(event.format)){$('#telemetry-format').value=event.format;renderTelemetryFormat();}
      renderTelemetry();
      $('#telemetry-status').textContent=event.message||'Waiting for telemetry';
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
    }else if(event.type==='mouseMode'){
      state.mouse=!!event.enabled;renderMouseSwitch();
    }else if(event.type==='touchGestures'){
      state.gestures=!!event.enabled;renderGestureSwitch();
    }else if(event.type==='gyroAim'){
      state.gyroAim=!!event.enabled;renderGyroControls();
    }else if(event.type==='remap'){
      state.remapArmed=!!event.enabled;
    }else if(event.type==='lightMode'){
      state.batteryLight=!!event.battery;renderBatteryLight();
    }else if(event.type==='triggerLease'){
      state.leaseDuration=event.active?event.durationMs:0;
      state.leaseUntil=event.active?performance.now()+event.durationMs:0;
      renderSession();
    }else if(event.type==='safetyReset'){
      state.autoSuppressed=true;
      state.leaseUntil=0;
      state.audioBridge=false;state.audioRoute='off';renderBridge();
      state.inputEffectsActive=false;$('#click-status').textContent='Not armed';
      state.lowBatteryAlert=false;state.timerUntil=0;renderAlerts();
      state.telemetryOn=false;renderTelemetry();
      renderSession();
    }else if(event.type==='core'){
      state.coreStatus = event.status || 'ready';
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
  let lastCanvas=0;function frame(now){if(now-lastCanvas>35){drawActivity();if(state.currentView==='haptics'||now<state.playUntil)drawWave(now);renderSession();if(state.currentView==='haptics'&&state.timerUntil)renderAlerts();updateStickTest(now);if(state.telemetryOn&&state.telemetryLast&&now-state.telemetryLast>1500)$('#telemetry-status').textContent='Listening · waiting for telemetry';lastCanvas=now;}requestAnimationFrame(frame);}requestAnimationFrame(frame);
  const keyMap={j:2,k:0,l:1,i:3,q:4,e:5,z:6,c:7,ArrowUp:12,ArrowDown:13,ArrowLeft:14,ArrowRight:15};
  const held=new Set();window.addEventListener('keydown',e=>{if(state.connected||e.target.matches('input,textarea,button,select'))return;if(!(e.key in keyMap))return;held.add(e.key);state.buttons[keyMap[e.key]]=1;renderInput();});
  window.addEventListener('keyup',e=>{if(!(e.key in keyMap)||state.connected)return;held.delete(e.key);state.buttons[keyMap[e.key]]=0;renderInput();});
  window.addEventListener('blur',()=>{if(state.connected)return;held.clear();state.buttons.fill(0);renderInput();});

  async function start(){
    renderAllProfileControls();setLightColor('#d4a84b');setConnected(false);renderInput();renderDeviceList();renderGestureSwitch();renderXinput();navigate(location.hash.slice(1)||'live');
    await loadProfiles();
    if(native){
      try{await window.__TAURI__.event.listen('core-event',e=>handleEvent(e.payload));await send({action:'listDevices'});reportUiFocus();log('Core ready');$('#core-status').textContent='Core: ready';}
      catch(error){toast(`Core connection failed: ${error}`);log(String(error));$('#core-status').textContent='Core: error';$('#core-status').classList.add('is-warn');}
    }else{$('#core-status').textContent='Core: browser preview';log('Browser preview');}
  }
  start();
})();
