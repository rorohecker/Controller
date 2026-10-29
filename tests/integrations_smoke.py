"""Integration selection, failure handling and experimental boundaries; no hardware."""
import base64
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event


def main():
    with sync_playwright() as p:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={'width':1280,'height':900});mock_core(page)
        page.add_init_script("""const integrationBase=window.__TAURI__.core.invoke;
          window.integrationData={windowsBuild:26100,processCapture:true,processes:[{pid:12345,name:'testgame.exe'},{pid:23456,name:'music.exe'}],nativeRelayEndpoints:[{id:'virtual-4ch',name:'Virtual haptic source'}],environment:{session:'windows',message:'Windows input is privilege limited.'}};
          window.__TAURI__.core.invoke=async(n,a)=>{
            if(n==='integration_info')return structuredClone(window.integrationData);
            if(n==='inspect_native_report'){window.inspectedReport=a.reportBase64;return{writesHardware:false,usbBytes:63,bluetoothBytes:78,leftTriggerMode:33,rightTriggerMode:37};}
            return integrationBase(n,a);
          };""")
        errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto((ROOT/'ui/index.html').as_uri());page.wait_for_function('!!window.integrationStudio?.info&&!!window.coreHandler')
        assert page.evaluate("!window.sent.some(c=>c.action==='startLinkExperiment')")
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-A',dongle:{compatible:true,firmware:'v1.2.3'}})")
        pad_event(page)
        assert 'DS5Dongle-compatible' in page.locator('#dongle-status').text_content()
        page.locator('[data-view="haptics"]').click()
        page.locator('#capture-mode').select_option('process')
        page.locator('#capture-process').select_option('12345')
        page.locator('#capture-use').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setAudioBridge'&&c.source?.pid===12345)")
        page.evaluate("window.coreHandler({type:'audioStatus',bridge:true,route:'voiceCoil',captureSource:{kind:'process',pid:12345}})")
        page.locator('#bridge-gain').fill('64');page.locator('#bridge-gain').dispatch_event('change')
        assert page.evaluate("window.sent.filter(c=>c.action==='setAudioBridge').at(-1).source.pid") == 12345
        page.evaluate("window.coreHandler({type:'audioMeters',active:true,bridge:true,captureSource:{kind:'process',pid:12345},captureState:'starting',peaks:[0,0,0]})")
        assert 'Starting' in page.locator('#capture-source-status').inner_text()
        page.evaluate("window.coreHandler({type:'audioMeters',active:true,bridge:true,captureSource:{kind:'process',pid:12345},captureState:'running',peaks:[0,.1,.1]})")
        assert 'process tree 12345' in page.locator('#capture-source-status').inner_text()
        assert 'Selected app' in page.locator('#audio-source-summary').inner_text()
        page.evaluate("window.coreHandler({type:'audioStatus',bridge:true,captureSource:{kind:'process',pid:12345},captureError:true,message:'Source request rejected'})")
        assert 'Previous capture remains active' in page.locator('#capture-source-status').inner_text()
        # Process failure stops this capture and never requests a broader source.
        sent=page.evaluate('window.sent.length')
        page.evaluate("window.coreHandler({type:'audioStatus',bridge:false,route:'off',captureError:true,message:'Selected process ended'});window.coreHandler({type:'error',message:'Selected process ended'})")
        assert 'Capture is off' in page.locator('#capture-source-status').inner_text()
        assert not page.evaluate("i=>window.sent.slice(i).some(c=>c.action==='setAudioBridge'&&c.enabled)",sent)
        assert not page.evaluate('window.controllerStudio.state.audioBridge')
        page.locator('#capture-mode').select_option('nativeEndpoint')
        page.locator('#capture-endpoint').select_option('virtual-4ch')
        page.locator('#capture-use').click()
        assert page.evaluate("window.sent.filter(c=>c.action==='setAudioBridge').at(-1).source") == {'kind':'nativeEndpoint','id':'virtual-4ch'}
        # Profile audio resolves a live PID, while its archive stores the executable only.
        page.evaluate("""async()=>{
          const a=window.controllerStudio,p=a.profile();p.matchExe='testgame.exe';p.session={enabled:true,inputRoute:'keep',audio:'game',audioGain:50,hideHint:false};
          await window.gameSessions.start(p,true);
        }""")
        assert page.evaluate("window.sent.filter(c=>c.action==='setAudioBridge'&&c.enabled).at(-1).source") == {'kind':'process','pid':12345}
        assert page.evaluate("window.controllerStudio.archive().profiles[0].session.audio") == 'game'
        assert '12345' not in page.evaluate('JSON.stringify(window.controllerStudio.archive())')
        page.evaluate("async()=>{await window.gameSessions.restore();window.integrationData.processes.push({pid:34567,name:'testgame.exe'});await window.gameSessions.start(window.controllerStudio.profile(),true);}")
        assert page.evaluate('window.gameSessions.active===null')
        assert 'one matching' in page.locator('#game-session-status').text_content()
        # Handoff releases monitoring; it does not pretend to emulate a native driver.
        page.locator('[data-view="profiles"]').click()
        page.locator('#native-handoff').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setNativeHandoff'&&c.enabled)")
        page.evaluate("window.coreHandler({type:'device',connected:false});window.coreHandler({type:'safetyReset'});window.coreHandler({type:'nativeHandoff',enabled:true})")
        assert page.locator('#native-handoff').text_content() == 'Resume monitoring'
        assert 'outside this app' in page.locator('#feedback-sources').text_content()
        page.locator('#native-handoff').click()
        assert page.evaluate("window.sent.at(-1)") == {'action':'setNativeHandoff','enabled':False}
        page.evaluate("window.coreHandler({type:'nativeHandoff',enabled:false});window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'Bluetooth',controllerId:'pad-A'})")
        # Backend checks request no pointer/key movement and can report revocation.
        page.locator('[data-view="pc-controls"]').click()
        page.locator('#runtime-access-test').click()
        assert page.evaluate("window.sent.at(-1).action") == 'checkDesktopInput'
        page.evaluate("window.coreHandler({type:'desktopInputStatus',ready:true,message:'No input sent'})")
        assert 'connection opened' in page.locator('#runtime-access-status').inner_text()
        page.evaluate("window.coreHandler({type:'desktopInputStatus',ready:false,message:'Access revoked'});window.coreHandler({type:'pcMode',enabled:false})")
        assert 'Access revoked' in page.locator('#runtime-access-status').inner_text()
        # Experiments require an explicit button and validate a small strength ceiling.
        page.locator('[data-view="diagnostics"]').click()
        page.locator('#link-test-strength').fill('26')
        sent=page.evaluate('window.sent.length');page.locator('#link-test-start').click()
        assert page.evaluate('window.sent.length') == sent
        page.locator('#link-test-strength').fill('10');page.locator('#link-test-start').click()
        assert page.evaluate('window.sent.at(-1)') == {'action':'startLinkExperiment','strength':10}
        page.evaluate("window.coreHandler({type:'safetyReset'});window.coreHandler({type:'linkExperiment',running:true,transport:'Bluetooth',unit:'packets',sent:5,skipped:1,maxScheduleLateUs:400,maxWriteUs:100})")
        assert page.locator('#link-test-start').is_disabled()
        page.evaluate("window.coreHandler({type:'linkExperiment',running:false,completed:true,transport:'Bluetooth',unit:'packets',sent:185,skipped:2,maxScheduleLateUs:400,maxWriteUs:100,physicalLatencyMeasured:false})")
        assert 'Physical latency not measured' in page.locator('#link-test-status').inner_text()
        assert '185 packets' in page.locator('#link-comparison').inner_text()
        assert page.locator('#link-test-start').is_enabled()
        # Packet inspection is read-only and never forwards a report.
        page.locator('.native-packet-preview summary').click()
        report=bytearray(63);report[0]=2
        page.locator('#native-packet').fill(base64.b64encode(report).decode())
        sent=page.evaluate('window.sent.length');page.locator('#native-packet-inspect').click()
        page.wait_for_function("document.querySelector('#native-packet-status').textContent.includes('No hardware write')")
        assert page.evaluate('window.sent.length') == sent
        for width in (390,640,900,1280,1600):
            page.set_viewport_size({'width':width,'height':900})
            for view in ('haptics','pc-controls','profiles','diagnostics'):
                page.evaluate('view=>window.controllerStudio.navigate(view)',view)
                assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),(width,view)
        page.set_viewport_size({'width':1280,'height':1000})
        page.locator('[data-view="haptics"]').click();page.locator('#capture-source-panel').scroll_into_view_if_needed()
        page.screenshot(path=str(ROOT/'target/integrations-check.png'))
        assert not errors,errors
        browser.close()
        print('Integrations smoke passed: scoped capture, no implicit fallback, per-game PID resolution, native handoff, permissions, opt-in experiments and read-only reports')


if __name__=='__main__':
    main()
