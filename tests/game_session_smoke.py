"""Session restoration, action controls and portable community records (no hardware)."""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome = Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        mock_core(page)
        page.add_init_script('''const invoke=window.__TAURI__.core.invoke;
          window.__TAURI__.core.invoke=async(n,a)=>{const result=await invoke(n,a);
            if(n==='send_command'&&a.command.action==='applyTriggers'&&window.delayTrigger){window.delayTrigger=false;await new Promise(resolve=>window.resumeTrigger=resolve);}
            return result;};''')
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto((ROOT / "ui/index.html").as_uri())
        page.wait_for_function("!!window.coreHandler")
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB'})")
        page.locator('[data-view="profiles"]').click()
        page.locator('#add-game-action').click()
        page.locator('.game-action-card label').filter(has_text='Retrigger while playing').locator('select').select_option('ignore')
        page.locator('.game-action-card label').filter(has_text='Only while held').locator('select').select_option('6')
        page.locator('.game-action-card label').filter(has_text='Condition threshold').locator('input').fill('75')
        page.locator('.game-action-card label').filter(has_text='Condition threshold').locator('input').press('Tab')
        page.get_by_label('Stop on button release', exact=True).check()
        page.locator('.game-action-card label').filter(has_text='Cancel event').locator('input').fill('death')
        page.locator('.game-action-card label').filter(has_text='Cancel event').locator('input').press('Tab')
        page.get_by_label('Author', exact=True).fill('Local tester')
        page.get_by_label('Author', exact=True).press('Tab')
        page.get_by_label('Profile version', exact=True).fill('2.1')
        page.get_by_label('Profile version', exact=True).press('Tab')
        page.get_by_label('Source URL', exact=False).fill('javascript:alert(1)')
        page.get_by_label('Source URL', exact=False).press('Tab')
        assert page.get_by_label('Source URL', exact=False).input_value() == ''
        page.get_by_label('Source URL', exact=False).fill('https://example.com/profiles/test')
        page.get_by_label('Source URL', exact=False).press('Tab')
        page.get_by_label('Compatibility notes', exact=True).fill('Inputs and USB rumble work')
        page.get_by_role('button', name='Record my test', exact=True).click()
        page.wait_for_function("window.saved?.profiles[0].community?.reports.length===1")
        assert page.evaluate("window.saved.profiles[0].actions[0].conditionThreshold") == .75
        assert page.evaluate("window.saved.profiles[0].actions[0].retrigger") == 'ignore'
        assert page.evaluate("window.saved.profiles[0].actions[0].cancelOnRelease")
        assert page.evaluate("window.saved.profiles[0].actions[0].cancelEvent") == 'death'
        assert page.evaluate("window.controllerStudio.validProfile(window.saved.profiles[0])")
        # Establish actual prior settings, then enter an executable-matched game.
        page.evaluate("""async () => {
          const a=window.controllerStudio;
          await a.send({action:'setGyroAim',enabled:false,sensitivity:1,yawScale:1,pitchScale:1,invertY:false,activation:'always',bias:[0,0,0],deadzone:90,smoothing:.18});
          await a.send({action:'setXinput',enabled:false});
          const game=a.normalizeProfile(a.clone(a.profile()));game.id='session-test';game.name='Test game';game.matchExe='testgame.exe';
          game.session={enabled:true,inputRoute:'xinput',audio:'off',audioGain:40,hideHint:true};game.gyroAim.enabled=true;game.actionsEnabled=true;
          a.state.profiles.push(game);
          window.coreHandler({type:'foreground',exe:'testgame.exe',title:'Test game'});
        }""")
        page.wait_for_function("window.gameSessions.active?.id==='session-test'&&window.sent.some(c=>c.action==='setGyroAim'&&c.enabled)")
        page.evaluate("window.coreHandler({type:'xinput',enabled:true,status:'active'});window.coreHandler({type:'gameActionsStatus',enabled:true});window.coreHandler({type:'gameFeedback',motors:[120,80],delivered:true})")
        assert 'Virtual Xbox' in page.locator('#setup-route').inner_text()
        assert '120 / 80' in page.locator('#game-feedback-status').inner_text()
        await_queue = "async()=>{await window.gameSessions.queue}"
        page.evaluate(await_queue)
        armed = page.evaluate("window.sent.filter(c=>c.action==='setGameActions').length")
        page.evaluate("window.coreHandler({type:'foreground',exe:'testgame.exe',title:'A new game window title'})")
        page.evaluate(await_queue)
        assert page.evaluate("window.sent.filter(c=>c.action==='setGameActions').length") == armed
        # Manual audio changes should survive leaving the session.
        page.evaluate("async()=>{await window.controllerStudio.send({action:'setAudioBridge',enabled:true,gain:.25,preset:'gentle'});window.coreHandler({type:'foreground',exe:'notepad.exe'})}")
        page.evaluate(await_queue)
        assert page.evaluate("window.gameSessions.active===null")
        assert page.evaluate("window.sent.filter(c=>c.action==='setXinput').at(-1).enabled===false")
        assert page.evaluate("window.sent.filter(c=>c.action==='setGyroAim').at(-1).enabled===false")
        assert page.evaluate("window.sent.filter(c=>c.action==='setAudioBridge').at(-1).gain") == .25
        # Reset must cancel queued session starts, not restart effects later.
        page.evaluate("window.coreHandler({type:'foreground',exe:'testgame.exe'});window.coreHandler({type:'safetyReset'})")
        page.evaluate(await_queue)
        assert page.evaluate("window.gameSessions.active===null")
        assert page.evaluate("window.controllerStudio.state.autoSuppressed")
        # Cancel an application already awaiting a native acknowledgement.
        page.evaluate("()=>{window.delayTrigger=true;window.gameSessions.enqueue(()=>window.gameSessions.start(window.controllerStudio.state.profiles.at(-1),true));}")
        page.wait_for_function("typeof window.resumeTrigger==='function'")
        before = page.evaluate('window.sent.length')
        page.evaluate("window.coreHandler({type:'safetyReset'});window.resumeTrigger()")
        page.evaluate(await_queue)
        after = page.evaluate('index=>window.sent.slice(index).map(c=>c.action)', before)
        assert all(action == 'stopSpeakerSound' for action in after), after
        assert page.evaluate('window.gameSessions.active===null')
        # Records survive real archive serialization/reload, not just editor state.
        archive = page.evaluate('window.controllerStudio.archive()')
        assert archive['profiles'][0]['community']['author'] == 'Local tester'
        assert archive['profiles'][0]['community']['reports'][0]['platform'] == 'windows'
        assert archive['profiles'][-1]['session']['inputRoute'] == 'xinput'
        restored = browser.new_page(viewport={"width": 900, "height": 800})
        mock_core(restored)
        restored.add_init_script("const base=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(n,a)=>n==='load_profiles'?"+json.dumps(archive)+":n==='platform_capabilities'?{os:'macos',desktopInput:true,xinput:false,systemAudioBridge:false,screenColor:false}:base(n,a)")
        restored.goto((ROOT / "ui/index.html").as_uri())
        restored.wait_for_function("window.controllerPlatform.os==='macos'&&!!window.coreHandler")
        restored.locator('[data-view="profiles"]').click()
        assert restored.get_by_label('Author', exact=True).input_value() == 'Local tester'
        assert restored.locator('.compatibility-reports li').count() == 1
        assert restored.locator('#game-session-settings select').count() == 0
        assert restored.evaluate("window.controllerStudio.archive().profiles.at(-1).session.inputRoute") == 'xinput'
        for width in (390, 640, 900, 1600):
            page.set_viewport_size({"width": width, "height": 900})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'), width
        assert not errors, errors
        browser.close()
        print('Game session smoke passed: routing restore, manual overrides, reset cancellation, conditions and profile records')


if __name__ == '__main__':
    main()
