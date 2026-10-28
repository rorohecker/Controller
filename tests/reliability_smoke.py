"""Transport races, system shortcut editing, permission UI and per-pad ranges."""
import io
import struct
import wave
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def connect(page, identity):
    page.evaluate("id=>window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:id,path:id})", identity)


def main():
    with sync_playwright() as p:
        chrome = Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        mock_core(page)
        page.add_init_script("""
          const reliabilityInvoke=window.__TAURI__.core.invoke;
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='record_shortcut') return [18,9];
            if(name==='cancel_shortcut_recording')return null;
            if(name==='send_command'&&args.command.append&&window.holdAppend){
              window.appendWaiting=true;await new Promise(resolve=>window.releaseAppend=resolve);
            }
            const result=await reliabilityInvoke(name,args);
            if(name==='send_command'&&args.command.action==='playSpeakerSound'&&!args.command.append){
              setTimeout(()=>window.coreHandler({type:'speakerStatus',source:'file',trackId:args.command.trackId}),0);
            }
            return result;
          };
          localStorage.clear();
          localStorage.setItem('aftertouch.inputCalibration',JSON.stringify({axisCenters:[.1,0,0,0],axisMins:[-1,-1,-1,-1],axisMaxs:[1,1,1,1],triggerRests:[0,0],triggerFulls:[1,1],touchMins:[0,0],touchMaxs:[1,1]}));
        """)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler')
        connect(page, 'pad-one')
        page.wait_for_function("window.sent.some(c=>c.action==='setCalibration'&&c.calibration.axisCenters[0]===.1)")
        connect(page, 'pad-two')
        page.wait_for_function("window.sent.findLast(c=>c.action==='setCalibration').calibration.axisCenters[0]===0")
        connect(page, 'pad-one')
        page.wait_for_function("window.sent.findLast(c=>c.action==='setCalibration').calibration.axisCenters[0]===.1")
        page.locator('[data-view="pc-controls"]').click()
        page.locator('[aria-label="CROSS common shortcut"]').select_option('switch')
        assert page.evaluate("JSON.parse(localStorage.getItem('aftertouch.pcBindings')).find(b=>b.button===0).chord") == [18, 9]
        page.locator('[aria-label="CROSS Shift modifier"]').click()
        assert set(page.evaluate("JSON.parse(localStorage.getItem('aftertouch.pcBindings')).find(b=>b.button===0).chord")) == {18, 16, 9}
        page.locator('[aria-label="CROSS key"]').select_option('65')
        assert set(page.evaluate("JSON.parse(localStorage.getItem('aftertouch.pcBindings')).find(b=>b.button===0).chord")) == {18, 16, 65}
        page.locator('[aria-label="CROSS record shortcut"]').click()
        page.wait_for_function("JSON.stringify(JSON.parse(localStorage.getItem('aftertouch.pcBindings')).find(b=>b.button===0).chord)==='[18,9]'")
        assert not page.locator('.desktop-access').is_visible()
        page.locator('[data-view="haptics"]').click()
        with io.BytesIO() as sound:
            with wave.open(sound, 'wb') as wav:
                wav.setnchannels(1); wav.setsampwidth(2); wav.setframerate(48000)
                wav.writeframes(struct.pack('<h', 2000) * 48000 * 15)
            page.locator('#speaker-file').set_input_files({'name':'long-test.wav','mimeType':'audio/wav','buffer':sound.getvalue()})
        page.wait_for_function("document.querySelector('#speaker-file-status').textContent==='Ready'")
        page.evaluate('window.holdAppend=true')
        page.locator('#speaker-file-play').click()
        page.wait_for_function('window.appendWaiting')
        page.locator('#speaker-file-stop').click()
        assert page.locator('#speaker-file-play').text_content() == 'Play'
        assert page.locator('#speaker-start').input_value() == '0'
        page.evaluate('window.holdAppend=false;window.releaseAppend()')
        page.wait_for_function("window.sent.at(-1).action==='stopSpeakerSound'")
        old = page.evaluate("window.sent.findLast(c=>c.action==='playSpeakerSound').trackId")
        page.evaluate("id=>window.coreHandler({type:'speakerStatus',source:'transport',paused:false,position:5,trackId:id})", old)
        assert page.locator('#speaker-file-play').text_content() == 'Play'
        page.locator('#speaker-file-play').click()
        page.wait_for_function("document.querySelector('#speaker-start').value>0")
        page.locator('#speaker-file-play').click()
        page.wait_for_function("window.sent.at(-1).action==='setSpeakerPaused'")
        value = page.locator('#speaker-start').input_value()
        page.wait_for_timeout(300)
        assert page.locator('#speaker-start').input_value() == value
        page.locator('#speaker-start').fill('7')
        assert page.locator('#speaker-file-play').text_content() == 'Resume'
        page.locator('#speaker-file-play').click()
        page.wait_for_function("document.querySelector('#speaker-start').value>7")
        connect(page, 'pad-two')
        page.evaluate("window.coreHandler({type:'device',connected:false})")
        assert page.locator('#speaker-file-play').is_disabled()
        assert not errors, errors
        browser.close()
    print('Reliability smoke passed: stop/append race, stale acknowledgements, pause/seek/resume, shortcuts, isolated calibration')


if __name__ == '__main__':
    main()
