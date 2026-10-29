"""Motion settings, controller ownership and radial editing without hardware."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event


def main():
    with sync_playwright() as p:
        chrome = Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        page.add_init_script("""const motionInvoke=window.__TAURI__.core.invoke;
          window.recordKeys=[18,9];
          window.__TAURI__.core.invoke=async(n,a)=>n==='record_shortcut'?(window.delayRecord?new Promise(r=>window.finishRecording=r):window.recordKeys):motionInvoke(n,a);""")
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler && !!window.motionStudio')
        page.evaluate("window.coreHandler({type:'device',connected:true,controllerId:'pad-A',name:'DualSense',transport:'USB',motionFactory:{valid:true,message:'Factory calibration loaded'}})")
        pad_event(page)
        page.locator('[data-view="touch"]').click()
        assert page.locator('#stick-workbench').is_visible()
        page.get_by_label('Mouse counts per 360°', exact=True).fill('3600')
        page.get_by_label('Mouse counts per 360°', exact=True).press('Tab')
        page.get_by_label('Gyro aim space', exact=True).select_option('world')
        page.get_by_label('Enable flick stick', exact=True).check()
        page.get_by_label('Left deadzone shape', exact=True).select_option('radial')
        page.get_by_label('Left inner deadzone · %', exact=True).fill('12')
        page.get_by_label('Left inner deadzone · %', exact=True).press('Tab')
        page.get_by_label('Enable radial menu', exact=True).check()
        page.get_by_label('Radial mode', exact=True).select_option('both')
        page.get_by_label('Sector 1 label', exact=True).fill('Switch app')
        page.locator('#radial-record-0').click()
        page.wait_for_function("window.motionStudio.controls().radial.sectors[0].chord.join(',')==='18,9'")
        page.locator('#motion-save').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setControlProfile'&&c.settings.motion.countsPerTurn===3600)")
        settings = page.evaluate("window.sent.filter(c=>c.action==='setControlProfile').at(-1).settings")
        assert settings['motion']['space'] == 'world'
        assert settings['motion']['flickEnabled']
        assert settings['sticks']['left']['inner'] == .12
        assert settings['radial']['sectors'][0]['chord'] == [18, 9]
        page.evaluate("settings=>window.coreHandler({type:'controlProfile',applied:true,settings})", json.loads(json.dumps(settings, sort_keys=True)))
        assert page.locator('#motion-settings-status').inner_text() == 'Input settings applied.'
        page.get_by_label('Aim sensitivity multiplier', exact=True).fill('0.5')
        page.get_by_label('Aim sensitivity multiplier', exact=True).press('Tab')
        assert 'not applied' in page.locator('#motion-settings-status').inner_text()
        page.get_by_label('Aim sensitivity multiplier', exact=True).fill('1')
        page.get_by_label('Aim sensitivity multiplier', exact=True).press('Tab')
        page.evaluate('window.controllerStudio.profile().gyroAim.sensitivity=1')
        page.locator('#motion-camera-angle').fill('180')
        page.locator('#motion-recalculate').click()
        assert page.get_by_label('Mouse counts per 360°', exact=True).input_value() == '7200'
        # Stale completion cannot overwrite another pad's still calibration.
        page.locator('#motion-calibrate').click()
        assert page.evaluate("window.sent.some(c=>c.action==='calibrateMotion'&&c.enabled)")
        reading = dict(valid=True, factory=True, factoryMessage='Factory calibrated', gyroDps=[1., 2., 3.],
                       accelG=[0., 0., 1.], gravity=[0., 0., 1.], pitchDeg=12., rollDeg=-5., yawDeg=10.,
                       dt=.004, clock='device', still=True, calibration='measuring', progress=.4,
                       biasDps=[0., 0., 0.], calibrationComplete=False)
        page.evaluate("reading=>window.coreHandler({type:'motion',reading})", reading)
        assert '4.00 ms' in page.locator('#motion-clock').inner_text()
        assert '12.0°' in page.locator('#motion-pose').inner_text()
        assert '40%' in page.locator('#motion-rest-status').inner_text()
        page.evaluate("window.coreHandler({type:'motionCalibration',status:'complete',controllerId:'pad-B',biasDps:[1,1,1]})")
        assert page.evaluate("localStorage.getItem('controller.motionBias.v1')") is None
        page.evaluate("window.coreHandler({type:'motionCalibration',status:'complete',controllerId:'pad-A',biasDps:[.1,.2,.3]})")
        assert page.evaluate("JSON.parse(localStorage.getItem('controller.motionBias.v1'))['pad-A']") == [.1, .2, .3]
        page.evaluate("window.coreHandler({type:'device',connected:false});window.coreHandler({type:'device',connected:true,controllerId:'pad-B',name:'DualSense',transport:'USB'})")
        before = page.evaluate("window.sent.filter(c=>c.action==='setMotionBias').length")
        assert 'No saved' in page.locator('#motion-rest-status').inner_text()
        page.evaluate("window.coreHandler({type:'device',connected:false});window.coreHandler({type:'device',connected:true,controllerId:'pad-A',name:'DualSense',transport:'USB'})")
        assert page.evaluate("window.sent.filter(c=>c.action==='setMotionBias').length") == before + 1
        page.evaluate("window.coreHandler({type:'radialMenu',open:true,consume:true,selected:0,preview:true})")
        assert page.evaluate('window.motionStudio.radialCapture')
        assert 'Switch app' in page.locator('#radial-status').inner_text()
        page.evaluate("window.coreHandler({type:'radialMenu',open:false,consume:true,selected:null,committed:0,preview:true})")
        assert 'Preview only' in page.locator('#radial-status').inner_text()
        assert page.evaluate('window.motionStudio.radialCapture')
        page.evaluate("window.coreHandler({type:'radialMenu',open:false,consume:false,selected:null})")
        assert not page.evaluate('window.motionStudio.radialCapture')
        page.locator('#radial-cancel').click()
        assert page.evaluate("window.sent.at(-1).action") == 'cancelRadial'
        pad_event(page)
        page.evaluate("window.motionStudio.event({type:'input',rawAxes:[.1,.2,.3,.4],virtualAxes:[0,.25,0,0],input:{axes:[.1,.2,.3,.4]}})")
        assert 'virtual 0.000 / 0.250' in page.locator('#stick-live-left').inner_text()
        # Old profiles default to disabled features; malformed controls cannot be imported.
        assert page.evaluate("!ControllerMotionStudio.valid({motion:{countsPerTurn:0}})")
        assert page.evaluate("!ControllerMotionStudio.valid({radial:{sectors:'invalid'}})")
        assert page.evaluate("ControllerMotionStudio.defaults().motion.countsPerTurn") == 101.25
        # Captures ending after navigation cannot change the profile.
        page.evaluate('window.delayRecord=true')
        page.locator('#radial-record-0').click()
        page.wait_for_function("typeof window.finishRecording==='function'")
        page.locator('[data-view="profiles"]').click()
        page.evaluate('window.finishRecording([17,65])')
        assert page.evaluate('window.motionStudio.controls().radial.sectors[0].chord') == [18, 9]
        page.locator('[data-view="touch"]').click()
        page.locator('#motion-save').click()
        page.wait_for_function('!!window.saved')
        archive = page.evaluate('window.controllerStudio.archive()')
        assert archive['profiles'][0]['controls']['motion']['countsPerTurn'] == 7200
        for width in (390, 640, 900, 1280, 1600):
            page.set_viewport_size({'width': width, 'height': 900})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'), width
        page.set_viewport_size({'width': 1280, 'height': 1000})
        page.locator('#radial-workbench').scroll_into_view_if_needed()
        page.screenshot(path=str(ROOT / 'target/motion-input-check.png'))
        for os in ('macos', 'linux'):
            restored = browser.new_page(viewport={'width': 900, 'height': 800})
            mock_core(restored)
            capabilities = {'os': os, 'desktopInput': True, 'xinput': False, 'screenColor': False, 'systemAudioBridge': False}
            restored.add_init_script('const motionBase=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(n,a)=>n===\'load_profiles\'?'+json.dumps(archive)+':n===\'platform_capabilities\'?'+json.dumps(capabilities)+':motionBase(n,a);')
            restored.goto((ROOT / 'ui/index.html').as_uri())
            restored.wait_for_function('!!window.motionStudio && !!window.coreHandler')
            restored.locator('[data-view="touch"]').click()
            assert not restored.locator('#stick-workbench').is_visible()
            assert restored.get_by_label('Mouse counts per 360°', exact=True).input_value() == '7200'
            assert restored.get_by_label('Sector 1 '+('Command' if os == 'macos' else 'Super'), exact=True).count() == 1
            restored.close()
        assert not errors, errors
        browser.close()
        print('Motion input smoke passed: profile roundtrip, applied state, per-pad bias, radial capture, shortcut cancellation and portable layouts')


if __name__ == '__main__':
    main()
