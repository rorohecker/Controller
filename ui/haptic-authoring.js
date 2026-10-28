/* Tactile authoring: requested settings stay separate from native encoding. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id),copy=x=>structuredClone(x);
  const lanes=[['leftTrigger','L2'],['rightTrigger','R2'],['leftGrip','Left grip'],['rightGrip','Right grip']];
  const defaults=()=>({enabled:false,minimum:[0,0],maximum:[100,100],balance:0,triggerLimit:8});
  const cue=(lane='leftGrip',kind='pulse',startMs=0,durationMs=100,strength=40)=>({lane,kind,startMs,durationMs,strength,fromStrength:0,frequency:90,shape:'sine',startZone:2,endZone:7});
  const builtins=()=>[
    {id:'tap',name:'Soft tap',tags:['soft','interface'],cues:[cue('leftGrip'),cue('rightGrip')]},
    {id:'latch',name:'Metal latch',tags:['metal','impact'],cues:[{...cue('leftGrip','pulse',0,45,65),shape:'square',frequency:160},cue('rightGrip','pulse',25,80,45)]},
    {id:'pebbles',name:'Pebble trail',tags:['rough','texture'],cues:[{...cue('leftGrip','texture',0,700,35),shape:'noise',frequency:120},{...cue('rightGrip','texture',30,670,35),shape:'noise',frequency:140}]},
    {id:'charge',name:'Charge and release',tags:['charge','trigger'],cues:[cue('rightTrigger','load',0,600,55),cue('rightTrigger','kick',600,80,85),cue('rightTrigger','release',680,160,0),cue('leftGrip','pulse',600,100,60),cue('rightGrip','pulse',600,100,60)]}
  ].map(c=>({...c,author:'CONTROLLER',license:'CC0-1.0',sourceUrl:''}));
  function validClip(c){
    const text=(s,max)=>typeof s==='string'&&s.length<=max&&!/[\u0000-\u001f]/.test(s),number=(x,min,max)=>Number.isFinite(x)&&x>=min&&x<=max,integer=(x,min,max)=>Number.isInteger(x)&&number(x,min,max);
    if(!c||!text(c.id,80)||!c.id||!text(c.name,40)||!c.name.trim()||!Array.isArray(c.tags)||c.tags.length>8||c.tags.some(t=>!text(t,24))||!text(c.author,80)||!text(c.license,120)||!text(c.sourceUrl,1024)||c.sourceUrl&&!/^https:\/\/[^\s]+$/.test(c.sourceUrl)||!Array.isArray(c.cues)||!c.cues.length||c.cues.length>32)return false;
    if(c.cues.some(v=>!v||!lanes.some(([l])=>l===v.lane)||!integer(v.startMs,0,7990)||!integer(v.durationMs,10,3000)||v.startMs+v.durationMs>8000||!number(v.strength,0,100)||!number(v.fromStrength,0,100)||!integer(v.frequency,20,300)||!['sine','square','saw','noise'].includes(v.shape)||!integer(v.startZone,0,7)||!integer(v.endZone,1,8)||v.endZone<=v.startZone||!(v.lane.endsWith('Grip')?['pulse','texture']:['load','wall','kick','release']).includes(v.kind)))return false;
    const start=v=>Math.ceil(v.startMs/10)*10,end=v=>Math.ceil((v.startMs+v.durationMs)/10)*10;
    return !c.cues.some((a,i)=>a.lane.endsWith('Trigger')&&c.cues.slice(i+1).some(b=>a.lane===b.lane&&start(a)<end(b)&&start(b)<end(a)));
  }
  const validLibrary=clips=>Array.isArray(clips)&&clips.length<=64&&new Set(clips.map(c=>c?.id)).size===clips.length&&clips.every(validClip);
  class ControllerHapticAuthoring {
    static validClip=validClip;
    static validLibrary=validLibrary;
    constructor(api){
      this.a=api;this.library=builtins();this.draft=copy(this.library[0]);this.selected=0;this.personal=defaults();this.owner='';this.playId=Date.now()*1000;this.personalRequest=Date.now()*1000;this.previewId=0;this.triggerRequest=0;this.playing=false;
      this.build();this.draw();this.personalForm();
    }
    panel(title,id,host){const n=this.a.gameNode,box=n('section',{class:'work-panel framed tactile-panel',id}),body=n('div',{class:'game-content'});box.append(n('div',{class:'panel-cap',text:title}),body);host.append(box);return body;}
    button(text,run,id){const b=this.a.gameNode('button',{type:'button',class:'quiet-action',text,id:id||''});b.addEventListener('click',run);return b;}
    number(label,value,min,max,change,step=1){let current=value;const n=this.a.gameNode('input',{type:'number',value,min,max,step});n.setAttribute('aria-label',label);n.addEventListener('change',()=>{const v=Number(n.value);if(!Number.isFinite(v)||v<min||v>max||step===1&&!Number.isInteger(v)){n.value=String(current);this.a.toast(`Enter ${min}–${max}`);return;}current=v;change(v);});return this.a.gameField(label,n);}
    text(label,value,max,change){const n=this.a.gameNode('input',{value,maxLength:max});n.setAttribute('aria-label',label);n.addEventListener('input',()=>change(n.value));return this.a.gameField(label,n);}
    select(label,options,value,change){const input=this.a.gameSelect(options,value,change);input.setAttribute('aria-label',label);return this.a.gameField(label,input);}
    build(){
      const n=this.a.gameNode;
      this.body=this.panel('Tactile workshop','tactile-workshop',$('haptics'));
      this.body.append(n('p',{text:'Build an effect in time. Trigger phases use motor zones; grip lanes can overlap. Stereo grip playback requires USB.'}));
      const toolbar=n('div',{class:'game-buttons'});this.name=n('input',{value:this.draft.name,maxLength:40,id:'tactile-name'});this.name.setAttribute('aria-label','Tactile clip name');this.name.addEventListener('input',()=>this.draft.name=this.name.value);
      this.play=this.button('Play clip',()=>this.playClip(),'tactile-play');this.stop=this.button('Stop clip',()=>this.stopClip(),'tactile-stop');
      toolbar.append(this.name,this.play,this.stop,this.button('Save as new',()=>this.save(false),'tactile-save'),this.button('Update saved',()=>this.save(true),'tactile-update'));this.body.append(toolbar);
      this.progress=n('progress',{id:'tactile-progress',max:1,value:0});this.progress.setAttribute('aria-label','Effect playback');this.status=n('p',{id:'tactile-status',role:'status',text:'Choose a cue to edit it.'});this.body.append(this.progress,this.status);
      const work=n('div',{class:'tactile-grid'});this.lanes=n('div',{class:'tactile-lanes',id:'tactile-lanes'});this.editor=n('div',{class:'tactile-editor',id:'tactile-editor'});work.append(this.lanes,this.editor);this.body.append(work);
      const actions=n('div',{class:'game-buttons'});actions.append(this.button('Add cue',()=>{if(this.draft.cues.length>=32)return this.a.toast('32 cue limit.');const lane=this.draft.cues[this.selected]?.lane||'leftGrip',last=Math.max(0,...this.draft.cues.filter(c=>c.lane===lane).map(c=>c.startMs+c.durationMs));if(last+100>8000)return this.a.toast('This lane is full.');this.draft.cues.push(cue(lane,lane.endsWith('Grip')?'pulse':'wall',last));this.selected=this.draft.cues.length-1;this.draw();},'tactile-add'),this.button('Remove cue',()=>{if(this.draft.cues.length===1)return this.a.toast('Keep at least one cue.');this.draft.cues.splice(this.selected,1);this.selected=0;this.draw();},'tactile-remove'));this.body.append(actions);
      const metadata=n('details');metadata.append(n('summary',{text:'Tags & source'}));this.meta=n('div',{class:'game-form-grid'});metadata.append(this.meta);this.body.append(metadata);
      this.encoded=n('div',{id:'tactile-encoded',class:'tactile-encoded'});this.body.append(n('p',{class:'pc-note',text:'Load ramps force over time. Wall holds a stop. Kick briefly changes the stop; leave room for trigger travel. Release removes resistance. At the end, the previous trigger settings return. HID changes are scheduled at 10 ms intervals and can arrive later under load.'}),this.encoded);
      const library=this.panel('Tactile library','tactile-library',$('haptics'));this.search=n('input',{placeholder:'Find clips by name or tag',id:'tactile-search'});this.search.setAttribute('aria-label','Find tactile clips');this.search.addEventListener('input',()=>this.drawLibrary());this.cards=n('div',{class:'tactile-library-grid',id:'tactile-library-grid'});library.append(this.search,n('p',{class:'pc-note',text:'Clips are included in profile exports and backups. Action rules keep their own copy, so editing a library clip does not silently change a game.'}),this.cards);
      this.personalBody=this.panel('Personal feel','personal-feel',$('haptics'));
      this.triggerPreview=this.panel('Requested / encoded','trigger-encoded',$('triggers'));
      $('triggers').addEventListener('change',()=>this.inspectTrigger());$('triggers').addEventListener('input',()=>this.inspectTrigger());$('triggers').addEventListener('click',()=>this.inspectTrigger());
      const compare=this.panel('Compare audio conversion','audio-conversion',$('haptics'));
      const modes=[['music','Full soundtrack'],['bass','Bass'],['impact','Impacts'],['texture','Texture']];this.aMode=this.a.gameSelect(modes,'bass',()=>{});this.aMode.id='conversion-a';this.bMode=this.a.gameSelect(modes,'texture',()=>{});this.bMode.id='conversion-b';
      compare.append(n('p',{text:'Load an audio file above. Capture a position, then compare the same five seconds through the grips at the same mixer level.'}));
      this.anchor=n('p',{id:'conversion-anchor',text:'No comparison position captured.'});const compareControls=n('div',{class:'game-buttons'});
      compareControls.append(this.button('Capture song position',()=>{this.compareAt=this.a.getPlayer().current();this.anchor.textContent=`Comparison starts at ${this.compareAt.toFixed(1)} s`;},'conversion-capture'),this.a.gameField('A',this.aMode),this.button('Play A',()=>this.compare(this.aMode.value),'conversion-play-a'),this.a.gameField('B',this.bMode),this.button('Play B',()=>this.compare(this.bMode.value),'conversion-play-b'));compare.append(compareControls,this.anchor);
    }
    duration(){return Math.max(...this.draft.cues.map(c=>Math.ceil((c.startMs+c.durationMs)/10)*10));}
    draw(){this.name.value=this.draft.name;this.drawTimeline();this.drawEditor();this.drawMetadata();this.drawLibrary();this.inspectClip();}
    drawTimeline(){
      const n=this.a.gameNode,total=this.duration();this.lanes.replaceChildren();
      for(const [lane,label] of lanes){const row=n('div',{class:'tactile-lane'}),track=n('div',{class:'tactile-track'});row.dataset.lane=lane;row.append(n('strong',{text:label}),track);
        this.draft.cues.forEach((c,i)=>{if(c.lane!==lane)return;const b=this.button('',()=>{this.selected=i;this.drawTimeline();this.drawEditor();});b.className='tactile-cue is-'+c.kind+(i===this.selected?' is-selected':'');b.style.left=`${c.startMs/total*100}%`;b.style.width=`${c.durationMs/total*100}%`;b.title=`${label}: ${c.kind}, ${c.startMs}–${c.startMs+c.durationMs} ms`;b.setAttribute('aria-label',b.title);track.append(b);});this.lanes.append(row,n('p',{class:'tactile-caption',text:this.draft.cues.filter(c=>c.lane===lane).map(c=>`${c.kind} ${c.startMs} to ${c.startMs+c.durationMs} ms / ${c.strength}%`).join(' / ')||'No cues'}));}
      this.lanes.append(n('p',{class:'pc-note',text:`0–${total} ms · ${this.draft.cues.length}/32 cues`}));
    }
    changed(){this.drawTimeline();this.inspectClip();}
    drawEditor(){
      const c=this.draft.cues[this.selected],grip=c.lane.endsWith('Grip');this.editor.replaceChildren();
      this.editor.append(this.select('Lane',lanes,c.lane,v=>{c.lane=v;c.kind=v.endsWith('Grip')?'pulse':'wall';this.drawEditor();this.changed();}),this.select('Phase',(grip?['pulse','texture']:['load','wall','kick','release']).map(v=>[v,v[0].toUpperCase()+v.slice(1)]),c.kind,v=>{c.kind=v;this.drawEditor();this.changed();}),this.number('Start · ms',c.startMs,0,7990,v=>{c.startMs=v;this.changed();}),this.number('Duration · ms',c.durationMs,10,3000,v=>{c.durationMs=v;this.changed();}),this.number('Strength · %',c.strength,0,100,v=>{c.strength=v;this.changed();},.1));
      if(c.kind==='load')this.editor.append(this.number('Initial force · %',c.fromStrength,0,100,v=>{c.fromStrength=v;this.changed();},.1));
      if(grip)this.editor.append(this.number('Frequency · Hz',c.frequency,20,300,v=>{c.frequency=v;this.changed();}),this.select('Texture',['sine','square','saw','noise'].map(v=>[v,v]),c.shape,v=>{c.shape=v;this.changed();}));
      else this.editor.append(this.number('Start zone · 1–8',c.startZone+1,1,8,v=>{c.startZone=v-1;this.changed();}),this.number('End zone · 2–9',c.endZone+1,2,9,v=>{c.endZone=v-1;this.changed();}));
    }
    drawMetadata(){const c=this.draft;this.meta.replaceChildren(this.text('Tags · comma separated',c.tags.join(', '),200,v=>c.tags=v.split(',').map(s=>s.trim()).filter(Boolean)),this.text('Clip author',c.author,80,v=>c.author=v),this.text('Clip license',c.license,120,v=>c.license=v),this.text('Clip source · HTTPS',c.sourceUrl,1024,v=>c.sourceUrl=v));}
    drawLibrary(){
      const n=this.a.gameNode,query=this.search.value.toLowerCase();this.cards.replaceChildren();
      for(const clip of this.library.filter(c=>[c.name,...c.tags].join(' ').toLowerCase().includes(query))){const card=n('article',{class:'tactile-library-card'}),buttons=n('div',{class:'game-buttons'});card.append(n('h3',{text:clip.name}),n('p',{text:clip.tags.join(' / ')||'No tags'}),n('small',{text:`${clip.author||'Unknown author'} · ${clip.license||'No reuse terms recorded'}`}));
        buttons.append(this.button('Load',()=>{this.draft=copy(clip);this.selected=0;this.draw();}),this.button('Use in new action',()=>{const p=this.a.profile();if(p.actions.length>=24)return this.a.toast('24 action limit.');p.actions.push({id:crypto.randomUUID(),name:clip.name,source:'press',button:1,event:'',windowMs:280,cooldownMs:180,durationMs:120,haptic:null,leftTrigger:null,rightTrigger:null,timeline:copy(clip)});this.a.saveProfiles();this.a.renderGameProfiles();this.a.navigate('profiles');this.a.toast('Choose the input, then Save & apply.');}),this.button('Delete',async()=>{const old=this.library;this.library=this.library.filter(c=>c.id!==clip.id);if(!await this.a.saveProfiles(true))this.library=old;this.drawLibrary();}));card.append(buttons);this.cards.append(card);}
      if(!this.cards.children.length)this.cards.append(n('p',{text:'No matching clips.'}));
    }
    restore(clips){this.library=clips==null?builtins():copy(clips);this.drawLibrary();}
    archive(){return copy(this.library);}
    async save(update){
      const c=copy(this.draft);if(!update)c.id=crypto.randomUUID();
      if(!validClip(c))return this.a.toast('Check cue timing, trigger overlaps, tags and source URL.');
      const i=this.library.findIndex(v=>v.id===c.id);if(update&&i<0)return this.a.toast('Save this clip as new first.');
      if(i<0&&this.library.length>=64)return this.a.toast('64 clip limit.');
      const old=copy(this.library);if(i<0)this.library.push(c);else this.library[i]=c;
      if(await this.a.saveProfiles(true)){this.draft=c;this.drawLibrary();this.a.toast('Clip saved.');}else this.library=old;
    }
    async playClip(){
      if(!validClip(this.draft))return this.a.toast('Fix the clip before playing it.');
      if(!this.a.state.connected)return this.a.toast('Connect a controller.');
      this.playId++;this.playing=true;const id=this.playId;
      this.status.textContent='Starting clip…';
      if(!await this.a.send({action:'playEffectClip',clip:copy(this.draft),playId:id})&&id===this.playId){this.playing=false;this.status.textContent='Could not start clip. Check Diagnostics.';}
    }
    stopClip(){this.playId++;this.playing=false;this.progress.value=0;this.status.textContent='Stopped';if(this.a.state.connected)this.a.send({action:'stopEffectClip',playId:this.playId});}
    inspectClip(){clearTimeout(this.clipTimer);const id=++this.previewId;this.clipTimer=setTimeout(async()=>{
      if(!validClip(this.draft)){this.encoded.textContent='Check cue ranges and overlapping trigger phases.';return;}
      if(!this.a.native){this.encoded.textContent='Open the desktop app to inspect hardware encoding.';return;}
      try{const result=await this.a.invoke('preview_authoring',{clip:copy(this.draft),trigger:null,personal:this.personal});if(id!==this.previewId)return;if(!result){this.encoded.textContent='Native encoding preview unavailable.';return;}this.encoded.replaceChildren(this.a.gameNode('strong',{text:`Encoded duration: ${result.durationMs} ms · ${result.quantumMs} ms timing steps`}));
        for(const c of result.cues)this.encoded.append(this.a.gameNode('p',{text:`${lanes.find(([l])=>l===c.lane)?.[1]} ${c.kind}: ${c.requestedStartMs}+${c.requestedDurationMs} ms → ${c.startMs}+${c.durationMs} ms${c.levels?' · force '+c.levels.join(' / '):''}`}));
      }catch(error){if(id===this.previewId)this.encoded.textContent=String(error);}
    },120);}
    inspectTrigger(){clearTimeout(this.triggerTimer);const id=++this.triggerRequest;this.triggerTimer=setTimeout(async()=>{
      try{const result=this.a.native?await this.a.invoke('preview_authoring',{clip:null,trigger:copy(this.a.trigger()),personal:this.personal}):null;if(id!==this.triggerRequest)return;
        if(!result){this.triggerPreview.textContent='Open the desktop app to inspect encoded trigger steps.';return;}
        const requested=this.a.trigger(),n=this.a.gameNode;this.triggerPreview.replaceChildren(n('p',{text:`Requested: ${requested.mode}, ${requested.strength}% · encoded force limit: ${result.triggerLimit}/8`}),n('p',{text:`Encoded zone steps: ${result.levels.join(' / ')}`}),n('p',{text:`Packet: ${result.packet.map(b=>b.toString(16).padStart(2,'0')).join(' ')}${result.frequencyHz?' · '+result.frequencyHz+' Hz':''}`}),n('p',{class:'pc-note',text:'Preview only; Save personal feel and Apply triggers to send changes. '+result.note}));
      }catch(error){this.triggerPreview.textContent=String(error);}
    },120);}
    device(event){
      const owner=event.controllerId||event.path||`${event.name}/${event.transport}`;this.owner=owner;this.personal=defaults();
      try{const stored=JSON.parse(localStorage.getItem('controller.personalFeel.v1')||'{}');if(stored[owner]&&this.validPersonal(stored[owner]))this.personal=stored[owner];}catch{}
      this.personalForm();this.personalStatus.textContent='Applying personal limits...';this.a.send({action:'setPersonalHaptics',settings:copy(this.personal),requestId:++this.personalRequest});this.inspectClip();this.inspectTrigger();
    }
    validPersonal(s){return s&&typeof s.enabled==='boolean'&&Array.isArray(s.minimum)&&s.minimum.length===2&&Array.isArray(s.maximum)&&s.maximum.length===2&&s.minimum.every((v,i)=>Number.isInteger(v)&&v>=0&&v<s.maximum[i]&&Number.isInteger(s.maximum[i])&&s.maximum[i]<=100)&&Number.isInteger(s.balance)&&s.balance>=-100&&s.balance<=100&&Number.isInteger(s.triggerLimit)&&s.triggerLimit>=1&&s.triggerLimit<=8;}
    personalForm(){
      const n=this.a.gameNode,s=this.personal,grid=n('div',{class:'game-form-grid'}),enabled=n('input',{type:'checkbox',checked:s.enabled,id:'personal-enabled'}),label=n('label',{class:'check-row',text:'Use personal limits'});enabled.addEventListener('change',()=>s.enabled=enabled.checked);label.prepend(enabled);grid.append(label);
      for(const [i,side] of ['Left','Right'].entries())grid.append(this.number(`${side} noticeable · %`,s.minimum[i],0,99,v=>s.minimum[i]=v),this.number(`${side} comfortable maximum · %`,s.maximum[i],1,100,v=>s.maximum[i]=v));
      grid.append(this.number('Balance · left −100 / right 100',s.balance,-100,100,v=>s.balance=v),this.number('Trigger force limit · 1–8',s.triggerLimit,1,8,v=>s.triggerLimit=v));
      this.personalStatus=n('p',{id:'personal-status',role:'status',text:this.owner?'Saved for this controller.':'Connect a controller to save personal limits.'});
      const save=this.button('Save personal feel',async()=>{if(!this.owner||!this.a.state.connected)return this.a.toast('Connect a controller.');if(!this.validPersonal(s))return this.a.toast('Noticeable strength must be below the comfortable maximum.');try{const stored=JSON.parse(localStorage.getItem('controller.personalFeel.v1')||'{}');stored[this.owner]=copy(s);localStorage.setItem('controller.personalFeel.v1',JSON.stringify(stored));}catch{this.a.toast('Local storage is unavailable; changes will last for this session.');}this.personalStatus.textContent='Applying personal limits...';const request=++this.personalRequest;if(!await this.a.send({action:'setPersonalHaptics',settings:copy(s),requestId:request})&&request===this.personalRequest)this.personalStatus.textContent='Controller update failed. Reconnect to apply saved limits.';this.inspectClip();this.inspectTrigger();},'personal-save');
      this.personalBody.replaceChildren(n('p',{text:'Measure what you can feel, then choose a comfortable maximum for each grip. These are personal preferences, not measured force. Saved grip caps affect all app feedback; the trigger cap limits encoded force steps and stays in place after Reset.'}),grid,save,this.personalStatus);
      this.guideStep=0;const guide=n('div',{class:'personal-guide'}),step=n('p',{id:'personal-guide-step'}),strength=n('input',{type:'number',value:10,min:0,max:100,id:'personal-test-strength'});strength.setAttribute('aria-label','Raw test strength');const stages=['Find the faintest left-grip pulse you can feel.','Find the faintest right-grip pulse you can feel.','Find a comfortable left-grip maximum. Start low.','Find a comfortable right-grip maximum. Start low.'];
      const test=this.button('Test grip · USB',()=>{const value=Number(strength.value);if(!Number.isInteger(value)||value<0||value>100)return this.a.toast('Choose 0–100%.');if(!this.a.state.connected)return this.a.toast('Connect a controller.');this.a.send({action:'playPersonalTest',side:this.guideStep%2,strength:value});},'personal-test');
      const next=this.button('Record & next',()=>{const value=Number(strength.value);if(!Number.isInteger(value)||value<0||value>100)return this.a.toast('Choose 0–100%.');if(this.guideStep<2)s.minimum[this.guideStep]=value;else s.maximum[this.guideStep-2]=value;this.guideStep++;if(this.guideStep>=4){this.personalForm();this.personalStatus.textContent='Measurements captured. Check the values, enable limits, then Save personal feel.';return;}strength.value=this.guideStep<2?10:40;step.textContent=`${this.guideStep+1}/4 · ${stages[this.guideStep]}`;},'personal-next');
      step.textContent='1/4 · '+stages[0];guide.append(step,n('p',{class:'pc-note',text:'Each test briefly solos one grip at raw strength, bypassing personal and mixer levels. The app’s fixed peak protection remains active. Use USB. Nothing plays until you press Test.'}),strength,test,next);this.personalBody.append(guide);
    }
    compare(kind){const player=this.a.getPlayer();if(!player.nativeFile)return this.a.toast('Choose a native audio file above to compare conversion modes.');if(this.compareAt==null){this.compareAt=player.current();this.anchor.textContent=`Comparison starts at ${this.compareAt.toFixed(1)} s`;}player.audition(kind,this.compareAt,5);}
    event(e){
      if(e.type==='personalHapticsStatus'&&e.requestId===this.personalRequest)this.personalStatus.textContent=e.applied?'Personal limits applied.':'Controller update failed: '+(e.message||'Reconnect and try again.');
      if(e.type==='effectTimeline'&&e.playId===this.playId){this.playing=!!e.playing;this.progress.max=e.durationMs;this.progress.value=e.positionMs;this.status.textContent=e.error|| (e.playing?`${e.name||'Clip'} · ${e.positionMs}/${e.durationMs} ms`:'Clip finished');}
      if(e.type==='safetyReset'||e.type==='device'&&!e.connected){this.playId++;this.playing=false;this.progress.value=0;this.status.textContent='Stopped';if(e.type==='device')this.owner='';}
    }
  }
  window.ControllerHapticAuthoring=ControllerHapticAuthoring;
})();
