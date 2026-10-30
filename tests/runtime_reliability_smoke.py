"""Visible/hidden rendering, passive timing/trace UI and blinded audio trials. No hardware."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event


def draw_sample(page, count=1, x=.5):
    page.evaluate("""({count,x})=>{for(let i=0;i<count;i++)window.coreHandler({type:'input',input:{
      buttons:Array(23).fill(0),axes:[x,0,0,0],touch:[{x:.8,y:.2},null],gyro:[1,2,3],
      battery:75,charging:false,batteryInfo:{chargeState:'discharging',sensorByte:7},fullReport:true}});} """, dict(count=count,x=x))


def meters(page, mode=None):
    page.evaluate("""mode=>{const a=window.controllerStudio,p=a.getPlayer();window.coreHandler({type:'audioMeters',controllerId:a.state.controllerId,
      firmware:'test',personal:JSON.parse(JSON.stringify(window.hapticAuthoring.personal)),mixer:a.getMixer(),
      active:true,outputStage:'ready',callbacks:100,peaks:[0,.1,.1],
      file:mode?{trackId:p.trackId,mode,position:mode==='finished'?7:2.5,duration:5,speaker:false,grips:true,native:true}:null});} """, mode)


def main():
    with sync_playwright() as playwright:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=playwright.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={'width':1280,'height':900});mock_core(page)
        page.add_init_script("""const runtimeBase=window.__TAURI__.core.invoke;
          window.traceRequests=[];window.mockHidden=false;
          Object.defineProperty(document,'hidden',{configurable:true,get:()=>window.mockHidden});
          window.__TAURI__.core.invoke=async(name,args)=>{
            const result=await runtimeBase(name,args);
            if(name==='send_command'){
              const c=args.command;
              if(c.action==='latencyProbe')setTimeout(()=>window.coreHandler({type:'latencyEcho',id:c.id}),2);
              if(c.action==='setHidRecording')window.coreHandler({type:'hidRecording',active:c.enabled,events:c.enabled?0:20});
              if(c.action==='getHidTrace')window.coreHandler({type:'hidTrace',trace:{schemaVersion:1,events:[{kind:'connection',atUs:0,transport:'USB'}]}});
              if(c.action==='inspectHidTrace')window.traceRequests.push(c.requestId);
            }
            return result;
          };
        """)
        errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
        page.goto((ROOT/'ui/index.html').as_uri());page.wait_for_function('!!window.coreHandler&&!!window.hapticEvaluation')
        page.evaluate("""()=>{window.controllerStudio.state.autoSuppressed=true;
          window.coreHandler({type:'device',connected:true,name:'DualSense',controllerId:'pad-test',path:'test',transport:'USB',firmware:'test'});} """)
        draw_sample(page)
        page.locator('[data-view="settings"]').click();page.wait_for_timeout(100)
        page.evaluate("""()=>{window.hiddenMutations=0;window.hiddenObserver=new MutationObserver(rows=>window.hiddenMutations+=rows.length);
          for(const id of ['live','touch','calibration'])window.hiddenObserver.observe(document.getElementById(id),{subtree:true,childList:true,attributes:true});} """)
        before=page.evaluate('window.controllerStudio.visualStats()')
        draw_sample(page,100,.75);page.wait_for_timeout(100)
        after=page.evaluate('window.controllerStudio.visualStats()')
        assert after['inputEvents']-before['inputEvents']==100
        assert after['inputDraws']==before['inputDraws']
        assert page.evaluate('window.hiddenMutations')==0
        page.evaluate('window.hiddenObserver.disconnect()')
        page.locator('[data-view="live"]').click()
        page.wait_for_function("document.querySelector('#left-stick-cap').getAttribute('transform')==='translate(28.5 0)'")
        draw_sample(page,200,.75)
        before=page.evaluate('window.controllerStudio.visualStats().widgetDraws')
        draw_sample(page,30,.75)
        assert page.evaluate('window.controllerStudio.visualStats().widgetDraws')==before

        # Hidden input keeps data current and resumes painting from the latest sample.
        page.evaluate("window.mockHidden=true;document.dispatchEvent(new Event('visibilitychange'))")
        before=page.evaluate('window.controllerStudio.visualStats()');draw_sample(page,80,-.5)
        after=page.evaluate('window.controllerStudio.visualStats()')
        assert after['hiddenEvents']-before['hiddenEvents']==80
        assert after['widgetDraws']==before['widgetDraws']
        page.evaluate("window.mockHidden=false;document.dispatchEvent(new Event('visibilitychange'))")
        page.wait_for_function("document.querySelector('#left-stick-cap').getAttribute('transform')==='translate(-19 0)'")
        page.locator('[data-view="diagnostics"]').click()
        page.wait_for_function("document.querySelector('#diag-temp').textContent==='7'")
        page.locator('[data-view="touch"]').click()
        page.wait_for_function("document.querySelector('#touch-1-pos').textContent==='0.800 / 0.200'")
        # Untimed calibration continues collecting while hidden, with no hidden DOM work.
        page.locator('[data-view="calibration"]').click();page.locator('#start-calibration').click()
        page.evaluate("window.mockHidden=true;document.dispatchEvent(new Event('visibilitychange'))")
        for _ in range(35):pad_event(page)
        assert page.locator('#next-calibration').is_disabled()
        page.evaluate("window.mockHidden=false;document.dispatchEvent(new Event('visibilitychange'))")
        page.wait_for_function("!document.querySelector('#next-calibration').disabled")
        page.locator('#cancel-calibration').click()

        # Legacy PCM chunks still pump while hidden, then Stop retires the timer.
        page.evaluate("""()=>{window.mockHidden=true;document.dispatchEvent(new Event('visibilitychange'));
          const p=window.controllerStudio.getPlayer();p.nativeFile=null;p.decoded={duration:20};p.duration=20;p.position=0;
          p.mode='playing';p.started=performance.now();p.next=0;p.hasStream=true;p.encode=()=>({pcmBase64:'AAAAAA==',duration:1});p.render();} """)
        page.wait_for_function("window.sent.some(c=>c.action==='playSpeakerSound'&&c.append)")
        page.evaluate('window.controllerStudio.getPlayer().stop()');page.wait_for_timeout(150)
        assert not page.evaluate('!!window.controllerStudio.getPlayer().tickTimer')
        page.evaluate("window.mockHidden=false;document.dispatchEvent(new Event('visibilitychange'))")
        page.locator('[data-view="diagnostics"]').click();page.locator('#runtime-ping').click()
        page.wait_for_function("document.querySelector('#runtime-status').textContent.includes('20/20 replies')")
        page.locator('#runtime-copy').click();page.wait_for_function("window.copiedDiagnostics?.includes('softwareOnly')")
        report=json.loads(page.evaluate('window.copiedDiagnostics'));assert report['roundTrip']['count']==20
        assert report['softwareOnly'] is True
        page.locator('#hid-record').click();page.wait_for_function("!document.querySelector('#hid-stop').disabled")
        page.locator('#hid-stop').click();page.wait_for_function("!document.querySelector('#hid-copy').disabled")
        page.locator('#hid-copy').click();page.wait_for_function("window.copiedDiagnostics?.includes('schemaVersion')")
        trace=json.loads(page.evaluate('window.copiedDiagnostics'));assert trace['schemaVersion']==1
        file={'name':'trace.json','mimeType':'application/json','buffer':json.dumps(trace).encode()}
        page.locator('#hid-replay-file').set_input_files(file);page.wait_for_function('window.traceRequests.length===1')
        page.locator('#hid-replay-file').set_input_files(file);page.wait_for_function('window.traceRequests.length===2')
        page.evaluate("""()=>{const report={validReports:2,invalidReports:1,connections:1,disconnects:0,readErrors:0,sequenceGaps:0,duplicateSequences:0,staleIntervals:0,intervalUs:[4000],droppedEvents:0};
          window.coreHandler({type:'hidReplay',requestId:2,report});window.coreHandler({type:'hidReplay',requestId:1,error:'stale result'});} """)
        assert '2 valid / 1 invalid' in page.locator('#hid-replay-result').inner_text()
        count=page.evaluate('window.traceRequests.length')
        page.locator('#hid-replay-file').set_input_files({'name':'large.json','mimeType':'application/json','buffer':b' '*(1024*1024+1)})
        assert page.evaluate('window.traceRequests.length')==count
        assert 'exceeds 1 MiB' in page.locator('#hid-replay-result').inner_text()

        # Balanced seed, explicit playback, actual completion before rating.
        page.locator('[data-view="haptics"]').click()
        page.evaluate("""()=>{const p=window.controllerStudio.getPlayer();p.nativeFile={path:'C:/private/test.wav',name:'test.wav',duration:20,sampleRate:48000,channels:2};
          p.decoded={native:true};p.duration=20;p.position=2;p.mode='paused';p.render();} """)
        meters(page)
        assert page.evaluate("JSON.stringify(ControllerHapticEvaluation.plan(137))===JSON.stringify(ControllerHapticEvaluation.plan(137))")
        assert page.evaluate("ControllerHapticEvaluation.plan(137).filter(r=>r[0]==='a').length") == 2
        count=page.evaluate("window.sent.filter(c=>c.action==='playAudioFile').length")
        page.locator('#evaluation-start').click()
        assert page.evaluate("window.sent.filter(c=>c.action==='playAudioFile').length")==count
        for trial in range(4):
            for slot in ['x','y']:
                page.locator('#evaluation-'+slot).click()
                page.wait_for_function('!!window.hapticEvaluation.active?.trackId')
                command=page.evaluate("window.sent.filter(c=>c.action==='playAudioFile').at(-1)")
                assert command['position']==2 and command['duration']==7 and command['route']=='grips'
                assert page.locator('#evaluation-next').is_disabled()
                meters(page,'playing');meters(page,'finished')
                page.wait_for_function('!window.hapticEvaluation.active')
            for slot in ['x','y']:
                for metric in ['strength','clarity']:page.locator(f'#evaluation-{slot}-{metric}').select_option('3')
            page.locator('#evaluation-preference').select_option('x');page.locator('#evaluation-next').click()
        assert len(page.evaluate('window.hapticEvaluation.session.trials'))==4
        page.locator('#evaluation-reveal').click();assert 'Trial 4' in page.locator('#evaluation-results').inner_text()
        with page.expect_download() as download:page.locator('#evaluation-export').click()
        exported=json.loads(Path(download.value.path()).read_text(encoding='utf-8'))
        assert 'path' not in exported['context']['source'] and exported['sourceChecksum'] is None
        assert exported['planVersion']==1 and len(exported['trials'])==4
        assert len(exported['playbacks'])==8 and all(row['completed'] for row in exported['playbacks'])
        page.locator('#evaluation-start').click();page.locator('#evaluation-x').click()
        page.wait_for_function('!!window.hapticEvaluation.active?.trackId')
        page.evaluate("window.controllerStudio.getPlayer().stop()")
        assert page.evaluate('window.hapticEvaluation.session.cancelled')

        # Measure the headless preview's main-thread work; no timing threshold is
        # asserted across machines. Counts enforce that idle/hidden draws stop.
        page.locator('[data-view="settings"]').click();page.wait_for_timeout(300)
        cdp=page.context.new_cdp_session(page);cdp.send('Performance.enable')
        # Compare the old CSS reflection with the new overlay on this same DOM.
        # Negative delay puts both sweeps in their moving phase for each sample.
        page.emulate_media(reduced_motion='no-preference')
        page.add_style_tag(content="""
          .card-reflection::before{animation-delay:-4s}
          .benchmark-legacy-effects .card-reflection{display:none}
          .benchmark-legacy-effects .framed::before{background:linear-gradient(110deg,transparent 33%,#ffffff29 44%,#c9f9f366 50%,#ffddda38 54%,transparent 65%) -100% 0/250% 100% no-repeat;animation:panel-reflector 13s ease-in-out -4s infinite}
          .benchmark-legacy-effects *,.benchmark-legacy-effects *::before,.benchmark-legacy-effects *::after{animation-play-state:running!important}
        """)
        samples=[]
        for legacy in [True,False]:
            page.evaluate("legacy=>document.body.classList.toggle('benchmark-legacy-effects',legacy)",legacy)
            for hidden in [False,True]:
                page.evaluate("hidden=>{window.mockHidden=hidden;document.dispatchEvent(new Event('visibilitychange'));}",hidden)
                cdp.send('HeapProfiler.collectGarbage');page.wait_for_timeout(200)
                start={m['name']:m['value'] for m in cdp.send('Performance.getMetrics')['metrics']}
                counts=page.evaluate('window.controllerStudio.visualStats()');page.wait_for_timeout(1500)
                end={m['name']:m['value'] for m in cdp.send('Performance.getMetrics')['metrics']}
                final=page.evaluate('window.controllerStudio.visualStats()')
                assert final['frames']==counts['frames']
                elapsed=end['Timestamp']-start['Timestamp']
                samples.append({'reflection':'legacy-background-position' if legacy else 'composited-overlay','simulatedHidden':hidden,'seconds':elapsed,'mainThreadTaskPercent':(end['TaskDuration']-start['TaskDuration'])/elapsed*100,'jsHeapBytes':end['JSHeapUsedSize'],'nodes':end['Nodes'],'extraFrames':final['frames']-counts['frames']})
        output=ROOT/'target'/'batch9'/'browser-resources.json';output.parent.mkdir(parents=True,exist_ok=True)
        output.write_text(json.dumps({'scope':'Headless Chromium preview; hidden flag simulated; not Tauri CPU or GPU','browser':browser.version,'appVersion':page.locator('.side-code').inner_text(),'viewport':{'width':1280,'height':900},'samples':samples},indent=2),encoding='utf-8')
        assert not errors,errors
        browser.close()
    print('Runtime reliability smoke passed: visible/hidden widgets, background PCM, passive traces/RTT, seeded haptic trials and idle resource samples')


if __name__=='__main__':main()
