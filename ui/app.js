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
  const defaultGyro = () => ({enabled:false,sensitivity:1,yawScale:1,pitchScale:1,invertY:false,activation:'always'});
  const defaultRemap = () => ({enabled:false,bindings:[]});
  const defaultProfiles = [
    {id:'standard',name:'Standard study',description:'A measured everyday response.',tag:'GENERAL',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:40,curve:clone(presets.Linear)},right:{mode:'resistance',strength:55,curve:clone(presets.Swell)}},haptics:{shape:'sine',frequency:90,attack:12,release:220,strength:44,pan:0},gyroAim:defaultGyro(),remap:defaultRemap()},
    {id:'rally',name:'Rally night',description:'Firm braking, light throttle texture.',tag:'RACING',matchExe:'',autoApply:true,triggers:{left:{mode:'resistance',strength:73,curve:clone(presets.Wall)},right:{mode:'vibration',strength:54,curve:clone(presets.Pulse)}},haptics:{shape:'saw',frequency:65,attack:8,release:240,strength:55,pan:-10},gyroAim:defaultGyro(),remap:defaultRemap()},
    {id:'arcade',name:'Arcade reflex',description:'A sharp defined break on both pulls.',tag:'ACTION',matchExe:'',autoApply:true,triggers:{left:{mode:'weapon',strength:57,curve:clone(presets.Break)},right:{mode:'weapon',strength:70,curve:clone(presets.Break)}},haptics:{shape:'square',frequency:180,attack:3,release:110,strength:39,pan:0},gyroAim:defaultGyro(),remap:defaultRemap()}
  ];
  const textures = [
    {name:'Soft tick',type:'01 / LIGHT',settings:{shape:'sine',frequency:175,attack:3,release:80,strength:30,pan:0}},
    {name:'Heartbeat',type:'02 / WARM',settings:{shape:'sine',frequency:65,attack:20,release:270,strength:62,pan:0}},
    {name:'Gravel',type:'03 / GRAIN',settings:{shape:'noise',frequency:78,attack:9,release:340,strength:47,pan:-20}},
    {name:'Impact',type:'04 / SHARP',settings:{shape:'saw',frequency:95,attack:3,release:190,strength:70,pan:0}}
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
    buttons:Array(names.length).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],
    profileId:'standard',profiles:clone(defaultProfiles),side:'left',packet:0,
    mouse:false,gestures:false,gyroAim:false,remapArmed:false,batteryLight:false,
    activity:Array(80).fill(0),lastInputAt:0,rate:0,devices:[],selectedPath:'',
    currentView:'live',playUntil:0,playStart:0,playDuration:0,lastExternalExe:'',
    leaseUntil:0,leaseDuration:0,fullReport:false,manualScan:false,
    autoApply:true,coreStatus:'ready'
  };
  let toastTimeout, audioContext, dragging = -1, saveTimeout, saveChain=Promise.resolve(), scanTimeout;

  const profile = () => state.profiles.find(p => p.id === state.profileId) || state.profiles[0];
  const trigger = () => profile().triggers[state.side];
  const timeLabel = () => new Date().toLocaleTimeString('en-US',{hour12:false});
  const modeLabel = {off:'OFF',resistance:'RESISTANCE',weapon:'WEAPON CLICK',vibration:'VIBRATION'};

  function normalizeProfile(p) {
    const n = clone(p);
    n.matchExe = n.matchExe || n.match_exe || '';
    delete n.match_exe;
    n.autoApply = n.autoApply !== false;
    n.gyroAim = Object.assign(defaultGyro(), n.gyroAim || {});
    n.remap = Object.assign(defaultRemap(), n.remap || {});
    n.remap.bindings = Array.isArray(n.remap.bindings) ? n.remap.bindings : [];
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
    if (!native) { toast('Hardware controls are available in the Tauri desktop app.'); return false; }
    try { await invoke('send_command',{command}); return true; }
    catch (error) { toast(String(error)); log(`CORE ERROR / ${error}`); return false; }
  }
  function navigate(view) {
    if (!$('#' + view)?.classList.contains('screen')) return;
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

  function validProfile(p) {
    return p && typeof p.id==='string' && p.id.length>0 && p.id.length<=80
      && typeof p.name === 'string' && p.name.trim().length>0 && p.name.length<=40
      && typeof p.description==='string' && p.description.length<=180 && typeof p.tag==='string' && p.tag.length<=40
      && (!p.matchExe || (typeof p.matchExe==='string' && p.matchExe.length<=80 && !/[\\/]/.test(p.matchExe)))
      && (!p.match_exe || (typeof p.match_exe==='string' && p.match_exe.length<=80 && !/[\\/]/.test(p.match_exe))) && p.triggers && ['left','right'].every(side => {
      const t = p.triggers[side];
      return t && modeLabel[t.mode] && Number.isFinite(t.strength) && t.strength>=0 && t.strength<=100 && Array.isArray(t.curve) && t.curve.length===11 && t.curve.every(n=>Number.isFinite(n)&&n>=0&&n<=1);
    }) && p.haptics && ['sine','square','saw','noise'].includes(p.haptics.shape)
      && hapticFields.every(f=>Number.isInteger(p.haptics[f.key]) && p.haptics[f.key]>=f.min && p.haptics[f.key]<=f.max);
  }
  async function loadProfiles() {
    try {
      const data = native ? await invoke('load_profiles') : JSON.parse(localStorage.getItem('aftertouch.profiles.v2') || 'null');
      if (data && (data.schemaVersion === 1 || data.schemaVersion === 2) && Array.isArray(data.profiles) && data.profiles.length && data.profiles.every(validProfile)) {
        state.profiles = data.profiles.map(normalizeProfile);
        state.profileId = state.profiles.some(p=>p.id===data.activeId) ? data.activeId : state.profiles[0].id;
        state.autoApply = data.autoApply !== false;
      }
    } catch (error) { log(`PROFILE LOAD / ${error}`); }
    renderAutoApply();
    renderAllProfileControls();
  }
  function persistProfiles() {
    const data={schemaVersion:2,activeId:state.profileId,autoApply:state.autoApply,profiles:clone(state.profiles)};
    saveChain=saveChain.catch(()=>{}).then(async()=>{
      if(native) await invoke('save_profiles',{data});
      else localStorage.setItem('aftertouch.profiles.v2',JSON.stringify(data));
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
      if (announce) toast('Connect a controller to apply this profile.');
      return false;
    }
    const ok = await send({action:'applyTriggers',left:p.triggers.left,right:p.triggers.right});
    if (!ok) return false;
    if (p.gyroAim) {
      await send({
        action:'setGyroAim',
        enabled: !!p.gyroAim.enabled,
        sensitivity: Number(p.gyroAim.sensitivity)||1,
        yawScale: Number(p.gyroAim.yawScale)||1,
        pitchScale: Number(p.gyroAim.pitchScale)||1,
        invertY: !!p.gyroAim.invertY,
        activation: p.gyroAim.activation || 'always'
      });
    }
    if (p.remap) {
      await send({
        action:'setRemap',
        enabled: !!p.remap.enabled,
        bindings: (p.remap.bindings||[]).map(b=>({button:+b.button,vk:+b.vk}))
      });
    }
    if (announce) toast(`Applied ${p.name} to hardware`);
    log(`AUTO APPLY / ${p.name}`);
    return true;
  }

  function renderAutoApply() {
    const b = $('#auto-apply-toggle');
    b.classList.toggle('is-active', state.autoApply);
    b.setAttribute('aria-pressed', String(state.autoApply));
    $('strong', b).textContent = state.autoApply ? 'ON' : 'OFF';
  }
  $('#auto-apply-toggle').addEventListener('click', () => {
    state.autoApply = !state.autoApply;
    renderAutoApply();
    saveProfiles(true);
    toast(state.autoApply ? 'Auto-apply enabled: matched profiles push triggers to the pad' : 'Auto-apply disabled: profiles only change the editor');
  });

  function renderProfiles() {
    $('#session-profile-name').textContent=profile().name.toUpperCase();
    $('#profile-count').textContent = `${String(state.profiles.length).padStart(2,'0')} SAVED`;
    const list=$('#profile-list'); list.replaceChildren();
    state.profiles.forEach((p,i) => {
      const b=document.createElement('button'); b.className='profile-row'+(p.id===state.profileId?' is-active':''); b.type='button'; b.dataset.profile=p.id;
      b.innerHTML=`<span class="number">${String(i+1).padStart(2,'0')}</span><span><strong></strong><small></small></span><span class="arrow">↗</span>`;
      $('strong',b).textContent=p.name; $('small',b).textContent=p.tag||'CUSTOM PROFILE'; list.append(b);
    });
    $('#profile-name').value=profile().name; $('#profile-description').value=profile().description||''; $('#profile-match').value=profile().matchExe||'';
    $('#profile-auto-apply').checked = profile().autoApply !== false;
    $('#remap-enabled').checked = !!profile().remap?.enabled;
  }
  $('#profile-list').addEventListener('click',async e => {
    const b=e.target.closest('[data-profile]'); if(!b)return;
    state.profileId=b.dataset.profile; renderAllProfileControls(); saveProfiles();
    toast(`Loaded ${profile().name}`);
    if (state.autoApply && profile().autoApply !== false) await applyProfileHardware(true);
  });
  $('#new-profile').addEventListener('click',() => {
    if(state.profiles.length>=50){toast('The archive holds up to 50 profiles.');return;}
    const p=normalizeProfile(clone(profile())); p.id=`profile-${crypto.randomUUID()}`; p.name='Untitled signal'; p.description=''; p.tag='CUSTOM'; p.matchExe=''; state.profiles.unshift(p); state.profileId=p.id;
    renderAllProfileControls(); saveProfiles(); $('#profile-name').focus(); $('#profile-name').select();
  });
  $('#duplicate-profile').addEventListener('click',() => {
    if(state.profiles.length>=50){toast('The archive holds up to 50 profiles.');return;}
    const p=normalizeProfile(clone(profile())); p.id=`profile-${crypto.randomUUID()}`; p.name=(p.name+' copy').slice(0,40); p.tag='CUSTOM'; p.matchExe=''; state.profiles.unshift(p); state.profileId=p.id;
    renderAllProfileControls(); saveProfiles(); toast('Profile duplicated');
  });
  $('#delete-profile').addEventListener('click',() => {
    if(state.profiles.length===1){toast('Keep at least one profile.');return;}
    state.profiles=state.profiles.filter(p=>p.id!==state.profileId); state.profileId=state.profiles[0].id; renderAllProfileControls();saveProfiles();toast('Profile deleted');
  });
  function commitProfileDraft(announce=false) {
    const name=$('#profile-name').value.trim(); if(!name){toast('Enter a profile name.');return;}
    const matchExe=$('#profile-match').value.trim().toLowerCase();
    if(matchExe && (!/^[\w. -]+\.exe$/.test(matchExe)||matchExe.length>80)){toast('Enter an executable name such as game.exe.');return;}
    profile().name=name.slice(0,40); profile().description=$('#profile-description').value.trim().slice(0,180); profile().matchExe=matchExe;
    profile().autoApply = $('#profile-auto-apply').checked;
    profile().remap.enabled = $('#remap-enabled').checked;
    const row=$('.profile-row.is-active strong');if(row)row.textContent=profile().name;
    $('#session-profile-name').textContent=profile().name.toUpperCase();
    $('#save-profile').textContent='SAVE PROFILE';
    saveProfiles(true).then(saved=>{if(saved&&announce)toast('Profile saved to this computer');});
  }
  $('#save-profile').addEventListener('click',()=>commitProfileDraft(true));
  $('#apply-profile-now').addEventListener('click',()=>{commitProfileDraft(false);applyProfileHardware(true);});
  $$('#profile-name,#profile-description,#profile-match').forEach(el=>{
    el.addEventListener('input',()=>{$('#save-profile').textContent='SAVE PROFILE •';});
    el.addEventListener('change',()=>commitProfileDraft(false));
  });
  $('#profile-auto-apply').addEventListener('change',()=>commitProfileDraft(false));
  $('#remap-enabled').addEventListener('change',()=>commitProfileDraft(false));
  $('#learn-process').addEventListener('click',()=>{if(!state.lastExternalExe){toast('Open the game once, then return here.');return;}$('#profile-match').value=state.lastExternalExe;commitProfileDraft(true);});
  $('#export-profiles').addEventListener('click',async () => {
    const data={schemaVersion:2,activeId:state.profileId,autoApply:state.autoApply,profiles:state.profiles};
    if(native) { try { const path=await invoke('export_profiles',{data}); toast(`Exported to ${path}`); } catch(error){toast(String(error));} }
    else { const blob=new Blob([JSON.stringify(data,null,2)],{type:'application/json'}),url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download='aftertouch-profiles.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000); }
  });
  $('#import-profiles').addEventListener('click',()=>$('#import-file').click());
  $('#import-file').addEventListener('change',async e => {
    const file=e.target.files[0]; if(!file)return;
    try { if(file.size>1_000_000)throw Error('File is too large'); const data=JSON.parse(await file.text());
      if(!(data.schemaVersion===1||data.schemaVersion===2)||!Array.isArray(data.profiles)||!data.profiles.length||data.profiles.length>50||!data.profiles.every(validProfile))throw Error('Unsupported or invalid profile file');
      if(state.profiles.length+data.profiles.length>50)throw Error('The archive holds up to 50 profiles');
      const imported=data.profiles.map(p=>{const c=normalizeProfile(p);c.id=`import-${crypto.randomUUID()}`;c.name=c.name.slice(0,40);c.description=String(c.description||'').slice(0,180);c.tag='IMPORTED';return c;});
      state.profiles.unshift(...imported);state.profileId=imported[0].id;renderAllProfileControls();saveProfiles();toast(`${imported.length} profiles imported`);
    } catch(error){toast(`Import failed: ${error.message}`);} e.target.value='';
  });

  function renderRemapEditor() {
    const box = $('#remap-bindings'); box.replaceChildren();
    const bindings = profile().remap.bindings;
    if (!bindings.length) {
      box.innerHTML = '<p class="remap-empty">No bindings yet. Add a button → key pair.</p>';
      return;
    }
    bindings.forEach((binding, index) => {
      const row = document.createElement('div');
      row.className = 'remap-row';
      const btnSelect = document.createElement('select');
      names.forEach((name, i) => {
        const opt = document.createElement('option');
        opt.value = String(i); opt.textContent = name;
        if (i === +binding.button) opt.selected = true;
        btnSelect.append(opt);
      });
      const keySelect = document.createElement('select');
      vkChoices.forEach(choice => {
        const opt = document.createElement('option');
        opt.value = String(choice.vk); opt.textContent = choice.label;
        if (choice.vk === +binding.vk) opt.selected = true;
        keySelect.append(opt);
      });
      const remove = document.createElement('button');
      remove.type = 'button'; remove.textContent = '✕'; remove.className = 'remap-remove';
      btnSelect.addEventListener('change', () => { binding.button = +btnSelect.value; saveProfiles(); });
      keySelect.addEventListener('change', () => { binding.vk = +keySelect.value; saveProfiles(); });
      remove.addEventListener('click', () => { profile().remap.bindings.splice(index, 1); renderRemapEditor(); saveProfiles(); });
      row.append(btnSelect, keySelect, remove);
      box.append(row);
    });
  }
  $('#add-remap').addEventListener('click', () => {
    if (profile().remap.bindings.length >= 12) { toast('Up to 12 remap bindings per profile.'); return; }
    profile().remap.bindings.push({button:0, vk:0x20});
    renderRemapEditor(); saveProfiles();
  });

  function renderTriggerControls() {
    const t=trigger();
    $$('.side-tab').forEach(b=>b.classList.toggle('is-active',b.dataset.side===state.side));
    $('#curve-side-label').textContent=state.side==='left'?'L2 / LEFT CHANNEL':'R2 / RIGHT CHANNEL';
    $('#trigger-modes').innerHTML=Object.entries(modeLabel).map(([key,label])=>`<button class="mode-button ${t.mode===key?'is-active':''}" data-mode="${key}" type="button"><span>${label}</span><small>${key==='off'?'CLEAN TRAVEL':key==='resistance'?'RISING PRESSURE':key==='weapon'?'TACTILE BREAK':'RHYTHMIC FORCE'}</small></button>`).join('');
    $('#trigger-strength').value=t.strength; $('#trigger-strength-number').textContent=`${t.strength}%`; fillRange($('#trigger-strength'));renderCurve();
  }
  $$('.side-tab').forEach(b=>b.addEventListener('click',()=>{state.side=b.dataset.side;renderTriggerControls();}));
  $('#trigger-modes').addEventListener('click',e=>{const b=e.target.closest('[data-mode]');if(!b)return;trigger().mode=b.dataset.mode;renderTriggerControls();saveProfiles();});
  function fillRange(el){const f=(+el.value-(+el.min||0))/((+el.max||100)-(+el.min||0))*100;el.style.setProperty('--fill',`${f}%`);}
  $('#trigger-strength').addEventListener('input',e=>{trigger().strength=+e.target.value;$('#trigger-strength-number').textContent=`${trigger().strength}%`;fillRange(e.target);saveProfiles();});
  $('#curve-presets').innerHTML=Object.keys(presets).map(n=>`<button type="button" data-preset="${n}">${n}</button>`).join('');
  $('#curve-presets').addEventListener('click',e=>{const b=e.target.closest('[data-preset]');if(!b)return;trigger().curve=clone(presets[b.dataset.preset]);renderCurve();saveProfiles();toast(`${b.dataset.preset} shape loaded`);});
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
  $('#apply-triggers').addEventListener('click',()=>send({action:'applyTriggers',left:profile().triggers.left,right:profile().triggers.right}));
  $('#reset-effects').addEventListener('click',()=>send({action:'reset'}));

  function renderHapticControls(){
    const h=profile().haptics;
    $('#wave-shapes').innerHTML=['sine','square','saw','noise'].map(s=>`<button type="button" data-shape="${s}" class="${s===h.shape?'is-active':''}">${s}</button>`).join('');
    $('#haptic-sliders').innerHTML=hapticFields.map(f=>`<div class="haptic-slider"><label for="haptic-${f.key}">${f.label} <b id="haptic-${f.key}-value">${h[f.key]}${f.unit}</b></label><input id="haptic-${f.key}" data-field="${f.key}" type="range" min="${f.min}" max="${f.max}" value="${h[f.key]}"></div>`).join('');
    $$('#haptic-sliders input').forEach(fillRange);drawWave(performance.now());
  }
  $('#wave-shapes').addEventListener('click',e=>{const b=e.target.closest('[data-shape]');if(!b)return;profile().haptics.shape=b.dataset.shape;renderHapticControls();saveProfiles();});
  $('#haptic-sliders').addEventListener('input',e=>{const el=e.target.closest('[data-field]');if(!el)return;profile().haptics[el.dataset.field]=+el.value;$('#haptic-'+el.dataset.field+'-value').textContent=el.value+hapticFields.find(f=>f.key===el.dataset.field).unit;fillRange(el);drawWave(performance.now());saveProfiles();});
  $('#texture-list').innerHTML=textures.map((t,i)=>`<button type="button" data-texture="${i}"><small>${t.type}</small><strong>${t.name}</strong><i>LOAD SIGNAL ↗</i></button>`).join('');
  $('#texture-list').addEventListener('click',e=>{const b=e.target.closest('[data-texture]');if(!b)return;Object.assign(profile().haptics,textures[+b.dataset.texture].settings);renderHapticControls();saveProfiles();playHaptic();});
  function waveSample(shape,p){const v=p%1;if(shape==='square')return Math.sin(p*Math.PI*2)>0?1:-1;if(shape==='saw')return 2*v-1;if(shape==='noise')return Math.sin(p*39.5)*Math.sin(p*61.3);return Math.sin(p*Math.PI*2);}
  function drawWave(now){
    const canvas=$('#wave-canvas'),ctx=canvas.getContext('2d'),h=profile().haptics,w=canvas.width,ht=canvas.height,ms=h.attack+h.release;
    ctx.clearRect(0,0,w,ht);ctx.beginPath();ctx.moveTo(0,ht/2);
    const cycles=Math.min(75,h.frequency*ms/1000);
    for(let x=0;x<=w;x+=3){const t=x/w,attack=h.attack/ms,env=t<attack?t/attack:(1-t)/(1-attack),amp=clamp(env)*Math.min(125,h.strength*1.25);ctx.lineTo(x,ht/2-waveSample(h.shape,t*cycles)*amp);}
    ctx.lineWidth=3;ctx.strokeStyle='#8fddd1';ctx.stroke();ctx.strokeStyle='#ff7451';ctx.lineWidth=1;ctx.beginPath();ctx.moveTo(0,ht/2);ctx.lineTo(w,ht/2);ctx.stroke();
    if(now<state.playUntil){const x=clamp((now-state.playStart)/state.playDuration)*w;ctx.fillStyle='rgba(255,116,81,.18)';ctx.fillRect(0,0,x,ht);ctx.fillStyle='#ff7451';ctx.fillRect(x-2,0,4,ht);}
    $('#wave-formula').textContent=`${h.shape.toUpperCase()} / ${h.frequency} HZ`;$('#wave-duration').textContent=`${ms} MS`;
  }
  async function playHaptic(){
    const h=profile().haptics,duration=h.attack+h.release;
    if(state.connected) {
      send({action:'playHaptic',shape:h.shape,frequency:h.frequency,attack:h.attack,release:h.release,strength:h.strength,pan:h.pan});
    } else if(native) toast('Connect a controller for the motor envelope.');
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

  function setLightColor(hex){$('#light-color').value=hex;$('#light-hex').textContent=hex.toUpperCase();$('#light-arcs').style.setProperty('--light',hex);}
  const lightColors=['#ff7451','#8fddd1','#f5e7b5','#6a8dff','#ff3e84','#7ed264'];
  $('#light-presets').innerHTML=lightColors.map(c=>`<button type="button" data-color="${c}" style="--color:${c}" aria-label="Color ${c}"></button>`).join('');
  $('#light-presets').addEventListener('click',e=>{const b=e.target.closest('[data-color]');if(b)setLightColor(b.dataset.color);});
  $('#light-color').addEventListener('input',e=>setLightColor(e.target.value));
  $('#send-light').addEventListener('click',()=>{const h=$('#light-color').value;send({action:'setLightbar',rgb:[1,3,5].map(i=>parseInt(h.slice(i,i+2),16))});});
  $('#battery-light').addEventListener('click',()=>send({action:'batteryLightbar',enabled:!state.batteryLight}));
  $('#release-light').addEventListener('click',()=>send({action:'releaseLightbar'}));
  function renderBatteryLight(){const b=$('#battery-light');b.textContent=`BATTERY COLOR MODE / ${state.batteryLight?'ON':'OFF'}`;b.setAttribute('aria-pressed',String(state.batteryLight));b.classList.toggle('is-active',state.batteryLight);}
  $('#touch-mouse-toggle').addEventListener('click',()=>send({action:'setTouchMouse',enabled:!state.mouse}));
  function renderMouseSwitch(){const b=$('#touch-mouse-toggle');b.classList.toggle('is-active',state.mouse);b.setAttribute('aria-pressed',String(state.mouse));b.innerHTML=state.mouse?'ON <span>●</span>':'OFF <span>○</span>';}
  $('#touch-gesture-toggle').addEventListener('click',()=>send({action:'setTouchGestures',enabled:!state.gestures}));
  function renderGestureSwitch(){const b=$('#touch-gesture-toggle');b.classList.toggle('is-active',state.gestures);b.setAttribute('aria-pressed',String(state.gestures));b.innerHTML=state.gestures?'ON <span>●</span>':'OFF <span>○</span>';}

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
        activation: g.activation
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
    b.classList.toggle('is-active', state.gyroAim);
    b.setAttribute('aria-pressed', String(state.gyroAim));
    b.innerHTML = state.gyroAim ? 'ON <span>●</span>' : 'OFF <span>○</span>';
  }
  $('#gyro-aim-toggle').addEventListener('click', () => {
    if (!state.connected) { toast('Connect a DualSense before enabling gyro aim.'); return; }
    state.gyroAim = !state.gyroAim;
    syncGyroCommand();
    renderGyroControls();
  });
  $('#gyro-sens').addEventListener('input', () => { fillRange($('#gyro-sens')); $('#gyro-sens-value').textContent = `${(+$('#gyro-sens').value/10).toFixed(1)}×`; });
  $('#gyro-sens').addEventListener('change', syncGyroCommand);
  $('#gyro-activation').addEventListener('change', syncGyroCommand);
  $('#gyro-invert').addEventListener('change', syncGyroCommand);

  $('#diag-reset').addEventListener('click',()=>send({action:'reset'}));
  $('#quick-reset').addEventListener('click',()=>send({action:'reset'}));
  function finishScan(){clearTimeout(scanTimeout);state.manualScan=false;$('#scan-button').disabled=false;$('#scan-button').innerHTML='SCAN <span>↗</span>';}
  async function manualScan(){
    state.manualScan=true;$('#scan-button').disabled=true;$('#scan-button').textContent='SCANNING…';
    scanTimeout=setTimeout(finishScan,4000);
    if(!await send({action:'listDevices'}))finishScan();
  }
  $('#diag-scan').addEventListener('click',manualScan);
  $('#scan-button').addEventListener('click',manualScan);
  $('#button-matrix').innerHTML=names.map((n,i)=>`<span data-matrix="${i}">${n}</span>`).join('');

  function renderDeviceList() {
    $('#device-count').textContent = String(state.devices.length).padStart(2,'0');
    const list = $('#device-list'); list.replaceChildren();
    if (!state.devices.length) {
      list.innerHTML = '<p class="device-empty">No DualSense pads reported. Plug in USB or pair Bluetooth, then scan.</p>';
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
    state.connected=connected;state.name=name;state.transport=transport;state.path=path||state.path;
    $('#side-dot').parentElement.classList.toggle('is-connected',connected);
    $('#side-state').textContent=connected?'PAD ONLINE':'SEARCHING FOR PAD';
    $('#top-transport').textContent=connected?`${transport.toUpperCase()} / LIVE`:'NO DEVICE';
    $('#photo-status').textContent=connected?`${transport.toUpperCase()} HID / LIVE`:'HID INPUT / WAITING';
    $('#device-name').textContent=connected?name:'NO CONTROLLER';
    $('#device-subtitle').textContent=connected?`${transport.toUpperCase()} • INPUT STREAM OPEN`:'CONNECT BY USB OR BLUETOOTH';
    $('#diag-device-name').textContent=connected?name:'WAITING FOR CONTROLLER';
    $('#diag-transport').textContent=connected?`${transport.toUpperCase()} / 054C`:'USB / BLUETOOTH';
    $('#diag-firmware').textContent=connected?(firmware||'FIRMWARE INFO UNAVAILABLE'):'FIRMWARE —';
    $('#owner-status').textContent=connected?'RUST CORE / SOLE WRITER':'NO PAD FOUND';
    $('#owner-square').classList.toggle('is-live',connected);
    $$('#apply-triggers,#reset-effects,#touch-mouse-toggle,#gyro-aim-toggle,#send-light,#release-light,#diag-reset,#quick-reset,#apply-profile-now').forEach(b=>b.disabled=!connected);
    if(!connected){state.buttons.fill(0);state.axes.fill(0);state.touch=[null,null];state.battery=null;state.mouse=false;state.gyroAim=false;state.remapArmed=false;state.batteryLight=false;state.leaseUntil=0;state.lastInputAt=0;state.fullReport=false;state.activity.fill(0);$('#diag-rate').textContent='—';renderMouseSwitch();renderGestureSwitch();renderGyroControls();renderBatteryLight();renderInput();}
    renderSession();
  }

  function renderSession(){
    const remaining=Math.max(0,state.leaseUntil-performance.now());
    $('#session-lease').textContent=remaining>0?`${(remaining/1000).toFixed(1)} S / ACTIVE`:'NEUTRAL';
    $('#lease-fill').style.width=remaining>0?`${remaining/state.leaseDuration*100}%`:'0%';
    $('#session-lease').parentElement.classList.toggle('is-active',remaining>0);
    const age=performance.now()-state.lastInputAt;
    const signal=!state.connected?'AWAITING INPUT':!state.lastInputAt?'CONNECTING':age>1500?'STREAM STALLED':state.fullReport?'FULL REPORT':'BASIC INPUT';
    $('#session-signal').textContent=signal;
    $('#session-signal-dot').classList.toggle('is-live',state.connected&&age<=1500&&!!state.lastInputAt);
  }
  function formatAxis(n){return(Math.abs(n)<.005?0:n).toFixed(2);}
  function renderInput(){
    const b=state.buttons,a=state.axes,l=Math.round((b[6]||0)*100),r=Math.round((b[7]||0)*100);
    $$('.hotspot,.shoulder-mark').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.button]||0)>.08));
    $$('.button-matrix span').forEach(el=>el.classList.toggle('is-pressed',(b[+el.dataset.matrix]||0)>.5));
    const active=names.filter((_,i)=>b[i]>.5);$('#active-buttons').textContent=active.length?active.join(' / '):'NONE';
    $('#live-l2').textContent=String(l).padStart(2,'0')+'%';$('#live-r2').textContent=String(r).padStart(2,'0')+'%';
    $('#live-l2-fill').style.width=l+'%';$('#live-r2-fill').style.width=r+'%';
    $('#left-axis').textContent=`${formatAxis(a[0])} / ${formatAxis(a[1])}`;$('#right-axis').textContent=`${formatAxis(a[2])} / ${formatAxis(a[3])}`;
    $('#diag-l2').textContent=(b[6]||0).toFixed(2);$('#diag-r2').textContent=(b[7]||0).toFixed(2);
    $('#left-stick-mark i').style.transform=`translate(${a[0]*8}px,${a[1]*8}px)`;
    $('#right-stick-mark i').style.transform=`translate(${a[2]*8}px,${a[3]*8}px)`;
    $('#battery-value').textContent=state.battery==null?'—':`${state.battery}%`;
    $('#diag-battery').textContent=state.battery==null?'—':`${state.battery}%`;
    $('#battery-detail').textContent=state.battery==null?'WAITING FOR DEVICE':state.charging?'CHARGING':'ON BATTERY';
    $('#battery-light').disabled=!state.connected||state.battery==null;
    $$('#battery-segments i').forEach((el,i)=>el.classList.toggle('is-full',state.battery!=null&&state.battery>(i*10)));
    state.touch.forEach((point,i)=>{const el=$(`#contact-${i}`);el.classList.toggle('is-active',!!point);if(point){el.style.left=`${point.x*100}%`;el.style.top=`${point.y*100}%`;} $(`#touch-${i+1}-pos`).textContent=point?`${point.x.toFixed(3)} / ${point.y.toFixed(3)}`:'—';});
    state.gyro.forEach((n,i)=>{const k=['x','y','z'][i];$(`#gyro-${k}`).textContent=String(n);const f=clamp(Math.abs(n)/16000)*50;const el=$(`#gyro-${k}-fill`);el.style.width=`${f}%`;el.style.left=n<0?`${50-f}%`:'50%';});
    const activity=(active.length/8+Math.abs(a[0])/4+Math.abs(a[1])/4+(b[6]+b[7])/4);state.activity.push(clamp(activity));state.activity.shift();
    updatePlayhead();$('#packet-counter').textContent=String(state.packet%10000).padStart(4,'0');$('#diag-packet').textContent=`REPORT ${String(state.packet%10000).padStart(4,'0')}`;
  }
  $('#battery-segments').innerHTML=Array.from({length:10},()=>'<i></i>').join('');
  function drawActivity(){const canvas=$('#activity-scope'),ctx=canvas.getContext('2d'),w=canvas.width,h=canvas.height;ctx.clearRect(0,0,w,h);ctx.beginPath();state.activity.forEach((v,i)=>{const x=i/(state.activity.length-1)*w,y=h-8-v*(h-16);if(i)ctx.lineTo(x,y);else ctx.moveTo(x,y);});ctx.lineWidth=3;ctx.strokeStyle='#ff7451';ctx.stroke();ctx.fillStyle='#8fddd1';const last=state.activity.at(-1);ctx.fillRect(w-5,h-8-last*(h-16)-4,8,8);}
  function handleEvent(event){
    if(event.type==='device'){
      const changed=state.connected!==!!event.connected || (event.connected&&(state.name!==event.name||state.transport!==event.transport||state.path!==(event.path||'')));
      setConnected(!!event.connected,event.name||'',event.transport||'',event.firmware||'',event.path||'');
      if(event.path) state.selectedPath = event.path;
      if(changed||state.manualScan){const msg=event.connected?`${event.name} CONNECTED / ${event.transport}`:(event.message||'CONTROLLER DISCONNECTED');log(msg);toast(msg);}
      if(state.manualScan)finishScan();
      renderDeviceList();
    }else if(event.type==='devices'){
      state.devices = Array.isArray(event.items) ? event.items : [];
      state.selectedPath = event.selected || state.selectedPath;
      renderDeviceList();
      if(state.manualScan)finishScan();
    }else if(event.type==='input'){
      state.buttons=event.input.buttons;state.axes=event.input.axes;state.touch=event.input.touch;state.gyro=event.input.gyro;
      state.battery=event.input.battery;state.charging=event.input.charging;state.fullReport=!!event.input.fullReport;
      $('#photo-status').textContent=event.input.fullReport?`${state.transport.toUpperCase()} HID / LIVE`:'BLUETOOTH HID / BASIC INPUT';
      state.packet++;state.lastInputAt=performance.now();renderInput();renderSession();
    }else if(event.type==='rate'){
      state.rate=event.reportsPerSecond||0;$('#diag-rate').textContent=state.connected?`${state.rate} / S`:'—';
    }else if(event.type==='conflict'){
      $('#conflict-message').textContent=event.message||'No competing controller tools detected.';
      $('#conflict-message').style.color=event.detected?'#ff7451':'';
      if(event.detected) log(`OUTPUT CONFLICT / ${event.message}`);
    }else if(event.type==='foreground'){
      state.lastExternalExe=event.exe||'';
      $('#detected-app').textContent=`LAST ACTIVE APP / ${state.lastExternalExe||'NONE DETECTED'}`;
      const match=state.profiles.find(p=>p.matchExe?.toLowerCase()===state.lastExternalExe);
      if(match && match.id!==state.profileId){
        state.profileId=match.id;renderAllProfileControls();saveProfiles();
        log(`AUTO LOAD / ${match.name} FOR ${state.lastExternalExe}`);
        toast(`Profile loaded: ${match.name}`);
        if (state.autoApply && match.autoApply !== false) applyProfileHardware(false);
      } else if (match && state.autoApply && match.autoApply !== false) {
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
    }else if(event.type==='core'){
      state.coreStatus = event.status || 'ready';
      $('#core-status').textContent = event.status === 'restarting' ? 'CORE / RESTARTING…'
        : event.status === 'stopped' ? 'CORE / STOPPED'
        : 'CORE / READY';
      $('#core-status').classList.toggle('is-warn', event.status === 'restarting' || event.status === 'stopped');
      if (event.message) log(`CORE / ${event.message}`);
    }else if(event.type==='notice'||event.type==='error'){
      toast(event.message);log(`${event.type.toUpperCase()} / ${event.message}`);
    }
  }
  let lastCanvas=0;function frame(now){if(now-lastCanvas>35){drawActivity();if(state.currentView==='haptics'||now<state.playUntil)drawWave(now);renderSession();lastCanvas=now;}requestAnimationFrame(frame);}requestAnimationFrame(frame);
  const keyMap={j:2,k:0,l:1,i:3,q:4,e:5,z:6,c:7,ArrowUp:12,ArrowDown:13,ArrowLeft:14,ArrowRight:15};
  const held=new Set();window.addEventListener('keydown',e=>{if(state.connected||e.target.matches('input,textarea,button,select'))return;if(!(e.key in keyMap))return;held.add(e.key);state.buttons[keyMap[e.key]]=1;renderInput();});
  window.addEventListener('keyup',e=>{if(!(e.key in keyMap)||state.connected)return;held.delete(e.key);state.buttons[keyMap[e.key]]=0;renderInput();});
  window.addEventListener('blur',()=>{if(state.connected)return;held.clear();state.buttons.fill(0);renderInput();});

  async function start(){
    renderAllProfileControls();setLightColor('#ff7451');setConnected(false);renderInput();renderDeviceList();renderGestureSwitch();navigate(location.hash.slice(1)||'live');
    await loadProfiles();
    if(native){
      try{await window.__TAURI__.event.listen('core-event',e=>handleEvent(e.payload));await send({action:'listDevices'});log('RUST CORE / EVENT CHANNEL OPEN');$('#core-status').textContent='CORE / READY';}
      catch(error){toast(`Core connection failed: ${error}`);log(String(error));$('#core-status').textContent='CORE / ERROR';$('#core-status').classList.add('is-warn');}
    }else{$('#core-status').textContent='CORE / BROWSER PREVIEW';log('BROWSER PREVIEW / KEYBOARD DEMO');}
  }
  start();
})();
