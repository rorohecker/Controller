"""Timeline/library, personal calibration, native encoding and matched conversion windows."""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome=Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={"width":1280,"height":900})
        mock_core(page)
        page.add_init_script("""
          localStorage.clear();const base=window.__TAURI__.core.invoke;window.previewCalls=[];
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='platform_capabilities')return {os:'windows',desktopInput:true,nativeAudioFiles:true,xinput:true,systemAudioBridge:true};
            if(name==='choose_audio_file')return {path:'C:/fixture.wav',name:'Test track.wav',duration:30,channels:2,sampleRate:48000};
            if(name==='preview_authoring'){
              window.previewCalls.push(args);
              if(args.trigger)return {packet:[33,255,3,0,0,0,0,0,0,0,0],levels:[0,1,2,3,4,5,6,7,8,0],triggerLimit:args.personal.enabled?args.personal.triggerLimit:8,note:'Encoded steps, not measured force.'};
              return {durationMs:Math.max(...args.clip.cues.map(c=>Math.ceil((c.startMs+c.durationMs)/10)*10)),quantumMs:10,triggerLimit:8,cues:args.clip.cues.map(c=>({...c,requestedStartMs:c.startMs,requestedDurationMs:c.durationMs,startMs:Math.ceil(c.startMs/10)*10,durationMs:Math.ceil((c.startMs+c.durationMs)/10)*10-Math.ceil(c.startMs/10)*10,levels:c.lane.endsWith('Trigger')?(c.kind==='release'?Array(10).fill(0):[0,0,6,6,6,6,6,6,0,0]):null}))};
            }
            return base(name,args);
          };
        """)
        errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
        page.goto((ROOT/'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'author-pad'})")
        page.locator('[data-view="haptics"]').click()
        page.locator('#tactile-name').fill('Dash hit')
        page.get_by_label('Lane',exact=True).select_option('rightTrigger')
        page.get_by_label('Phase',exact=True).select_option('load')
        for label,value in [('Start \u00b7 ms','3'),('Duration \u00b7 ms','97'),('Strength \u00b7 %','80')]:
            field=page.get_by_label(label,exact=True);field.fill(value);field.press('Tab')
        page.wait_for_function("document.querySelector('#tactile-encoded').textContent.includes('10+90 ms')")
        page.locator('#tactile-add').click()
        page.get_by_label('Phase',exact=True).select_option('release')
        page.get_by_label('Strength \u00b7 %',exact=True).fill('0')
        page.get_by_label('Strength \u00b7 %',exact=True).press('Tab')
        page.locator('#tactile-save').click()
        page.wait_for_function("window.saved?.tactileClips.some(c=>c.name==='Dash hit')")
        assert page.evaluate("window.saved.tactileClips.length")==5
        page.locator('#tactile-play').click()
        page.wait_for_function("window.sent.some(c=>c.action==='playEffectClip')")
        play_id=page.evaluate("window.sent.findLast(c=>c.action==='playEffectClip').playId")
        page.evaluate("id=>window.coreHandler({type:'effectTimeline',playId:id,playing:true,positionMs:75,durationMs:200,name:'Dash hit'})",play_id)
        assert page.locator('#tactile-progress').evaluate('e=>e.value')==75
        page.locator('#tactile-stop').click()
        page.evaluate("id=>window.coreHandler({type:'effectTimeline',playId:id,playing:true,positionMs:120,durationMs:200})",play_id)
        assert page.locator('#tactile-progress').evaluate('e=>e.value')==0
        page.get_by_label('Left noticeable \u00b7 %',exact=True).fill('12');page.get_by_label('Left noticeable \u00b7 %',exact=True).press('Tab')
        page.get_by_label('Left comfortable maximum \u00b7 %',exact=True).fill('60');page.get_by_label('Left comfortable maximum \u00b7 %',exact=True).press('Tab')
        page.locator('#personal-enabled').check();page.locator('#personal-save').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setPersonalHaptics'&&c.settings.minimum[0]===12&&c.settings.maximum[0]===60)")
        request=page.evaluate("window.sent.findLast(c=>c.action==='setPersonalHaptics').requestId")
        page.evaluate("requestId=>window.coreHandler({type:'personalHapticsStatus',requestId,applied:false,message:'Output unavailable'})",request)
        assert 'Output unavailable' in page.locator('#personal-status').inner_text()
        page.evaluate("requestId=>window.coreHandler({type:'personalHapticsStatus',requestId,applied:true})",request)
        assert page.locator('#personal-status').inner_text()=='Personal limits applied.'
        page.locator('#personal-test').click()
        page.wait_for_function("window.sent.some(c=>c.action==='playPersonalTest'&&c.side===0&&c.strength===10)")
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'other-pad'})")
        assert page.get_by_label('Left noticeable \u00b7 %',exact=True).input_value()=='0'
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'author-pad'})")
        assert page.get_by_label('Left noticeable \u00b7 %',exact=True).input_value()=='12'
        card=page.locator('.tactile-library-card').filter(has=page.get_by_role('heading',name='Dash hit',exact=True))
        card.get_by_role('button',name='Use in new action',exact=True).click()
        page.locator('#game-actions-enabled').check();page.locator('#save-game-actions').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setGameActions'&&c.enabled&&c.rules[0]?.timeline?.name==='Dash hit')")
        assert page.evaluate("window.saved.profiles[0].actions[0].haptic===null")
        page.locator('#export-profiles').click()
        page.wait_for_function('!!window.exported')
        archive=page.evaluate('window.exported')
        assert len(archive['tactileClips'])==5
        assert archive['profiles'][0]['actions'][0]['timeline']['name']=='Dash hit'
        page.locator('[data-view="triggers"]').click()
        page.wait_for_function("document.querySelector('#trigger-encoded').textContent.includes('Encoded zone steps')")
        page.locator('[data-view="haptics"]').click()
        page.locator('#speaker-native-file').click()
        page.wait_for_function("document.querySelector('#speaker-file-status').textContent==='Ready'")
        page.evaluate('window.controllerStudio.getPlayer().position=6.5')
        page.locator('#conversion-capture').click();page.locator('#conversion-play-a').click()
        page.wait_for_function("window.sent.some(c=>c.action==='playAudioFile'&&c.kind==='bass')")
        a=page.evaluate("window.sent.findLast(c=>c.action==='playAudioFile')")
        page.locator('#conversion-play-b').click()
        page.wait_for_function("window.sent.findLast(c=>c.action==='playAudioFile')?.kind==='texture'")
        b=page.evaluate("window.sent.findLast(c=>c.action==='playAudioFile')")
        assert (a['position'],a['duration'],a['route'])==(6.5,11.5,'grips')
        assert (b['position'],b['duration'],b['route'])==(6.5,11.5,'grips')
        page.evaluate("id=>window.coreHandler({type:'audioMeters',file:{trackId:id,mode:'finished',position:5,duration:5}})",b['trackId'])
        assert page.evaluate('window.controllerStudio.getPlayer().duration')==30
        assert page.evaluate('window.controllerStudio.getPlayer().position')==6.5
        # Two comparisons requested together should start only the newest one.
        before=page.evaluate("window.sent.filter(c=>c.action==='playAudioFile').length")
        page.evaluate("async()=>{const player=window.controllerStudio.getPlayer();await Promise.all([player.audition('bass',2,5),player.audition('texture',2,5)]);}")
        commands=page.evaluate("index=>window.sent.filter(c=>c.action==='playAudioFile').slice(index)",before)
        assert len(commands)==1 and commands[0]['kind']=='texture',commands
        restored=browser.new_page(viewport={"width":900,"height":800});mock_core(restored)
        restored.add_init_script("const load=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(n,a)=>n==='load_profiles'?"+json.dumps(archive)+":load(n,a)")
        restored.goto((ROOT/'ui/index.html').as_uri());restored.wait_for_function('!!window.coreHandler')
        assert restored.evaluate("window.hapticAuthoring.archive().length")==5
        assert restored.evaluate("window.controllerStudio.profile().actions[0].timeline.name")=='Dash hit'
        for width in (390,640,900,1600):
            page.set_viewport_size({"width":width,"height":900})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),width
        page.set_viewport_size({"width":1280,"height":900});page.locator('#tactile-workshop').scroll_into_view_if_needed();page.screenshot(path=str(ROOT/'target/haptic-authoring-check.png'))
        assert not errors,errors
        browser.close()
        print('Haptic authoring smoke passed: timelines, encoding, clip export/reload, action snapshots, per-pad limits, stereo tests and matched A/B')


if __name__=='__main__':
    main()
