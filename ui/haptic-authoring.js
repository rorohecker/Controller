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
    if(c.cues.some(v=>!v||!lanes.some(([l])=>l===v.lane)||!integer(v.startMs,0,7990)||!integer(v.durationMs,10,3000)||v.startMs+v.durationMs>8000||!number(v.strength,0,100)||!number(v.fromStrength,0,100)||!integer(v.frequency,20,300)||!['sine','triangle','square','saw','noise','pulse','double','swell'].includes(v.shape)||!integer(v.startZone,0,7)||!integer(v.endZone,1,8)||v.endZone<=v.startZone||!(v.lane.endsWith('Grip')?['pulse','texture']:['load','wall','kick','release']).includes(v.kind)))return false;
    const start=v=>Math.ceil(v.startMs/10)*10,end=v=>Math.ceil((v.startMs+v.durationMs)/10)*10;
    return !c.cues.some((a,i)=>a.lane.endsWith('Trigger')&&c.cues.slice(i+1).some(b=>a.lane===b.lane&&start(a)<end(b)&&start(b)<end(a)));
  }
  // Imports are rebuilt from known keys only: the backend structs reject unknown fields.
  const CLIP_KEYS=['id','name','tags','author','license','sourceUrl'],CUE_KEYS=['lane','kind','startMs','durationMs','strength','fromStrength','frequency','shape','startZone','endZone'];
  const pick=(o,keys)=>Object.fromEntries(keys.filter(k=>o&&Object.hasOwn(o,k)).map(k=>[k,structuredClone(o[k])]));
  const sanitizeClip=c=>c&&typeof c==='object'&&!Array.isArray(c)?{...pick(c,CLIP_KEYS),cues:Array.isArray(c.cues)?c.cues.slice(0,64).map(v=>v&&typeof v==='object'?pick(v,CUE_KEYS):v):c.cues}:c;
  const validLibrary=clips=>Array.isArray(clips)&&clips.length<=64&&new Set(clips.map(c=>c?.id)).size===clips.length&&clips.every(validClip);
  class ControllerHapticAuthoring {
    static validClip=validClip;
    static validLibrary=validLibrary;
    constructor(api){
      this.a=api;this.library=builtins();this.draft=copy(this.library[0]);this.selected=0;this.personal=defaults();this.owner='';this.playId=Date.now()*1000;this.personalRequest=Date.now()*1000;this.previewId=0;this.triggerRequest=0;this.playing=false;
      this.build();this.draw();this.personalForm();this.resetHistory();this.injectStyle();
      document.addEventListener('controller-view',()=>this.renderPlayback());
      document.addEventListener('visibilitychange',()=>this.renderPlayback());
    }
    injectStyle(){
      const st=document.createElement('style');st.textContent=`
.tactile-ruler-track{height:18px;background:transparent!important}.tactile-tick{position:absolute;top:0;font:.65rem var(--mono,monospace);color:var(--parchment-dim,#d5e3df);border-left:1px solid var(--line,#a8bec1);padding-left:2px;height:100%}
.tactile-track{position:relative}.tactile-playhead{position:absolute;top:-2px;bottom:-2px;width:2px;background:var(--gold,#e7b46b);pointer-events:none;z-index:2}.tactile-playhead[hidden]{display:none}
.tactile-lane[data-lane=leftTrigger] .tactile-cue{background:var(--teal,#54c6c5)}.tactile-lane[data-lane=rightTrigger] .tactile-cue{background:var(--teal-glow,#9be9e4)}.tactile-lane[data-lane=leftGrip] .tactile-cue{background:var(--gold,#e7b46b)}.tactile-lane[data-lane=rightGrip] .tactile-cue{background:var(--gold-deep,#a8663f)}
.tactile-cue{position:absolute;touch-action:none;cursor:grab;border-right:6px solid rgba(0,0,0,.35)}.tactile-cue.is-selected{outline:2px solid var(--cream,#f0f8f7)}
.tactile-compare{width:100%;border-collapse:collapse;font-size:.85rem}.tactile-compare th,.tactile-compare td{text-align:left;padding:2px 8px;border-bottom:1px solid var(--stage-lift,#254158)}.tactile-compare .shift{color:var(--gold-soft,#ffe0ad)}
.personal-validation{font-size:.85rem}.personal-validation.bad{color:var(--danger,#c45c3e)}`;
      document.head.append(st);
    }
    panel(title,id,host,meta){const n=this.a.gameNode,box=n('section',{class:'work-panel framed tactile-panel',id}),body=n('div',{class:'game-content'});box.append(n('div',{class:'panel-cap',text:title}),body);if(meta)window.controllerPlatform?.section?.(box,meta.id,meta.level,meta.title||title,meta.hint,meta);host.append(box);return body;}
    button(text,run,id){const b=this.a.gameNode('button',{type:'button',class:'quiet-action',text,...(id?{id}:{})});b.addEventListener('click',run);return b;}
    number(label,value,min,max,change,step=1){let current=value;const n=this.a.gameNode('input',{type:'number',value,min,max,step});n.setAttribute('aria-label',label);n.addEventListener('change',()=>{const v=Number(n.value);if(!Number.isFinite(v)||v<min||v>max||step===1&&!Number.isInteger(v)){n.value=String(current);this.a.toast(`Enter ${min}–${max}`);return;}current=v;change(v);});return this.a.gameField(label,n);}
    text(label,value,max,change){const n=this.a.gameNode('input',{value,maxLength:max});n.setAttribute('aria-label',label);n.addEventListener('input',()=>change(n.value));return this.a.gameField(label,n);}
    select(label,options,value,change){const input=this.a.gameSelect(options,value,change);input.setAttribute('aria-label',label);return this.a.gameField(label,input);}
    build(){
      const n=this.a.gameNode;
      this.body=this.panel('Tactile workshop','tactile-workshop',$('haptics'),{id:'haptics.workshop',level:'basic',title:'Tactile workshop',hint:'Edit a cue on a timeline',open:true,keywords:'tactile workshop cue timeline clip editor haptic edit weapon'});
      this.body.append(n('p',{text:'Build an effect in time. Trigger phases use motor zones; grip lanes can overlap. Stereo grip playback requires USB.'}));
      const toolbar=n('div',{class:'game-buttons'});this.name=n('input',{value:this.draft.name,maxLength:40,id:'tactile-name'});this.name.setAttribute('aria-label','Tactile clip name');this.name.addEventListener('input',()=>this.draft.name=this.name.value);
      this.play=this.button('Play clip',()=>this.playClip(),'tactile-play');this.stop=this.button('Stop clip',()=>this.stopClip(),'tactile-stop');
      toolbar.append(this.name,this.play,this.stop,this.button('Save as new',()=>this.save(false),'tactile-save'),this.button('Update saved',()=>this.save(true),'tactile-update'));this.body.append(toolbar);
      this.progress=n('progress',{id:'tactile-progress',max:1,value:0});this.progress.setAttribute('aria-label','Effect playback');this.status=n('p',{id:'tactile-status',role:'status',text:'Choose a cue to edit it.'});this.body.append(this.progress,this.status);
      const work=n('div',{class:'tactile-grid'});this.lanes=n('div',{class:'tactile-lanes',id:'tactile-lanes'});this.editor=n('div',{class:'tactile-editor',id:'tactile-editor'});work.append(this.lanes,this.editor);this.body.append(work);
      const actions=n('div',{class:'game-buttons'});actions.append(this.button('Add cue',()=>{if(this.draft.cues.length>=32)return this.a.toast('32 cue limit.');const lane=this.draft.cues[this.selected]?.lane||'leftGrip',last=Math.max(0,...this.draft.cues.filter(c=>c.lane===lane).map(c=>c.startMs+c.durationMs));if(last+100>8000)return this.a.toast('This lane is full.');this.draft.cues.push(cue(lane,lane.endsWith('Grip')?'pulse':'wall',last));this.selected=this.draft.cues.length-1;this.noteChange(this.lastJson);this.draw();},'tactile-add'),this.button('Remove cue',()=>{if(this.draft.cues.length===1)return this.a.toast('Keep at least one cue.');this.draft.cues.splice(this.selected,1);this.selected=0;this.noteChange(this.lastJson);this.draw();},'tactile-remove'));
      this.undoBtn=this.button('Undo edit',()=>this.undoClip(),'tactile-undo');this.redoBtn=this.button('Redo edit',()=>this.redoClip(),'tactile-redo');
      this.loop=n('input',{type:'checkbox',id:'tactile-loop'});const loopLabel=n('label',{class:'check-row',text:'Loop playback'});loopLabel.prepend(this.loop);
      actions.append(this.button('Copy cue',()=>{this.cueClipboard=copy(this.draft.cues[this.selected]);this.a.toast('Cue copied.');},'tactile-copy'),this.button('Paste cue',()=>this.pasteCue(),'tactile-paste'),this.undoBtn,this.redoBtn,loopLabel,this.button('Export clip',()=>this.exportClips([this.draft],'tactile-clip.json'),'tactile-export'),this.button('Export library',()=>this.exportClips(this.library,'tactile-library.json'),'tactile-export-library'),this.button('Import clips',()=>this.importClips(),'tactile-import'));
      this.body.append(actions);
      const metadata=n('details');window.controllerPlatform?.section?.(metadata,'haptics.tags','advanced','Tags and source','Clip metadata',{keywords:'tags source author license clip metadata'});metadata.append(n('summary',{text:'Tags and source'}));this.meta=n('div',{class:'game-form-grid'});metadata.append(this.meta);this.body.append(metadata);
      this.encoded=n('div',{id:'tactile-encoded',class:'tactile-encoded'});this.body.append(n('p',{class:'pc-note',text:'Load ramps force over time. Wall holds a stop. Kick briefly changes the stop; leave room for trigger travel. Release removes resistance. At the end, the previous trigger settings return. HID changes are scheduled at 10 ms intervals and can arrive later under load.'}),this.encoded);
      const library=this.panel('Clip library','tactile-library',$('haptics'),{id:'haptics.library',level:'advanced',title:'Clip library',hint:'Saved clips, import and export',keywords:'clip library tactile saved import export share'});this.search=n('input',{placeholder:'Find clips by name or tag',id:'tactile-search'});this.search.setAttribute('aria-label','Find tactile clips');this.search.addEventListener('input',()=>this.drawLibrary());this.cards=n('div',{class:'tactile-library-grid',id:'tactile-library-grid'});library.append(this.search,n('p',{class:'pc-note',text:'Clips are included in profile exports and backups. Action rules keep their own copy, so editing a library clip does not silently change a game.'}),this.cards);
      this.personalBody=this.panel('Personal feel','personal-feel',$('haptics'),{id:'haptics.personal',level:'advanced',title:'Personal feel',hint:'Your own comfortable vibration limits',keywords:'personal feel limits comfortable maximum noticeable balance grip cap strength'});
      this.triggerPreview=this.panel('Requested and encoded steps','trigger-encoded',$('triggers'),{id:'triggers.encoded',level:'tool',title:'Requested and encoded steps',hint:'What is sent to the controller',keywords:'encoded packet requested trigger steps force zones hex'});
      $('triggers').addEventListener('change',()=>this.inspectTrigger());$('triggers').addEventListener('input',()=>this.inspectTrigger());$('triggers').addEventListener('click',()=>this.inspectTrigger());
      const compare=this.panel('Audio conversion A/B','audio-conversion',$('haptics'),{id:'haptics.conversion',level:'tool',title:'Audio conversion A/B',hint:'Compare conversion methods',keywords:'audio conversion compare a/b bass music texture beats bands waveform'});
      const modes=[['music','Full soundtrack'],['bass','Bass'],['impact','Impacts'],['texture','Texture'],['beats','Beats'],['bands:1,1,1','Three-band · balanced'],['bands:1.5,1,0.5','Three-band · bass-weighted']];this.aMode=this.a.gameSelect(modes,'bass',()=>{});this.aMode.id='conversion-a';this.bMode=this.a.gameSelect(modes,'texture',()=>{});this.bMode.id='conversion-b';
      compare.append(n('p',{text:'Load an audio file above. Capture a position, then compare the same five seconds through the grips at the same mixer level.'}));
      this.anchor=n('p',{id:'conversion-anchor',text:'No comparison position captured.'});const compareControls=n('div',{class:'game-buttons'});
      compareControls.append(this.button('Capture song position',()=>{this.compareAt=this.a.getPlayer().current();this.anchor.textContent=`Comparison starts at ${this.compareAt.toFixed(1)} s`;},'conversion-capture'),this.a.gameField('A',this.aMode),this.button('Play A',()=>this.compare(this.aMode.value),'conversion-play-a'),this.a.gameField('B',this.bMode),this.button('Play B',()=>this.compare(this.bMode.value),'conversion-play-b'));compare.append(compareControls,this.anchor);
      this.previewCanvases={};const previews=n('div',{class:'conversion-previews',id:'conversion-previews'});
      for(const [slot,select] of [['A',this.aMode],['B',this.bMode]]){const canvas=document.createElement('canvas');canvas.className='conversion-preview';canvas.id='conversion-preview-'+slot.toLowerCase();canvas.setAttribute('role','img');canvas.setAttribute('aria-label',`Waveform preview for mode ${slot}`);canvas.hidden=true;this.previewCanvases[slot]=[canvas,select];select.addEventListener('change',()=>this.drawPreviews());previews.append(canvas);}
      const note=n('p',{class:'pc-note',id:'conversion-preview-note',text:'Waveform previews appear for native audio files. They show the converted energy of the whole track.'});compare.append(previews,note);this.previewNote=note;
      const css=document.createElement('style');css.textContent='.conversion-previews{display:grid;gap:4px;margin:6px 0}.conversion-preview{display:block;width:100%;height:36px;border-radius:3px;background:var(--stage-lift,#254158);color:var(--teal,#4cc3c9)}.conversion-preview[hidden]{display:none}';document.head.append(css);
      document.addEventListener('controller-view',()=>this.drawPreviews());setInterval(()=>{const path=this.a.getPlayer?.()?.nativeFile?.path||'';if(path!==this.previewPath&&!document.hidden){this.previewPath=path;this.drawPreviews();}},1500);
    }
    drawPreviews(){
      const player=this.a.getPlayer?.(),env=window.controllerEnvelope;if(!this.previewCanvases||!env)return;
      const path=player?.nativeFile?.path;
      for(const [canvas,select] of Object.values(this.previewCanvases)){
        const token=canvas.dataset.token=String((+canvas.dataset.token||0)+1);canvas.hidden=!path;if(!path)continue;
        env.draw(canvas,null);
        env.request(this.a.send,path,select.value,256,env.stamp?.(player.nativeFile)).then(values=>{if(canvas.dataset.token!==token)return;canvas.title=values?`Converted energy: ${select.selectedOptions[0]?.textContent}`:'Preview unavailable';env.draw(canvas,values);});
      }
    }
    duration(){return Math.max(...this.draft.cues.map(c=>Math.ceil((c.startMs+c.durationMs)/10)*10));}
    draw(){this.name.value=this.draft.name;this.drawTimeline();this.drawEditor();this.drawMetadata();this.drawLibrary();this.inspectClip();}
    drawTimeline(){
      const n=this.a.gameNode,total=this.duration();this.lanes.replaceChildren();this.total=total;
      const ruler=n('div',{class:'tactile-lane tactile-ruler'}),rt=n('div',{class:'tactile-track tactile-ruler-track'});ruler.append(n('strong',{text:'ms'}),rt);
      const step=[50,100,250,500,1000].find(v=>total/v<=10)||1000;
      for(let t=0;t<=total;t+=step){const m=n('span',{class:'tactile-tick',text:String(t)});if(t+step>total){m.style.left='auto';m.style.right='0';m.style.borderLeft='0';m.style.borderRight='1px solid var(--line,#a8bec1)';m.style.paddingLeft='0';m.style.paddingRight='2px';}else m.style.left=`${t/total*100}%`;rt.append(m);}
      this.lanes.append(ruler);
      // Hide ticks that would collide with the previous visible label at narrow widths.
      requestAnimationFrame(()=>{let edge=-Infinity;for(const tick of rt.querySelectorAll('.tactile-tick')){const box=tick.getBoundingClientRect();if(!box.width)continue;if(box.left<edge+4&&tick!==rt.lastElementChild)tick.style.visibility='hidden';else edge=box.right;}const last=rt.lastElementChild;if(last){const box=last.getBoundingClientRect();let prev=last.previousElementSibling;while(prev&&prev.style.visibility==='hidden')prev=prev.previousElementSibling;if(prev&&prev.getBoundingClientRect().right+4>box.left)prev.style.visibility='hidden';}});
      for(const [lane,label] of lanes){const row=n('div',{class:'tactile-lane'}),track=n('div',{class:'tactile-track'});row.dataset.lane=lane;row.append(n('strong',{text:label}),track);
        this.draft.cues.forEach((c,i)=>{if(c.lane!==lane)return;const b=this.button('',()=>{if(this.dragged){this.dragged=false;return;}this.selected=i;this.drawTimeline();this.drawEditor();});b.className='tactile-cue is-'+c.kind+(i===this.selected?' is-selected':'');b.style.left=`${c.startMs/total*100}%`;b.style.width=`${c.durationMs/total*100}%`;b.title=`${label}: ${c.kind}, ${c.startMs}–${c.startMs+c.durationMs} ms. Drag to move, drag the right edge to resize, or use arrow keys (Shift resizes).`;b.setAttribute('aria-label',b.title);this.attachDrag(b,c,i,track,total);track.append(b);});
        track.append(n('div',{class:'tactile-playhead',hidden:true}));
        this.lanes.append(row,n('p',{class:'tactile-caption',text:this.draft.cues.filter(c=>c.lane===lane).map(c=>`${c.kind} ${c.startMs} to ${c.startMs+c.durationMs} ms / ${c.strength}%`).join(' / ')||'No cues'}));}
      this.lanes.append(n('p',{class:'pc-note',text:`0–${total} ms · ${this.draft.cues.length}/32 cues · positions snap to 10 ms`}));
    }
    attachDrag(b,c,i,track,total){
      const snap=v=>Math.round(v/10)*10;
      b.addEventListener('pointerdown',e=>{
        if(e.button!==0)return;const rect=track.getBoundingClientRect(),bw=b.getBoundingClientRect(),resize=e.clientX>bw.right-Math.min(8,bw.width/3);
        const startX=e.clientX,start0=c.startMs,dur0=c.durationMs,before=JSON.stringify(this.draft);let moved=false;
        try{b.setPointerCapture(e.pointerId);}catch{}
        const move=ev=>{const dms=snap((ev.clientX-startX)/Math.max(1,rect.width)*total);if(!moved&&Math.abs(ev.clientX-startX)<3)return;moved=true;
          if(resize)c.durationMs=Math.max(10,Math.min(3000,8000-c.startMs,dur0+dms));else c.startMs=Math.max(0,Math.min(7990,8000-c.durationMs,start0+dms));
          b.style.left=`${c.startMs/total*100}%`;b.style.width=`${c.durationMs/total*100}%`;};
        const up=()=>{b.removeEventListener('pointermove',move);b.removeEventListener('pointerup',up);b.removeEventListener('pointercancel',up);
          if(moved){this.dragged=true;setTimeout(()=>this.dragged=false,0);this.selected=i;if(!validClip(this.draft)){this.draft=JSON.parse(before);this.a.toast('That would overlap another trigger cue; move undone.');}else this.noteChange(before);this.drawTimeline();this.drawEditor();this.inspectClip();}};
        b.addEventListener('pointermove',move);b.addEventListener('pointerup',up);b.addEventListener('pointercancel',up);
      });
      b.addEventListener('keydown',e=>{
        if(!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();const d=e.key==='ArrowLeft'?-10:10,before=JSON.stringify(this.draft);
        if(e.shiftKey)c.durationMs=Math.max(10,Math.min(3000,8000-c.startMs,c.durationMs+d));else c.startMs=Math.max(0,Math.min(7990,8000-c.durationMs,c.startMs+d));
        if(!validClip(this.draft)){this.draft=JSON.parse(before);this.a.toast('That would overlap another trigger cue.');}else this.noteChange(before);
        this.selected=i;this.drawTimeline();this.drawEditor();this.inspectClip();this.lanes.querySelectorAll('.tactile-cue.is-selected')[0]?.focus();
      });
    }
    noteChange(before,coalesce=false){
      const now=JSON.stringify(this.draft);if(before===now)return;
      const t=performance.now(),stack=this.undoStack||(this.undoStack=[]);
      // Rapid typing in one field is one undo step; drags, arrows and discrete edits always push.
      if(!(coalesce&&stack.length&&t-(this.lastNoteAt||0)<600)){stack.push(before);if(stack.length>50)stack.shift();}
      this.lastNoteAt=t;this.lastJson=now;this.redoStack=[];this.syncHistoryButtons();
    }
    resetHistory(){this.undoStack=[];this.redoStack=[];this.lastJson=JSON.stringify(this.draft);this.syncHistoryButtons();}
    syncHistoryButtons(){if(this.undoBtn){this.undoBtn.disabled=!this.undoStack?.length;this.redoBtn.disabled=!this.redoStack?.length;}}
    undoClip(){if(!this.undoStack?.length)return;(this.redoStack||(this.redoStack=[])).push(JSON.stringify(this.draft));this.draft=JSON.parse(this.undoStack.pop());this.selected=Math.min(this.selected,this.draft.cues.length-1);this.lastJson=JSON.stringify(this.draft);this.draw();this.syncHistoryButtons();}
    redoClip(){if(!this.redoStack?.length)return;this.undoStack.push(JSON.stringify(this.draft));this.draft=JSON.parse(this.redoStack.pop());this.selected=Math.min(this.selected,this.draft.cues.length-1);this.lastJson=JSON.stringify(this.draft);this.draw();this.syncHistoryButtons();}
    changed(){this.noteChange(this.lastJson,true);this.drawTimeline();this.inspectClip();}
    pasteCue(){
      const src=this.cueClipboard;if(!src)return this.a.toast('Copy a cue first.');
      if(this.draft.cues.length>=32)return this.a.toast('32 cue limit.');
      const c=copy(src),end=Math.max(0,...this.draft.cues.filter(v=>v.lane===c.lane).map(v=>v.startMs+v.durationMs));
      c.startMs=Math.min(7990,Math.ceil(end/10)*10);if(c.startMs+c.durationMs>8000)c.durationMs=Math.max(10,8000-c.startMs);
      this.draft.cues.push(c);
      if(!validClip(this.draft)){this.draft.cues.pop();return this.a.toast('The pasted cue would overlap another trigger cue or exceed 8 s.');}
      this.selected=this.draft.cues.length-1;this.noteChange(this.lastJson);this.draw();this.a.toast('Cue pasted at the end of its lane.');
    }
    exportClips(clips,name){
      window.controllerPlatform.download(name,JSON.stringify({kind:'controller-tactile-clips',version:1,clips:copy(clips)},null,2));
      this.a.toast(`Exported ${clips.length} clip${clips.length===1?'':'s'}.`);
    }
    async importClips(){
      const data=await window.controllerPlatform.pickJson();if(data===null)return;
      const list=Array.isArray(data)?data:Array.isArray(data?.clips)?data.clips:data&&data.cues?[data]:undefined;
      if(!list)return this.a.toast('That file is not a tactile clip or clip library.');
      const sig=c=>JSON.stringify([c.name,c.cues]);const have=new Set(this.library.map(sig));let added=0,dup=0,bad=0;const next=copy(this.library);
      for(const raw of list){const c=sanitizeClip(raw);if(!validClip(c)){bad++;continue;}if(have.has(sig(c))){dup++;continue;}const clip=copy(c);if(next.some(v=>v.id===clip.id))clip.id=crypto.randomUUID();if(next.length>=64){bad++;continue;}next.push(clip);have.add(sig(clip));added++;}
      if(!added)return this.a.toast(`Nothing imported: ${dup} duplicate${dup===1?'':'s'}, ${bad} invalid or over the 64 clip limit.`);
      const old=this.library;this.library=next;
      if(await this.a.saveProfiles(true)){this.drawLibrary();this.a.toast(`Imported ${added}; skipped ${dup} duplicate${dup===1?'':'s'}${bad?`, ${bad} invalid`:''}.`);}else{this.library=old;this.drawLibrary();}
    }
    explain(c){
      const lane=l=>lanes.find(([k])=>k===l)?.[1]||l;
      if(!c.name||!c.name.trim())return 'Give the clip a name.';
      if(c.name.length>40)return 'Clip name is longer than 40 characters.';
      if(c.tags.length>8)return `Use at most 8 tags (you have ${c.tags.length}).`;
      const longTag=c.tags.find(t=>t.length>24);if(longTag)return `Tag "${longTag.slice(0,24)}..." is longer than 24 characters.`;
      if(c.sourceUrl&&!/^https:\/\/[^\s]+$/.test(c.sourceUrl))return 'Clip source must be a full https:// address with no spaces.';
      for(const [i,v] of c.cues.entries()){
        const tag=`Cue ${i+1} (${lane(v.lane)} ${v.kind})`;
        if(!Number.isInteger(v.startMs)||v.startMs<0||v.startMs>7990)return `${tag}: start must be a whole number from 0 to 7990 ms.`;
        if(!Number.isInteger(v.durationMs)||v.durationMs<10||v.durationMs>3000)return `${tag}: duration must be a whole number from 10 to 3000 ms.`;
        if(v.startMs+v.durationMs>8000)return `${tag}: ends at ${v.startMs+v.durationMs} ms; clips are limited to 8000 ms.`;
        if(!(v.lane.endsWith('Grip')?['pulse','texture']:['load','wall','kick','release']).includes(v.kind))return `${tag}: this phase does not work on that lane.`;
        if(v.endZone<=v.startZone)return `${tag}: end zone must come after the start zone.`;
        if(!Number.isInteger(v.frequency)||v.frequency<20||v.frequency>300)return `${tag}: frequency must be a whole number from 20 to 300 Hz.`;
      }
      const s=v=>Math.ceil(v.startMs/10)*10,e=v=>Math.ceil((v.startMs+v.durationMs)/10)*10;
      for(const [i,a] of c.cues.entries())if(a.lane.endsWith('Trigger'))for(let j=i+1;j<c.cues.length;j++){const b=c.cues[j];if(a.lane===b.lane&&s(a)<e(b)&&s(b)<e(a))return `Cues ${i+1} and ${j+1} overlap on ${lane(a.lane)}. Trigger phases must follow each other.`;}
      return 'Check cue timing, trigger overlaps, tags and source URL.';
    }
    drawEditor(){
      const c=this.draft.cues[this.selected],grip=c.lane.endsWith('Grip');this.editor.replaceChildren();
      this.editor.append(this.select('Lane',lanes,c.lane,v=>{c.lane=v;c.kind=v.endsWith('Grip')?'pulse':'wall';this.drawEditor();this.changed();}),this.select('Phase',(grip?['pulse','texture']:['load','wall','kick','release']).map(v=>[v,v[0].toUpperCase()+v.slice(1)]),c.kind,v=>{c.kind=v;this.drawEditor();this.changed();}),this.number('Start · ms',c.startMs,0,7990,v=>{c.startMs=v;this.changed();}),this.number('Duration · ms',c.durationMs,10,3000,v=>{c.durationMs=v;this.changed();}),this.number('Strength · %',c.strength,0,100,v=>{c.strength=v;this.changed();},.1));
      if(c.kind==='load')this.editor.append(this.number('Initial force · %',c.fromStrength,0,100,v=>{c.fromStrength=v;this.changed();},.1));
      if(grip)this.editor.append(this.number('Frequency · Hz',c.frequency,20,300,v=>{c.frequency=v;this.changed();}),this.select('Texture',['sine','triangle','square','saw','noise','pulse','double','swell'].map(v=>[v,({sine:'Smooth sine',triangle:'Soft triangle',square:'Sharp square',saw:'Snap saw',noise:'Rough noise',pulse:'Pulse',double:'Double tap',swell:'Swell'})[v]]),c.shape,v=>{c.shape=v;this.changed();}));
      else this.editor.append(this.number('Start zone · 1–8',c.startZone+1,1,8,v=>{c.startZone=v-1;this.changed();}),this.number('End zone · 2–9',c.endZone+1,2,9,v=>{c.endZone=v-1;this.changed();}));
    }
    drawMetadata(){const c=this.draft;this.meta.replaceChildren(this.text('Tags · comma separated',c.tags.join(', '),200,v=>c.tags=v.split(',').map(s=>s.trim()).filter(Boolean)),this.text('Clip author',c.author,80,v=>c.author=v),this.text('Clip license',c.license,120,v=>c.license=v),this.text('Clip source · HTTPS',c.sourceUrl,1024,v=>c.sourceUrl=v));}
    drawLibrary(){
      const n=this.a.gameNode,query=this.search.value.toLowerCase();this.cards.replaceChildren();
      for(const clip of this.library.filter(c=>[c.name,...c.tags].join(' ').toLowerCase().includes(query))){const card=n('article',{class:'tactile-library-card'}),buttons=n('div',{class:'game-buttons'});card.append(n('h3',{text:clip.name}),n('p',{text:clip.tags.join(' / ')||'No tags'}),n('small',{text:`${clip.author||'Unknown author'} · ${clip.license||'No reuse terms recorded'}`}));
        buttons.append(this.button('Load',()=>{this.draft=copy(clip);this.selected=0;this.resetHistory();this.draw();}),this.button('Use in new action',()=>{const p=this.a.profile();if(p.actions.length>=24)return this.a.toast('24 action limit.');p.actions.push({id:crypto.randomUUID(),name:clip.name,source:'press',button:1,event:'',windowMs:280,cooldownMs:180,durationMs:120,haptic:null,leftTrigger:null,rightTrigger:null,timeline:copy(clip)});this.a.saveProfiles();this.a.renderGameProfiles();this.a.navigate('profiles');this.a.toast('Choose the input, then Save and apply.');}),this.button('Export',()=>this.exportClips([clip],`${clip.name.replace(/[^a-z0-9]+/gi,'-').toLowerCase()||'clip'}.json`)),this.button('Delete',async()=>{
          if(!await window.controllerPlatform.confirm(`Delete the clip "${clip.name}" from your library? Action rules keep their own copy.`,{title:'Delete clip',confirmLabel:'Delete clip'}))return;
          const old=this.library;this.library=this.library.filter(c=>c.id!==clip.id);if(!await this.a.saveProfiles(true)){this.library=old;this.drawLibrary();return;}this.drawLibrary();
          window.controllerPlatform.undoToast(`Deleted "${clip.name}".`,async()=>{if(this.library.some(c=>c.id===clip.id)||this.library.length>=64)return;const prev=this.library;this.library=[...this.library,clip];if(!await this.a.saveProfiles(true))this.library=prev;this.drawLibrary();});
        }));card.append(buttons);this.cards.append(card);}
      if(!this.cards.children.length)this.cards.append(n('p',{text:'No matching clips.'}));
    }
    restore(clips){this.library=clips==null?builtins():copy(clips);this.drawLibrary();}
    archive(){return copy(this.library);}
    async save(update){
      const c=copy(this.draft);if(!update)c.id=crypto.randomUUID();
      if(!validClip(c)){const m=this.explain(c);this.status.textContent=m;return this.a.toast(m);}
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
        const table=document.createElement('table');table.className='tactile-compare';table.innerHTML='<thead><tr><th>Cue</th><th>Requested (start+length)</th><th>Encoded (start+length)</th><th>Force steps</th></tr></thead>';const tb=document.createElement('tbody');
        for(const c of result.cues){const tr=document.createElement('tr'),cell=t=>{const td=document.createElement('td');td.textContent=t;tr.append(td);return td;};cell(`${lanes.find(([l])=>l===c.lane)?.[1]} ${c.kind}`);cell(`${c.requestedStartMs}+${c.requestedDurationMs} ms`);const enc=cell(`${c.startMs}+${c.durationMs} ms`);if(c.startMs!==c.requestedStartMs||c.durationMs!==c.requestedDurationMs)enc.className='shift';cell(c.levels?c.levels.join(' / '):'-');tb.append(tr);}
        table.append(tb);this.encoded.append(table);
      }catch(error){if(id===this.previewId)this.encoded.textContent=String(error);}
    },120);}
    inspectTrigger(){clearTimeout(this.triggerTimer);const id=++this.triggerRequest;this.triggerTimer=setTimeout(async()=>{
      try{const result=this.a.native?await this.a.invoke('preview_authoring',{clip:null,trigger:copy(this.a.trigger()),personal:this.personal}):null;if(id!==this.triggerRequest)return;
        if(!result){this.triggerPreview.textContent='Open the desktop app to inspect encoded trigger steps.';return;}
        const requested=this.a.trigger(),n=this.a.gameNode;this.triggerPreview.replaceChildren(n('p',{text:`Requested: ${requested.mode}, ${requested.strength}% · encoded force limit: ${result.triggerLimit}/8`}),n('p',{text:`Encoded zone steps: ${result.levels.join(' / ')}`}),n('p',{text:`Packet: ${result.packet.map(b=>b.toString(16).padStart(2,'0')).join(' ')}${result.frequencyHz?' · '+result.frequencyHz+' Hz':''}`}),n('p',{class:'pc-note',text:'Preview only; Save personal feel and Apply triggers to send changes. '+result.note}));
      }catch(error){this.triggerPreview.textContent=String(error);}
    },120);}
    device(event){
      const owner=event.controllerId||event.path||`${event.name}/${event.transport}`;const resume=this.lastOwner===owner&&this.guideStep>0&&this.guideStep<4;this.owner=owner;this.lastOwner=owner;
      if(!resume){this.personal=defaults();this.guideStep=0;this.guideStrength=null;
      try{const stored=JSON.parse(localStorage.getItem('controller.personalFeel.v1')||'{}');if(stored[owner]&&this.validPersonal(stored[owner]))this.personal=stored[owner];}catch{}}
      this.personalForm();this.personalStatus.textContent='Applying personal limits...';this.a.send({action:'setPersonalHaptics',settings:copy(this.personal),requestId:++this.personalRequest});this.inspectClip();this.inspectTrigger();
    }
    validPersonal(s){return s&&typeof s.enabled==='boolean'&&Array.isArray(s.minimum)&&s.minimum.length===2&&Array.isArray(s.maximum)&&s.maximum.length===2&&s.minimum.every((v,i)=>Number.isInteger(v)&&v>=0&&v<s.maximum[i]&&Number.isInteger(s.maximum[i])&&s.maximum[i]<=100)&&Number.isInteger(s.balance)&&s.balance>=-100&&s.balance<=100&&Number.isInteger(s.triggerLimit)&&s.triggerLimit>=1&&s.triggerLimit<=8;}
    personalForm(){
      const n=this.a.gameNode,s=this.personal,grid=n('div',{class:'game-form-grid'}),enabled=n('input',{type:'checkbox',checked:s.enabled,id:'personal-enabled'}),label=n('label',{class:'check-row',text:'Use personal limits'});enabled.addEventListener('change',()=>s.enabled=enabled.checked);label.prepend(enabled);grid.append(label);
      for(const [i,side] of ['Left','Right'].entries())grid.append(this.number(`${side} noticeable · %`,s.minimum[i],0,99,v=>s.minimum[i]=v),this.number(`${side} comfortable maximum · %`,s.maximum[i],1,100,v=>s.maximum[i]=v));
      grid.append(this.number('Balance · left −100 / right 100',s.balance,-100,100,v=>s.balance=v),this.number('Trigger force limit · 1–8',s.triggerLimit,1,8,v=>s.triggerLimit=v));
      const validation=n('p',{id:'personal-validation',class:'personal-validation',role:'status'});
      const checkLive=()=>{const v=[...grid.querySelectorAll('input[type=number]')].map(i=>Number(i.value));const bad=[];for(const [i,side] of ['Left','Right'].entries()){const lo=v[i*2],hi=v[i*2+1];if(!(lo<hi))bad.push(`${side}: noticeable (${lo}%) must be below comfortable maximum (${hi}%).`);}validation.textContent=bad.join(' ');validation.classList.toggle('bad',!!bad.length);};
      grid.addEventListener('input',checkLive);grid.addEventListener('change',checkLive);
      this.personalStatus=n('p',{id:'personal-status',role:'status',text:this.owner?'Saved for this controller.':'Connect a controller to save personal limits.'});
      const save=this.button('Save personal feel',async()=>{if(!this.owner||!this.a.state.connected)return this.a.toast('Connect a controller.');if(!this.validPersonal(s))return this.a.toast('Noticeable strength must be below the comfortable maximum.');try{const stored=JSON.parse(localStorage.getItem('controller.personalFeel.v1')||'{}');stored[this.owner]=copy(s);localStorage.setItem('controller.personalFeel.v1',JSON.stringify(stored));}catch{this.a.toast('Local storage is unavailable; changes will last for this session.');}this.personalStatus.textContent='Applying personal limits...';const request=++this.personalRequest;if(!await this.a.send({action:'setPersonalHaptics',settings:copy(s),requestId:request})&&request===this.personalRequest)this.personalStatus.textContent='Controller update failed. Reconnect to apply saved limits.';this.inspectClip();this.inspectTrigger();},'personal-save');
      this.personalBody.replaceChildren(validation,n('p',{text:'Measure what you can feel, then choose a comfortable maximum for each grip. These are personal preferences, not measured force. Saved grip caps affect all app feedback; the trigger cap limits encoded force steps and stays in place after Reset.'}),grid,save,this.personalStatus);
      this.guideStep=this.guideStep||0;const guide=n('div',{class:'personal-guide'}),step=n('p',{id:'personal-guide-step'}),strength=n('input',{type:'number',value:this.guideStrength??(this.guideStep<2?10:40),min:0,max:100,id:'personal-test-strength'});strength.addEventListener('input',()=>this.guideStrength=Number(strength.value));strength.setAttribute('aria-label','Raw test strength');const stages=['Find the faintest left-grip pulse you can feel.','Find the faintest right-grip pulse you can feel.','Find a comfortable left-grip maximum. Start low.','Find a comfortable right-grip maximum. Start low.'];
      const test=this.button('Test grip · USB',()=>{const value=Number(strength.value);if(!Number.isInteger(value)||value<0||value>100)return this.a.toast('Choose 0–100%.');if(!this.a.state.connected)return this.a.toast('Connect a controller.');this.a.send({action:'playPersonalTest',side:this.guideStep%2,strength:value});},'personal-test');
      const next=this.button('Record & next',()=>{const value=Number(strength.value);if(!Number.isInteger(value)||value<0||value>100)return this.a.toast('Choose 0–100%.');if(this.guideStep<2)s.minimum[this.guideStep]=value;else s.maximum[this.guideStep-2]=value;this.guideStep++;if(this.guideStep>=4){this.guideStep=0;this.guideStrength=null;this.personalForm();this.personalStatus.textContent='Measurements captured. Check the values, enable limits, then Save personal feel.';return;}strength.value=this.guideStep<2?10:40;this.guideStrength=Number(strength.value);step.textContent=`${this.guideStep+1}/4 · ${stages[this.guideStep]}`;},'personal-next');
      step.textContent=`${this.guideStep+1}/4 · ${stages[this.guideStep]}`;checkLive();guide.append(step,n('p',{class:'pc-note',text:'Each test briefly solos one grip at raw strength, bypassing personal and mixer levels. The app’s fixed peak protection remains active. Use USB. Nothing plays until you press Test.'}),strength,test,next);this.personalBody.append(guide);
    }
    async compare(kind){const player=this.a.getPlayer();if(!player.nativeFile)return this.a.toast('Choose a native audio file above to compare conversion modes.');if(this.compareAt==null){this.compareAt=player.current();this.anchor.textContent=`Comparison starts at ${this.compareAt.toFixed(1)} s`;}this.drawPreviews();const result=await player.audition(kind,this.compareAt,5);if(result===undefined||result===false)this.a.toast(document.getElementById('speaker-file-status')?.textContent||'Could not start the comparison.');}
    renderPlayback(){
      const e=this.lastPlayback;if(!e||e.playId!==this.playId||document.hidden||this.a.state.currentView!=='haptics')return;
      this.progress.max=e.durationMs;this.progress.value=e.positionMs;
      const pct=Math.max(0,Math.min(100,e.positionMs/Math.max(1,this.total||e.durationMs)*100));this.lanes.querySelectorAll('.tactile-playhead').forEach(h=>{h.hidden=!e.playing;h.style.left=`${pct}%`;});this.status.textContent=e.error||(e.playing?`${e.name||'Clip'} \u00b7 ${e.positionMs}/${e.durationMs} ms`:'Clip finished');
    }
    event(e){
      if(e.type==='envelope')window.controllerEnvelope?.handle(e);
      if(e.type==='personalHapticsStatus'&&e.requestId===this.personalRequest)this.personalStatus.textContent=e.applied?'Personal limits applied.':'Controller update failed: '+(e.message||'Reconnect and try again.');
      if(e.type==='effectTimeline'&&e.playId===this.playId){this.lastPlayback=e;const finished=this.playing&&!e.playing&&!e.error;this.playing=!!e.playing;this.renderPlayback();if(!e.playing)this.lanes.querySelectorAll('.tactile-playhead').forEach(h=>h.hidden=true);if(finished&&this.loop?.checked)this.playClip();}
      if(e.type==='safetyReset'||e.type==='device'&&!e.connected){this.playId++;this.playing=false;this.progress.value=0;this.status.textContent='Stopped';if(e.type==='device')this.owner='';}
    }
  }
  window.ControllerHapticAuthoring=ControllerHapticAuthoring;
})();
