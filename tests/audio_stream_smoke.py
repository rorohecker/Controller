"""Native file commands, playback state, mixer persistence and actual route meters."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={'width':1280,'height':900})
        mock_core(page)
        page.add_init_script("""
          localStorage.clear();
          const audioInvoke=window.__TAURI__.core.invoke;
          window.audioSelection={path:'C:/music/stereo.wav',name:'Stereo test.wav',duration:30,channels:2,sampleRate:44100};
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='platform_capabilities')return {os:'windows',desktopInput:true,nativeAudioFiles:true,xinput:true,screenColor:true,systemAudioBridge:true};
            if(name==='choose_audio_file')return window.audioSelection;
            return audioInvoke(name,args);
          };
          AudioContext.prototype.decodeAudioData=()=>{throw Error('Native playback must not decode in the webview');};
          window.audioStatus=(mode,position=0)=>{
            const command=window.sent.findLast(c=>c.action==='playAudioFile');
            window.coreHandler({type:'audioMeters',active:true,endpoint:'DualSense USB test endpoint',peaks:[.4,.7,0],reduction:.2,cueActive:true,bridge:false,fileUnderruns:1,
              file:{trackId:command.trackId,native:true,mode,position,duration:30-command.position,queueMs:800,speaker:true,grips:true}});
          };
        """)
        errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
        page.goto((ROOT/'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'test-pad'})")
        page.locator('[data-view="haptics"]').click()
        assert page.locator('#speaker-native-file').is_visible()
        assert not page.locator('#speaker-file').is_visible()
        page.locator('#speaker-native-file').click()
        page.wait_for_function("document.querySelector('#speaker-file-status').textContent==='Ready'")
        assert 'Stereo' in page.locator('#speaker-file-format').text_content()
        page.locator('#speaker-route').select_option('both')
        page.locator('#speaker-content-kind').select_option('haptic')
        page.locator('#speaker-file-play').click()
        page.wait_for_function("window.sent.some(c=>c.action==='playAudioFile')")
        assert page.evaluate("window.sent.findLast(c=>c.action==='playAudioFile').kind")=='haptic'
        assert page.evaluate("window.sent.every(c=>c.action!=='playSpeakerSound')")
        page.evaluate("window.audioStatus('playing',3)")
        assert page.locator('#speaker-file-play').text_content()=='Pause'
        assert page.locator('#audio-meter-left').evaluate('e=>e.value')==.7
        assert page.locator('#audio-meter-right').evaluate('e=>e.value')==0
        assert 'both grips' in page.locator('#audio-route-summary').text_content()
        assert page.locator('#audio-endpoint').text_content()=='DualSense USB test endpoint'
        page.locator('#speaker-file-play').click()
        page.evaluate("window.audioStatus('paused',4)")
        value=page.locator('#speaker-start').input_value()
        page.wait_for_timeout(300)
        assert page.locator('#speaker-start').input_value()==value
        page.locator('#speaker-start').fill('12')
        page.locator('#speaker-file-play').click()
        page.wait_for_function("window.sent.findLast(c=>c.action==='playAudioFile').position===12")
        page.evaluate("window.audioStatus('buffering',0)")
        assert 'Buffering' in page.locator('#speaker-file-status').text_content()
        page.evaluate("window.audioStatus('playing',1)")
        page.locator('#audio-mix-fileGain').fill('42')
        page.locator('#grip-audition').select_option('left')
        page.wait_for_function("window.sent.findLast(c=>c.action==='setAudioMixer').settings.solo==='left'")
        assert page.evaluate("window.sent.findLast(c=>c.action==='setAudioMixer').settings.fileGain")==.42
        assert page.evaluate("JSON.parse(localStorage.getItem('controller.audioMixer')).solo")=='left'
        page.locator('#speaker-file-stop').click()
        assert page.locator('#speaker-start').input_value()=='0'
        page.evaluate("window.audioStatus('playing',10)")
        assert page.locator('#speaker-file-play').text_content()=='Play'
        page.locator('#audio-mixer-reset').click()
        assert page.locator('#grip-audition').input_value()=='stereo'
        for width in (640,900,1280):
            page.set_viewport_size({'width':width,'height':900})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),width
        assert not errors,errors
        page.screenshot(path=str(ROOT/'target/audio-stream-ui.png'),full_page=True)
        browser.close()
    print('Audio stream smoke passed: native picker/commands, pause/seek, stale status, stereo meters, mixer/solo, responsive layout')


if __name__=='__main__':
    main()
