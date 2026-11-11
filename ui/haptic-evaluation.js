/* Reproducible listening trials. A session prepares a plan; Play is always explicit. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id),copy=value=>JSON.parse(JSON.stringify(value));
  function plan(seed,trials=4){
    let random=seed>>>0;const next=()=>{random^=random<<13;random^=random>>>17;random^=random<<5;return (random>>>0)/4294967296;};
    const order=Array.from({length:trials},(_,i)=>i<Math.ceil(trials/2)?0:1);for(let i=order.length-1;i>0;i--){const j=Math.floor(next()*(i+1));[order[i],order[j]]=[order[j],order[i]];}
    return order.map(swap=>swap?['b','a']:['a','b']);
  }
  class ControllerHapticEvaluation {
    constructor(api){
      this.a=api;this.generation=0;
      const n=api.gameNode,box=n('section',{id:'haptic-evaluation',class:'work-panel framed tactile-panel evaluation-lab'}),body=n('div',{class:'game-content'});
      box.append(n('div',{class:'panel-cap',text:'Haptic comparison'}),body);window.controllerPlatform?.section?.(box,'haptics.evaluation','tool','Wireless haptics comparison','Blind comparison of test trials',{keywords:'blind comparison trials evaluation a/b conversion haptic listening study export results'});$('haptics').append(box);
      body.append(n('p',{text:'Compare two audio conversions in four blinded trials. Each plays the same five seconds at your current levels. Keep the source file with the report.'}));
      const setup=n('div',{class:'evaluation-setup'}),modes=[['music','Full range'],['bass','Bass'],['impact','Impacts'],['texture','Texture']];
      let saved={};try{saved=JSON.parse(localStorage.getItem('controller.evaluationSetup'))||{};}catch{}
      this.modeA=api.gameSelect(modes,modes.some(m=>m[0]===saved.a)?saved.a:'music');this.modeB=api.gameSelect(modes,modes.some(m=>m[0]===saved.b)?saved.b:'texture');
      this.seed=n('input',{id:'evaluation-seed',type:'number',min:1,max:4294967295,step:1,value:Number.isInteger(saved.seed)&&saved.seed>=1?saved.seed:137});
      this.trialCount=api.gameSelect([[2,'2 trials'],[4,'4 trials'],[6,'6 trials'],[8,'8 trials']],[2,4,6,8].includes(saved.trials)?saved.trials:4);this.trialCount.id='evaluation-trials';
      const persist=()=>{try{localStorage.setItem('controller.evaluationSetup',JSON.stringify({a:this.modeA.value,b:this.modeB.value,seed:Number(this.seed.value),trials:Number(this.trialCount.value)}));}catch{}};
      for(const el of [this.modeA,this.modeB,this.seed,this.trialCount])el.addEventListener('change',persist);
      setup.append(api.gameField('A conversion',this.modeA),api.gameField('B conversion',this.modeB),api.gameField('Shuffle seed',this.seed),api.gameField('Trials',this.trialCount));
      this.begin=this.button('Prepare trials','evaluation-start',()=>this.start());
      this.cancelButton=this.button('Cancel','evaluation-cancel',()=>this.cancel('Comparison cancelled.'));this.cancelButton.disabled=true;
      this.playX=this.button('Play X','evaluation-x',()=>this.play('x'));this.playY=this.button('Play Y','evaluation-y',()=>this.play('y'));
      this.playX.disabled=this.playY.disabled=true;
      const actions=n('div',{class:'game-buttons'});actions.append(this.begin,this.playX,this.playY,this.cancelButton);
      this.status=n('p',{id:'evaluation-status',role:'status',text:'Choose a USB audio file, set the song position, then prepare trials. Nothing plays automatically.'});
      const scores=n('div',{class:'evaluation-scores'});this.scores={};
      const gs=document.createElement('style');gs.textContent='.evaluation-scores{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:6px 10px;align-items:end}.evaluation-scores select{width:100%}.evaluation-summary{font:.85rem var(--mono,monospace);white-space:pre-line}';document.head.append(gs);
      for(const slot of ['x','y'])for(const metric of ['strength','clarity']){
        const select=n('select',{id:`evaluation-${slot}-${metric}`});select.append(n('option',{value:'',text:'Choose…'}));for(let i=0;i<=5;i++)select.append(n('option',{value:String(i),text:String(i)}));
        select.setAttribute('aria-label',`${slot.toUpperCase()} ${metric}`);this.scores[`${slot}-${metric}`]=select;scores.append(api.gameField(`${slot.toUpperCase()} ${metric} · 0–5`,select));
      }
      this.preference=api.gameSelect([['','Choose…'],['x','X'],['y','Y'],['tie','No preference']],'');this.preference.id='evaluation-preference';scores.append(api.gameField('Preferred feel',this.preference));
      this.save=this.button('Record & next','evaluation-next',()=>this.record());this.save.disabled=true;
      this.exportButton=this.button('Export results','evaluation-export',()=>this.export());this.exportButton.disabled=true;
      this.reveal=this.button('Reveal order','evaluation-reveal',()=>this.revealOrder());this.reveal.disabled=true;
      const results=n('div',{class:'game-buttons'});results.append(this.save,this.exportButton,this.reveal);
      this.result=n('p',{id:'evaluation-results',class:'pc-note'});body.append(setup,actions,this.status,scores,results,this.result);
      document.addEventListener('controller-player-state',()=>{
        const player=this.a.getPlayer(),active=this.active;
        if(active?.trackId!=null&&(player.trackId!==active.trackId||['paused','stopped'].includes(player.mode)))this.cancel('Playback was interrupted. Prepare new trials.',false);
      });
    }
    button(text,id,run){const button=this.a.gameNode('button',{type:'button',id,class:'quiet-action',text});button.addEventListener('click',run);return button;}
    context(){
      const player=this.a.getPlayer(),audio=window.runtimeLab?.audio;
      return {controllerId:this.a.state.controllerId,transport:this.a.state.transport,firmware:audio?.controllerId===this.a.state.controllerId?audio.firmware||null:null,
        source:player.nativeFile?{name:player.nativeFile.name,path:player.nativeFile.path,duration:player.duration,sampleRate:player.nativeFile.sampleRate,channels:player.nativeFile.channels}:null,
        mixer:this.a.getMixer(),requestedPersonal:copy(window.hapticAuthoring.personal),
        appliedPersonal:audio?.controllerId===this.a.state.controllerId?audio.personal||null:null,appliedMixer:audio?.controllerId===this.a.state.controllerId?audio.mixer||null:null};
    }
    sameContext(){return this.session&&JSON.stringify(this.session.context)===JSON.stringify(this.context());}
    start(){
      if(this.session&&!this.session.finished&&!this.session.cancelled)return this.a.toast('Finish or cancel the current trials.');
      const player=this.a.getPlayer(),seed=Number(this.seed.value),audio=window.runtimeLab?.audio;
      if(!this.a.state.connected||this.a.state.transport!=='USB'||!player.nativeFile)return this.a.toast('Connect by USB and choose a native audio file.');
      if(audio?.controllerId!==this.a.state.controllerId||!audio.personal||!audio.mixer)return this.a.toast('Waiting for controller output settings. Try again.');
      if(!Number.isInteger(seed)||seed<1||seed>4294967295)return this.a.toast('Enter a shuffle seed from 1 to 4294967295.');
      if(this.modeA.value===this.modeB.value)return this.a.toast('Choose two different conversions.');
      if(['playing','buffering','starting'].includes(player.mode))return this.a.toast('Pause playback before preparing trials.');
      if(audio&&(audio.bridge||audio.gameActive||audio.timelineActive||audio.cueActive))return this.a.toast('Stop other haptic sources before comparing.');
      if(player.duration-player.current()<5)return this.a.toast('Choose a position with at least five seconds left.');
      const trialCount=Number(this.trialCount.value)||4;
      this.generation++;this.session={schemaVersion:1,appVersion:document.querySelector('.side-code')?.textContent,createdAt:new Date().toISOString(),planVersion:1,seed,order:plan(seed,trialCount),trialCount,modes:{a:this.modeA.value,b:this.modeB.value},position:player.current(),duration:5,context:this.context(),trials:[],playbacks:[],index:0,heard:{},cancelled:false,finished:false};
      this.begin.disabled=true;this.cancelButton.disabled=false;this.playX.disabled=this.playY.disabled=false;this.reveal.disabled=true;this.exportButton.disabled=false;this.result.textContent='';document.getElementById('evaluation-summary')?.remove();
      for(const control of [this.seed,this.modeA,this.modeB,this.trialCount])control.disabled=true;
      this.showTrial();
    }
    showTrial(){
      this.session.heard={};this.save.disabled=true;for(const select of Object.values(this.scores))select.value='';this.preference.value='';
      this.status.textContent=`Trial ${this.session.index+1}/${this.session.trialCount}. Play X and Y, then score strength and clarity. 0 means none; 5 means very strong or very clear.`;
    }
    async play(slot){
      if(!this.session||this.session.finished||this.session.cancelled||this.active)return;
      if(!this.sameContext())return this.cancel('Source or output settings changed. Prepare a new comparison.');
      const player=this.a.getPlayer(),epoch=this.generation,session=this.session;
      const record={trial:session.index+1,slot,requestedAt:new Date().toISOString(),completed:false};session.playbacks.push(record);
      const active={slot,epoch,played:false,record};this.active=active;this.playX.disabled=this.playY.disabled=true;this.save.disabled=true;
      this.status.textContent=`Trial ${session.index+1}/${session.trialCount} · ${slot.toUpperCase()} opening…`;
      const key=session.order[session.index][slot==='x'?0:1];
      const pending=player.audition(session.modes[key],session.position,5,id=>{if(this.generation===epoch){active.trackId=id;record.trackId=id;}});
      active.stopId=player.trackId;
      try{await pending;if(this.generation!==epoch)return;if(!active.trackId)throw Error('Playback did not start.');}
      catch(error){if(this.generation===epoch)this.cancel(`Comparison stopped: ${error.message}`);}
    }
    event(e){
      if(!this.session||this.session.finished||this.session.cancelled)return;
      if(e.type==='safetyReset'||e.type==='device'&&(!e.connected||e.controllerId!==this.session.context.controllerId))return this.cancel('Controller changed or output was reset.',false);
      if(e.type==='speakerStatus'&&e.source==='error'&&e.trackId===this.active?.trackId)return this.cancel('Playback failed. Prepare new trials.',false);
      if(e.type!=='audioMeters')return;
      if(!this.sameContext())return this.cancel('Source or output settings changed. Prepare a new comparison.');
      const active=this.active,file=e.file;if(!active||file?.trackId!==active.trackId)return;
      if(['playing','buffering'].includes(file.mode))active.played=true;
      if(file.mode==='finished'){
        if(!active.played)return this.cancel('No playback was confirmed. Try the file player first.',false);
        active.record.completed=true;active.record.completedAt=new Date().toISOString();
        this.session.heard[active.slot]={trackId:active.trackId,outputCallbacks:e.callbacks||null};this.active=null;
        this.playX.disabled=this.playY.disabled=false;this.save.disabled=!(this.session.heard.x&&this.session.heard.y);
        this.status.textContent=`Trial ${this.session.index+1}/${this.session.trialCount} · ${active.slot.toUpperCase()} finished. ${this.save.disabled?'Play the other sample.':'Score both samples, then record.'}`;
      }
    }
    record(){
      const s=this.session;if(!s||s.finished||s.cancelled||this.active||!s.heard.x||!s.heard.y)return;
      if(!this.sameContext())return this.cancel('Source or output settings changed. Prepare new trials.');
      const scores={};for(const [key,select] of Object.entries(this.scores)){const value=Number(select.value);if(select.value===''||!Number.isInteger(value)||value<0||value>5)return this.a.toast('Score strength and clarity for X and Y.');scores[key]=value;}
      if(!['x','y','tie'].includes(this.preference.value))return this.a.toast('Choose X, Y or No preference.');
      s.trials.push({trial:s.index+1,scores,preference:this.preference.value,heard:copy(s.heard),recordedAt:new Date().toISOString()});s.index++;
      if(s.index<s.trialCount)this.showTrial();else{s.finished=true;this.finishControls();this.reveal.disabled=false;this.status.textContent=`${s.trialCount===4?'Four':s.trialCount} trials recorded. Reveal the order or export your results.`;}
    }
    finishControls(){
      this.begin.disabled=false;this.cancelButton.disabled=true;this.playX.disabled=this.playY.disabled=this.save.disabled=true;
      for(const control of [this.seed,this.modeA,this.modeB,this.trialCount])control.disabled=false;
    }
    cancel(message='Comparison cancelled.',stop=true){
      if(!this.session||this.session.finished||this.session.cancelled)return;
      const player=this.a.getPlayer(),active=this.active;
      this.generation++;this.session.cancelled=true;this.session.cancelReason=message;
      if(stop&&active&&(player.trackId===active.trackId||player.trackId===active.stopId))player.stop();
      this.active=null;this.finishControls();this.status.textContent=message;
    }
    revealOrder(){
      if(!this.session?.finished)return;
      this.result.textContent=this.session.order.map((order,i)=>`Trial ${i+1}: X = ${this.session.modes[order[0]]}, Y = ${this.session.modes[order[1]]}`).join(' · ');
      this.summarize();
    }
    summarize(){
      const s=this.session,tally={};
      for(const key of ['a','b'])tally[key]={mode:s.modes[key],wins:0,strength:[],clarity:[]};
      let ties=0;
      s.trials.forEach((t,i)=>{
        const order=s.order[i];
        order.forEach((key,slotIndex)=>{const slot=slotIndex?'y':'x';tally[key].strength.push(t.scores[slot+'-strength']);tally[key].clarity.push(t.scores[slot+'-clarity']);});
        if(t.preference==='tie')ties++;else tally[order[t.preference==='x'?0:1]].wins++;
      });
      const avg=a=>a.length?(a.reduce((x,y)=>x+y,0)/a.length).toFixed(1):'-';
      let box=document.getElementById('evaluation-summary');
      if(!box){box=document.createElement('p');box.id='evaluation-summary';box.className='pc-note evaluation-summary';this.result.after(box);}
      box.textContent=['a','b'].map(k=>`${k.toUpperCase()} (${tally[k].mode}): preferred ${tally[k].wins}x, strength ${avg(tally[k].strength)}, clarity ${avg(tally[k].clarity)}`).join('\n')+`\nNo preference: ${ties}. Subjective ratings from ${s.trials.length} trial${s.trials.length===1?'':'s'}; not a quality ranking.`;
    }
    export(){
      if(!this.session)return;const result=copy(this.session);delete result.context.source.path;
      result.sourceChecksum=null;result.note='Subjective ratings, not an objective quality ranking. Keep the exact source file with this report. Unscored trials are absent.';
      const url=URL.createObjectURL(new Blob([JSON.stringify(result,null,2)],{type:'application/json'})),link=document.createElement('a');link.href=url;link.download='controller-haptic-comparison.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),30000);
    }
  }
  ControllerHapticEvaluation.plan=plan;
  window.ControllerHapticEvaluation=ControllerHapticEvaluation;
})();
