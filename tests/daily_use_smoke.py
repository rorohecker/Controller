"""Save/apply state, native confirmation, Stop all, controls and scaling states."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event

VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights', 'profiles', 'calibration', 'settings', 'diagnostics']


def setup(page):
    mock_core(page)
    page.add_init_script("""const dailyBase=window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke=async(n,a)=>{
        if(n==='load_profiles'&&window.holdProfileLoad)return new Promise(r=>window.finishProfileLoad=r);
        if(n==='load_profiles'&&window.rejectProfileLoad)throw Error('Profiles unreadable');
        if(n==='save_profiles'&&window.failSave)throw Error('Disk unavailable');
        if(n==='save_profiles'&&window.holdSave)await new Promise(r=>window.releaseSave=r);
        if(n==='send_command'&&a.command.action==='applyTriggers'&&window.holdTrigger){window.holdTrigger=false;window.triggerWaiting=true;await new Promise(r=>window.releaseTrigger=r);}
        const result=await dailyBase(n,a);
        if(n==='send_command'){
          if(a.command.action==='applyTriggers'&&window.rejectTriggers)setTimeout(()=>window.coreHandler({type:'error',message:'HID write failed'}),0);
          if(a.command.action==='profileCheckpoint'){
            window.lastCheckpoint=a.command.requestId;
            if(!window.holdCheckpoint)setTimeout(()=>window.coreHandler({type:'profileCheckpoint',requestId:a.command.requestId,connected:true,controllerId:'pad-A'}),20);
          }
        }
        return result;
      };""")


def main():
    with sync_playwright() as p:
        chrome = Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        setup(page)
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function('!!window.dailyUse && !!window.coreHandler && window.dailyUse.loadedOnce')
        assert page.locator('#profile-apply-state').inner_text() == 'No controller'
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-A'})")
        pad_event(page)
        page.locator('[data-view="profiles"]').click()
        page.locator('#save-profile').click()
        page.wait_for_function("document.querySelector('#profile-save-state').textContent==='Saved'")
        # Native save errors do not mark edits as saved; retry recovers.
        page.evaluate('window.failSave=true')
        page.locator('#profile-description').fill('A deliberate unsaved note')
        page.locator('#save-profile').click()
        page.wait_for_function("document.querySelector('#profile-save-state').textContent==='Save failed'")
        assert 'Disk unavailable' in page.locator('#profile-change-summary').text_content(), page.locator('#profile-change-summary').text_content()
        page.evaluate('window.failSave=false')
        page.locator('#save-profile').click()
        page.wait_for_function("document.querySelector('#profile-save-state').textContent==='Saved'")
        # An IPC enqueue alone cannot claim the profile was applied.
        page.evaluate('window.holdCheckpoint=true')
        page.locator('#apply-profile-now').click()
        page.wait_for_function('!!window.lastCheckpoint')
        assert page.locator('#profile-apply-state').inner_text() == 'Applying…'
        page.evaluate("window.coreHandler({type:'profileCheckpoint',requestId:window.lastCheckpoint-1,connected:true,controllerId:'pad-A'})")
        assert page.locator('#profile-apply-state').inner_text() == 'Applying…'
        page.evaluate("window.coreHandler({type:'profileCheckpoint',requestId:window.lastCheckpoint,connected:true,controllerId:'pad-A'})")
        page.wait_for_function("document.querySelector('#profile-apply-state').textContent==='Applied'")
        # Restoring a known applied configuration changes the editor without writing output.
        old_strength = page.evaluate('window.controllerStudio.profile().triggers.left.strength')
        page.evaluate("()=>{window.controllerStudio.profile().triggers.left.strength=81;window.controllerStudio.renderAllProfileControls();window.controllerStudio.saveProfiles(true);}")
        page.wait_for_function("document.querySelector('#profile-save-state').textContent==='Saved'")
        assert page.locator('#profile-apply-state').inner_text()=='Changes not applied'
        page.locator('#daily-summary').click()
        sent = page.evaluate('window.sent.length')
        page.locator('#daily-restore-applied').click()
        assert page.evaluate('window.controllerStudio.profile().triggers.left.strength') == old_strength
        assert page.evaluate('window.sent.length') == sent
        # Revert a draft that has not fired its change/autosave event.
        page.locator('#profile-description').fill('Not committed')
        page.wait_for_function("document.querySelector('#profile-save-state').textContent==='Unsaved changes'")
        # Prevent blur's autosave so the test specifically exercises the draft.
        page.evaluate('window.dailyUse.revert(false)')
        assert page.locator('#profile-description').input_value() == 'A deliberate unsaved note'
        # An error before the native checkpoint keeps apply failed.
        page.evaluate('window.holdCheckpoint=false;window.rejectTriggers=true')
        page.locator('#apply-profile-now').click()
        page.wait_for_function("document.querySelector('#profile-apply-state').textContent.includes('HID write failed')")
        page.evaluate('window.rejectTriggers=false')
        # Manual writes during an apply invalidate its confirmation; its packet
        # still comes from the immutable snapshot captured at the start.
        captured = page.evaluate('window.controllerStudio.profile().triggers.left.strength')
        page.evaluate('()=>{window.holdTrigger=true;window.profileJob=window.controllerStudio.applyProfileHardware(true);}')
        page.wait_for_function('window.triggerWaiting===true')
        page.evaluate("async()=>{const a=window.controllerStudio;a.profile().triggers.left.strength+=1;await a.send({action:'setControlProfile',settings:structuredClone(a.profile().controls)});window.releaseTrigger();await window.profileJob;}")
        page.wait_for_function("document.querySelector('#profile-apply-state').textContent.includes('changed while')")
        assert page.evaluate("window.sent.filter(c=>c.action==='applyTriggers').at(-1).left.strength") == captured
        # Source names come from native ownership and actual USB mixer activity.
        page.evaluate("""window.coreHandler({type:'feedbackState',controllerId:'pad-A',triggerSource:'Weapon feedback',motorSource:'Idle',lightSource:'Battery'});
          window.coreHandler({type:'audioMeters',peaks:[.2,.4,.4],active:true,file:{mode:'playing',speaker:true,grips:true},gameActive:true,bridge:true});
          window.coreHandler({type:'inputEffects',enabled:true});""")
        sources = page.locator('#feedback-sources').text_content()
        assert all(name in sources for name in ('Weapon feedback', 'Music', 'Xbox rumble', 'System audio', 'Battery')), sources
        assert 'Armed' in page.locator('#feedback-armed').text_content()
        # Stale events from another controller must not replace ownership.
        page.evaluate("window.coreHandler({type:'feedbackState',controllerId:'pad-B',triggerSource:'Wrong pad',motorSource:'Idle',lightSource:'Released'})")
        assert 'Wrong pad' not in page.locator('#feedback-sources').text_content()
        page.keyboard.press('Control+Shift+Backspace')
        page.wait_for_timeout(200)
        assert page.evaluate("window.sent.slice(-8).some(c=>c.action==='reset')"), (page.evaluate('window.sent.slice(-8).map(c=>c.action)'), errors)
        assert page.evaluate('window.controllerStudio.state.autoSuppressed')
        assert page.evaluate('window.gameSessions.active===null')
        page.evaluate("window.coreHandler({type:'safetyReset'})")
        assert 'Stopped' in page.locator('#profile-apply-state').inner_text()
        assert 'Music' not in page.locator('#feedback-sources').text_content()
        # Disconnection cancels any old apply acknowledgement.
        page.evaluate('window.holdCheckpoint=true')
        page.locator('#apply-profile-now').click()
        page.wait_for_function('!!window.dailyUse.pending')
        page.evaluate("window.coreHandler({type:'device',connected:false});window.coreHandler({type:'profileCheckpoint',requestId:window.lastCheckpoint,connected:true,controllerId:'pad-A'})")
        assert page.locator('#profile-apply-state').inner_text() == 'No controller'
        # Numeric tools dispatch the original slider's real handlers and validate bounds.
        page.locator('[data-view="pc-controls"]').click()
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-A'})")
        number = page.locator('#pc-click-threshold-number-control')
        number.fill('68'); number.press('Tab')
        assert page.locator('#pc-click-threshold').input_value() == '68'
        assert page.evaluate("window.sent.filter(c=>c.action==='setPcBindings').at(-1).click_threshold") == .68
        number.fill('999'); number.press('Tab')
        assert page.locator('#pc-click-threshold').input_value() == '68'
        page.locator('#pc-click-threshold-reset-control').click()
        assert page.locator('#pc-click-threshold').input_value() == page.locator('#pc-click-threshold').evaluate('e=>e.defaultValue')
        page.locator('[data-view="settings"]').click()
        page.locator('#display-density').select_option('compact')
        page.locator('#display-numbers').uncheck()
        assert page.locator('body').get_attribute('data-density') == 'compact'
        assert not page.locator('.range-tools').first.is_visible()
        page.reload(); page.wait_for_function('!!window.dailyUse && !!window.coreHandler')
        assert page.locator('body').get_attribute('data-density') == 'compact'
        assert page.evaluate("JSON.parse(localStorage.getItem('controller.display.v1')).numbers") is False
        page.locator('[data-view="settings"]').click()
        page.locator('#display-reset').click()
        assert page.locator('body').get_attribute('data-density') == 'comfortable'
        page.get_by_label('Reduce decorative motion', exact=True).check()
        page.wait_for_function("document.querySelector('#display-motion-status').textContent.includes('Reduced motion')")
        page.evaluate("window.dailyUse.pulse('applied')")
        assert page.locator('#daily-status').get_attribute('data-pulse') is None
        page.locator('[data-view="haptics"]').click()
        page.locator('#audio-mixer-reset').scroll_into_view_if_needed()
        page.wait_for_function("getComputedStyle(document.querySelector('#stop-all')).position==='fixed'")
        assert page.locator('#stop-all').is_visible()
        page.locator('#stop-all').click()
        # Effective CSS widths shrink at OS scaling while device pixels grow.
        for scale in (1, 1.25, 1.5, 2):
            scaled = browser.new_page(viewport={'width': round(1280 / scale), 'height': round(1000 / scale)}, device_scale_factor=scale, reduced_motion='reduce')
            setup(scaled); scaled.on('pageerror', lambda e: errors.append(str(e)))
            scaled.goto((ROOT / 'ui/index.html').as_uri()); scaled.wait_for_function('!!window.dailyUse && !!window.coreHandler')
            for density in ('comfortable', 'compact'):
                scaled.evaluate("d=>{window.dailyUse.preferences.density=d;window.dailyUse.applyPreferences();}", density)
                for view in VIEWS:
                    scaled.evaluate('view=>window.controllerStudio.navigate(view)', view)
                    assert scaled.evaluate('document.documentElement.scrollWidth<=innerWidth+1'), (scale, density, view)
                    assert scaled.locator('#stop-all').is_visible()
            scaled.locator('#daily-summary').click()
            assert scaled.locator('#daily-save').is_visible()
            box = scaled.locator('.daily-detail-content').bounding_box()
            assert box['x'] >= 0 and box['x'] + box['width'] <= 1280 / scale + 1
            assert scaled.locator('#daily-status').evaluate('e=>getComputedStyle(e).animationName') == 'none'
            scaled.close()
        # Loading and failed archives keep a readable, explicit state at 200%.
        for failed in (False, True):
            special = browser.new_page(viewport={'width': 640, 'height': 500}, device_scale_factor=2)
            setup(special)
            special.add_init_script('window.'+('rejectProfileLoad' if failed else 'holdProfileLoad')+'=true;')
            special.goto((ROOT / 'ui/index.html').as_uri())
            special.wait_for_function('!!window.dailyUse')
            special.wait_for_function("document.querySelector('#profile-save-state').textContent==="+repr('Load failed' if failed else 'Loading profiles'))
            assert special.evaluate('document.documentElement.scrollWidth<=innerWidth+1')
            assert special.locator('#stop-all').is_visible()
            if not failed:
                special.evaluate('window.finishProfileLoad(null)')
                special.wait_for_function('!!window.coreHandler')
            special.close()
        # Denied macOS permissions use their own controls and still fit at 200%.
        mac = browser.new_page(viewport={'width': 640, 'height': 500}, device_scale_factor=2)
        setup(mac)
        mac.add_init_script("""const macBase=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(n,a)=>
          n==='platform_capabilities'?{os:'macos',desktopInput:true,xinput:false,systemAudioBridge:false,screenColor:false}:
          n==='desktop_access_status'?{controlAllowed:false,recordingAllowed:false}:macBase(n,a);""")
        mac.goto((ROOT / 'ui/index.html').as_uri());mac.wait_for_function('!!window.coreHandler')
        mac.locator('[data-view="pc-controls"]').click()
        mac.wait_for_function("document.querySelector('#desktop-access-status').textContent.includes('permission needed')")
        assert mac.locator('.desktop-access').is_visible()
        assert 'Command' in mac.locator('#stop-all').get_attribute('title')
        assert mac.evaluate('document.documentElement.scrollWidth<=innerWidth+1')
        mac.close()
        page.locator('[data-view="haptics"]').click()
        page.screenshot(path=str(ROOT / 'target/daily-use-check.png'))
        assert not errors, errors
        browser.close()
        print('Daily use smoke passed: save failures, native apply confirmation, source ownership, Stop all, numeric controls, preferences and 100-200% scaling')


if __name__ == '__main__':
    main()
