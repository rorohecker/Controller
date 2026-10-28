/* Game routing is explicit. Compatibility reports travel with exported profiles. */
(() => {
  'use strict';
  const copy=value=>structuredClone(value), $=id=>document.getElementById(id);
  const sessionDefaults=()=>({enabled:false,inputRoute:'keep',audio:'keep',audioGain:50,hideHint:false});
  const communityDefaults=()=>({author:'',version:'',gameVersion:'',sourceUrl:'',license:'',updatedAt:'',reports:[]});
  const tracked=new Set(['setXinput','setAudioBridge','setGyroAim','setRemap','applyTriggers','setInputEffects','setGameActions','setGameLink','setAudioMixer']);
  const text=(value,max)=>typeof value==='string'&&value.length<=max&&!/[\u0000-\u001f]/.test(value);
  function valid(p){
    const s={...sessionDefaults(),...p.session},m={...communityDefaults(),...p.community};
    return typeof s.enabled==='boolean'&&['keep','native','xinput'].includes(s.inputRoute)&&['keep','off','system'].includes(s.audio)&&Number.isInteger(s.audioGain)&&s.audioGain>=0&&s.audioGain<=100&&typeof s.hideHint==='boolean'
      &&['author','version','gameVersion','license'].every(key=>text(m[key],80))&&text(m.updatedAt,32)&&text(m.sourceUrl,1024)&&(!m.sourceUrl||/^https:\/\/[^\s]+$/.test(m.sourceUrl))
      &&Array.isArray(m.reports)&&m.reports.length<=20&&m.reports.every(r=>r&&['windows','macos','linux'].includes(r.platform)&&['usb','bluetooth'].includes(r.transport)&&['works','partial','broken'].includes(r.result)&&text(r.date,32)&&text(r.gameVersion,80)&&typeof r.notes==='string'&&r.notes.length<=300&&!r.notes.includes('\0'));
  }
  class ControllerGameSessions {
    static valid=valid;
    static normalize(p){p.session={...sessionDefaults(),...p.session};p.community={...communityDefaults(),...p.community};return p;}
    constructor(api){
      this.a=api;this.current=new Map();this.active=null;this.queue=Promise.resolve();this.epoch=0;this.last='No feedback received';this.notice='No session running';
      this.build();this.renderProfile();this.render();
    }
    panel(title,id){
      const box=this.a.gameNode('section',{class:'work-panel framed game-session-panel',id});
      const cap=this.a.gameNode('div',{class:'panel-cap'});cap.append(this.a.gameNode('span',{text:title}));
      const body=this.a.gameNode('div',{class:'game-content'});box.append(cap,body);return {box,body};
    }
    build(){
      const {gameNode:n}=this.a,setup=this.panel('Game setup','game-setup');
      const grid=n('dl',{class:'game-setup-grid'});
      for(const [id,label] of [['pad','Controller'],['game','Focused game'],['route','Input route'],['source','Feedback source'],['profile','Profile']]){
        const cell=n('div');cell.append(n('dt',{text:label}),n('dd',{id:'setup-'+id}));grid.append(cell);
      }
      this.status=n('p',{id:'game-session-status',role:'status'});this.eventStatus=n('p',{id:'game-feedback-status',role:'status'});
      setup.body.append(grid,this.status,this.eventStatus,n('p',{class:'pc-note',text:'For games without native effects, choose Xbox routing or custom actions. Enable vibration in the game. Native DualSense games should usually keep their own feedback. Xbox rumble carries two motor levels; it does not identify weapons.'}));
      const buttons=n('div',{class:'game-buttons'});
      for(const [label,action] of [['Start this session',()=>this.enqueue(()=>this.start(this.a.profile(),true))],['End session',()=>this.enqueue(()=>this.restore())]]){
        const b=n('button',{type:'button',class:'quiet-action',text:label});b.addEventListener('click',action);buttons.append(b);
      }
      setup.body.append(buttons);$('profiles').querySelector('.page-heading').after(setup.box);
      const session=this.panel('Session settings','game-session-settings');this.sessionBody=session.body;
      const meta=this.panel('Profile source & compatibility','game-community');this.metaBody=meta.body;
      $('profiles').append(session.box,meta.box);
    }
    command(command,owned=false){
      if(command.action==='reset'){
        this.epoch++;this.active=null;this.current.clear();this.notice='Stopped. Apply a profile to resume.';this.render();return;
      }
      if(!tracked.has(command.action))return;
      this.current.set(command.action,copy(command));
      if(this.active){
        if(owned)this.active.owned.set(command.action,copy(command));
        else this.active.owned.delete(command.action); // Keep manual changes when restoring.
      }
      this.render();
    }
    enqueue(task){const epoch=this.epoch;this.queue=this.queue.then(()=>epoch===this.epoch?task():undefined).catch(error=>{this.notice=String(error);this.a.toast(String(error));this.render();});return this.queue;}
    baseline(){
      const {state,profile}=this.a,p=profile(),baseline=new Map(this.current);
      const defaults=[{action:'setXinput',enabled:state.xinput},{action:'setAudioBridge',enabled:state.audioBridge,gain:+$('bridge-gain').value/100,preset:$('bridge-preset').value},
        {action:'setGyroAim',...copy(p.gyroAim),enabled:state.gyroAim},{action:'setRemap',...copy(p.remap),enabled:state.remapArmed},
        {action:'setGameActions',enabled:false,rules:[]},{action:'setGameLink',settings:{enabled:false,port:6969,format:'json'}},
        {action:'setInputEffects',enabled:false,threshold:.82,strength:55},
        {action:'applyTriggers',left:{mode:'off',strength:0,curve:Array(11).fill(0)},right:{mode:'off',strength:0,curve:Array(11).fill(0)}}];
      for(const c of defaults)if(!baseline.has(c.action))baseline.set(c.action,c);
      return baseline;
    }
    async write(command,epoch=this.epoch){if(epoch!==this.epoch||!this.a.state.connected)return false;return this.a.send(command,true);}
    async start(p,manual=false){
      const {state}=this.a;
      if(!state.connected){this.a.toast('Connect a controller.');return;}
      if(state.pcMode){this.a.toast('Switch to Game mode before starting a game session.');return;}
      if(this.active?.id===p.id&&!manual){await this.a.applyGameRules(true);return;}
      if(this.active)await this.restore();
      const epoch=this.epoch;
      this.active={id:p.id,previousId:state.profileId,baseline:this.baseline(),owned:new Map()};
      const s=p.session||sessionDefaults();
      if(s.inputRoute!=='keep'&&window.controllerPlatform.xinput){
        if(!await this.write({action:'setXinput',enabled:s.inputRoute==='xinput'},epoch))return this.restore();
      }
      if(s.audio!=='keep'&&window.controllerPlatform.systemAudioBridge){
        if(!await this.write({action:'setAudioBridge',enabled:s.audio==='system',gain:s.audioGain/100,preset:'gentle'},epoch))return this.restore();
      }
      if(epoch!==this.epoch)return;
      if(state.profileId!==p.id)await this.a.selectProfile(p.id,false);
      if(epoch!==this.epoch)return;
      if(!await this.a.applyProfileHardware(manual,true,epoch)){await this.restore();return;}
      this.notice=`Session: ${p.name}. Previous settings restore on exit.${s.hideHint?' HidHide needs manual setup.':''}`;
      this.render();
    }
    async restore(){
      const active=this.active;if(!active)return;
      this.active=null;
      const epoch=this.epoch;let failed=false;
      for(const [action,owned] of active.owned){
        if(JSON.stringify(this.current.get(action))!==JSON.stringify(owned))continue;
        const previous=active.baseline.get(action);
        if(previous&&!await this.write(previous,epoch))failed=true;
        if(epoch!==this.epoch)return;
      }
      if(this.a.state.profileId===active.id&&this.a.state.profiles.some(p=>p.id===active.previousId))await this.a.selectProfile(active.previousId,false);
      this.notice=failed?'Some previous settings could not be restored. Reconnect and apply your profile.':'Session ended. Previous settings restored.';this.render();
    }
    foreground(match,previous){
      if(!this.a.state.foregroundExe)return; // Own app/minimized desktop has no external executable.
      this.enqueue(async()=>{
        if((match?.id||'')!==this.a.state.autoMatchId)return;
        const {state}=this.a,eligible=match&&state.autoApply&&match.autoApply!==false&&!state.autoSuppressed&&state.connected&&!state.pcMode;
        if(this.active&&(!eligible||this.active.id!==match.id))await this.restore();
        if(match?.session?.enabled&&eligible){await this.start(match);return;}
        const changed=match&&match.id!==state.profileId;
        if(changed)await this.a.selectProfile(match.id,false);
        if(eligible&&(changed||previous!==match.id))await this.a.applyProfileHardware(false);
        if(state.connected)await this.a.applyGameRules();
        this.render();
      });
    }
    event(event){
      if(!['device','foreground','pcMode','xinput','audioStatus','gameActionsStatus','gameLinkStatus','gameFeedback','gameAction','gameEvent','safetyReset'].includes(event.type))return;
      if(event.type==='device'&&!event.connected){this.epoch++;this.active=null;this.current.clear();this.last='No feedback received';this.notice='Connect a controller to start a session.';}
      if(event.type==='safetyReset'){this.epoch++;this.active=null;this.current.clear();this.notice='Stopped with PS + Options.';}
      if(event.type==='gameFeedback')this.last=`Xbox rumble: ${event.motors?.join(' / ')||'0 / 0'}${event.delivered?'':event.suppressed?' (custom cue has priority)':' (output unavailable)'}`;
      if(event.type==='gameAction')this.last=`Action: ${event.name}`;
      if(event.type==='gameEvent')this.last=`Game link: ${[event.weapon,...(event.events||[])].filter(Boolean).join(', ')||'packet received'}`;
      if(event.type==='pcMode'&&event.enabled&&this.active)this.enqueue(()=>this.restore());
      if(event.type==='gameActionsStatus')this.rulesActive=!!event.enabled;
      if(event.type==='gameLinkStatus')this.linkActive=!!event.enabled;
      if(event.type==='device'&&!event.connected){this.rulesActive=false;this.linkActive=false;}
      queueMicrotask(()=>this.render());
    }
    render(){
      const s=this.a.state,p=this.a.profile();
      $('setup-pad').textContent=s.connected?`${s.name||'DualSense'} / ${s.transport}`:'Disconnected';
      $('setup-game').textContent=s.foregroundExe||'No external game focused';
      $('setup-route').textContent=window.controllerPlatform.xinput?(s.xinput?'Virtual Xbox 360':s.xinputStatus==='unavailable'?'Xbox driver unavailable':'Physical DualSense'):'Physical DualSense';
      $('setup-source').textContent=[s.xinput?'Xbox rumble receiver':null,this.rulesActive?'Input actions':null,this.linkActive?'Game link':null,s.audioBridge?'System audio':null].filter(Boolean).join(' + ')||'Native game / no app effects';
      $('setup-profile').textContent=p.name;this.status.textContent=this.notice;this.eventStatus.textContent=this.last;
    }
    renderProfile(){
      const {gameNode:n,gameSelect:select,gameField:field}=this.a,p=this.a.profile();ControllerGameSessions.normalize(p);
      const s=p.session,m=p.community,save=()=>this.a.saveProfiles();
      const grid=n('div',{class:'game-form-grid'});
      const check=(label,key)=>{const input=n('input',{type:'checkbox',checked:s[key]});input.addEventListener('change',()=>{s[key]=input.checked;save();});const l=n('label',{class:'check-row'});l.append(input,document.createTextNode(label));return l;};
      grid.append(check('Manage settings while this game is focused','enabled'));
      if(window.controllerPlatform.xinput)grid.append(field('Input route',select([['keep','Keep current'],['native','Physical DualSense'],['xinput','Xbox 360 bridge']],s.inputRoute,v=>{s.inputRoute=v;save();})),check('Remind me to configure HidHide','hideHint'));
      if(window.controllerPlatform.systemAudioBridge){
        grid.append(field('System audio haptics',select([['keep','Keep current'],['off','Off'],['system','On']],s.audio,v=>{s.audio=v;save();})));
        const gain=n('input',{type:'number',min:0,max:100,value:s.audioGain});gain.addEventListener('change',()=>{s.audioGain=Math.round(Math.max(0,Math.min(100,Number(gain.value)||0)));gain.value=s.audioGain;save();});grid.append(field('Audio strength · %',gain));
      }
      this.sessionBody.replaceChildren(grid,n('p',{class:'pc-note',text:'Gyro, key remaps, triggers and action rules use this profile. Session changes restore when another app takes focus. Manual changes are kept. Xbox routing requires ViGEmBus on Windows. Hiding the physical pad remains a separate HidHide setup step.'}));
      const details=n('div',{class:'game-form-grid'});
      for(const [key,label,max] of [['author','Author',80],['version','Profile version',80],['gameVersion','Game version',80],['sourceUrl','Source URL · HTTPS',1024],['license','License / reuse terms',80]]){
        const input=n('input',{value:m[key],maxLength:max});input.setAttribute('aria-label',label);
        input.addEventListener('change',()=>{if(key==='sourceUrl'&&input.value&&!/^https:\/\/[^\s]+$/.test(input.value)){input.value=m[key];this.a.toast('Use an HTTPS source URL.');return;}m[key]=input.value.trim();m.updatedAt=new Date().toISOString();save();const link=$('community-source-link');link.hidden=!m.sourceUrl;link.href=m.sourceUrl||'#';$('community-updated').textContent='Updated: '+m.updatedAt.slice(0,10);});details.append(field(label,input));
      }
      this.metaBody.replaceChildren(details,n('p',{class:'pc-note',text:'Source details and test reports are saved with exports. Reports are self-reported, not independently verified. Import only profiles whose source and reuse terms you trust.'}));
      this.metaBody.append(n('a',{id:'community-source-link',href:m.sourceUrl||'#',hidden:!m.sourceUrl,target:'_blank',rel:'noreferrer',text:'Open profile source'}));
      this.metaBody.append(n('p',{id:'community-updated',text:m.updatedAt?`Updated: ${m.updatedAt.slice(0,10)}`:'No maintenance date recorded.'}));
      const reports=n('ul',{class:'compatibility-reports'});
      m.reports.forEach((r,i)=>{const row=n('li'),remove=n('button',{type:'button',class:'quiet-action',text:'Remove report'});remove.addEventListener('click',()=>{m.reports.splice(i,1);save();this.renderProfile();});row.append(n('span',{text:`${r.date.slice(0,10)} / ${r.platform} / ${r.transport} / ${r.result}${r.gameVersion?' / game '+r.gameVersion:''}${r.notes?' — '+r.notes:''}`}),remove);reports.append(row);});
      const form=n('div',{class:'game-form-grid'}),result=select([['works','Works'],['partial','Partly works'],['broken','Does not work']],'works',()=>{}),transport=select([['usb','USB'],['bluetooth','Bluetooth']],/bluetooth/i.test(this.a.state.transport)?'bluetooth':'usb',()=>{}),notes=n('input',{placeholder:'What worked or failed?',maxLength:300});notes.setAttribute('aria-label','Compatibility notes');
      const add=n('button',{type:'button',class:'quiet-action',text:'Record my test'});add.addEventListener('click',()=>{if(!['windows','macos','linux'].includes(window.controllerPlatform.os)){this.a.toast('Open the desktop app to record its platform.');return;}if(m.reports.length>=20){this.a.toast('Remove an older report first (20 max).');return;}m.reports.push({platform:window.controllerPlatform.os,transport:transport.value,result:result.value,date:new Date().toISOString().slice(0,10),gameVersion:m.gameVersion,notes:notes.value.trim()});m.updatedAt=new Date().toISOString();save();this.renderProfile();});
      form.append(field('My test result',result),field('Connection tested',transport),field('Test notes',notes),add);this.metaBody.append(reports,form);this.render();
    }
  }
  window.ControllerGameSessions=ControllerGameSessions;
})();
