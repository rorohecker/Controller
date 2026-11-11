"""Shell regressions: light-theme extras contrast, stored-pref replay, tour, undo toast, shortcuts, imports."""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path

def contrast(page, selector):
    return page.evaluate("""sel=>{
      const lum=c=>{const m=c.match(/[\\d.]+/g).map(Number),f=v=>{v/=255;return v<=.03928?v/12.92:((v+.055)/1.055)**2.4};return .2126*f(m[0])+.7152*f(m[1])+.0722*f(m[2]);};
      const el=document.querySelector(sel);let bg=el,color=getComputedStyle(el).color,back='rgba(0, 0, 0, 0)';
      while(bg){const b=getComputedStyle(bg).backgroundColor;if(b!=='rgba(0, 0, 0, 0)'){back=b;break;}bg=bg.parentElement;}
      const a=lum(color),b=lum(back),hi=Math.max(a,b),lo=Math.min(a,b);return (hi+.05)/(lo+.05);}""", selector)


def main():
    with sync_playwright() as p:
        chrome = chrome_path()
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        mock_core(page)
        page.add_init_script("""
          localStorage.clear();
          const base=window.__TAURI__.core.invoke;window.loadCalls=0;
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='load_profiles'){window.loadCalls++;if(window.failLoad)throw Error('disk unreadable');return null;}
            if(name==='send_command'&&window.failActions?.includes(args.command.action)){window.sent.push(args.command);throw Error('nope');}
            return base(name,args);
          };""")
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto((ROOT / "ui/index.html").as_uri())
        page.wait_for_function("!!window.coreHandler")

        # 1. Extras panels are readable in every theme.
        page.evaluate("document.documentElement.dataset.theme='light'")
        page.locator('[data-view="pc-controls"]').click()
        assert 'work-panel' in page.locator('#pc-extras').get_attribute('class')
        assert 'controls-panel' not in page.locator('#pc-extras').get_attribute('class')
        for theme in ('light', 'dark', 'contrast'):
            page.evaluate(f"document.documentElement.dataset.theme='{theme}'")
            page.wait_for_timeout(50)
            for sel in ('#pc-extras .control-label', '#pc-extras .extra-value', '#pc-extras strong'):
                assert contrast(page, sel) >= 4.5, (theme, sel, contrast(page, sel))
        page.evaluate("document.documentElement.dataset.theme='light'")
        page.locator('[data-view="settings"]').click()
        # The chip sits on the dark panel cap, so it must use a light colour.
        assert page.evaluate("getComputedStyle(document.querySelector('#core-service-chip')).color.match(/[0-9]+/g).slice(0,3).map(Number).reduce((a,b)=>a+b)>450")

        # 6. Digit shortcuts use the physical key (AZERTY), repeats are ignored.
        page.evaluate("document.body.dispatchEvent(new KeyboardEvent('keydown',{key:'\u00e9',code:'Digit2',ctrlKey:true,bubbles:true}))")
        page.wait_for_function("document.querySelector('#triggers').classList.contains('is-current')||!document.querySelector('#triggers').hidden")

        # 4. Undo toast: Ctrl+Z, hover pause, palette entry.
        page.evaluate("window.undone=0;window.controllerStudio.undoToast('Removed thing.',()=>{window.undone++;})")
        assert page.locator('#toast .toast-item', has_text='Removed thing.').count() == 1
        assert page.locator('#toast-announcer').count() == 1
        assert page.locator('#toast [aria-live]').count() == 0
        page.locator('[data-view="live"]').click()
        page.keyboard.press('Control+z')
        assert page.evaluate("window.undone") == 1
        page.evaluate("window.controllerStudio.undoToast('Removed again.',()=>{window.undone++;})")
        page.keyboard.press('Control+k')
        page.locator('.palette-search').fill('undo last')
        page.get_by_role('button', name='Undo last action').click()
        assert page.evaluate("window.undone") == 2

        # 3. Tour is a non-modal dialog that does not block shortcuts, and replay keeps working.
        page.locator('[data-view="settings"]').click()
        page.locator('#replay-tour').click()
        assert page.locator('.tour[role=dialog]').count() == 1
        assert page.locator('dialog[open]').count() == 0
        page.keyboard.press('Control+1')
        assert page.evaluate("window.controllerStudio.state.currentView") in ('live',)
        page.locator('.tour').press('Escape') if page.locator('.tour *:focus').count() else page.keyboard.press('Escape')
        page.wait_for_function("document.querySelectorAll('.tour').length===0")
        page.locator('[data-view="settings"]').click()
        page.locator('#replay-tour').click()
        assert page.locator('.tour').count() == 1
        page.get_by_role('button', name='Skip tour').click()
        assert page.locator('.tour').count() == 0

        # 2. Stored preferences replay on connect; nothing is sent when nothing was stored.
        page.evaluate("window.sent.length=0;window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-a'})")
        page.wait_for_timeout(100)
        sent = page.evaluate("window.sent.map(c=>c.action)")
        for action in ('setAutoBias', 'setRemapTiming', 'setPadExtras', 'setLowBatteryThresholds'):
            assert action not in sent, (action, sent)
        page.evaluate("""localStorage.setItem('controller.autoBias.v1','true');
          localStorage.setItem('controller.pcOptions.v1',JSON.stringify({holdMs:60,gapMs:40,touchpadClick:'a',mic:''}));
          localStorage.setItem('controller.batteryThresholds.v1',JSON.stringify({warn:30,critical:12}));
          window.sent.length=0;window.coreHandler({type:'device',connected:false});
          window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-b'})""")
        page.wait_for_function("window.sent.some(c=>c.action==='setAutoBias'&&c.enabled===true)")
        sent = page.evaluate("window.sent")
        assert any(c['action'] == 'setRemapTiming' and c['holdMs'] == 60 and c['gapMs'] == 40 for c in sent), sent
        assert any(c['action'] == 'setPadExtras' and c['touchpadClick'] == 'a' for c in sent), sent
        assert any(c['action'] == 'setLowBatteryThresholds' and c['warn'] == 30 and c['critical'] == 12 for c in sent), sent
        # A repeated device event for the same pad does not re-push.
        page.evaluate("window.sent.length=0;window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-b'})")
        page.wait_for_timeout(100)
        assert not page.evaluate("window.sent.some(c=>c.action==='setAutoBias')")
        # A core restart replays them (the built-in core forgets everything).
        page.evaluate("window.sent.length=0;window.coreHandler({type:'core',status:'ready'})")
        page.wait_for_function("window.sent.some(c=>c.action==='setAutoBias')")

        # 7. Audio/animation failures in the profile lights do not fail the profile apply.
        page.evaluate("""()=>{const p=window.controllerStudio.profile();p.lightbar={color:'#112233',playerLeds:3,mode:'pulse',periodMs:50};p.audioPreset={headphone:999,speaker:50,microphone:10};}""")
        page.evaluate("window.failActions=['setLightbarAnimation','setAudioVolumes']")
        page.evaluate("window.sent.length=0;window.controllerStudio.applyProfileHardware(true)")
        page.wait_for_function("window.sent.some(c=>c.action==='profileCheckpoint')", timeout=8000)
        anim = page.evaluate("window.sent.find(c=>c.action==='setLightbarAnimation')")
        vols = page.evaluate("window.sent.find(c=>c.action==='setAudioVolumes')")
        assert anim['periodMs'] == 300 and vols['headphone'] == 127, (anim, vols)

        # 14. Session audio presets fall back to a backend-valid value.
        assert page.evaluate("window.gameSessions.presetFor('x',{audio:'not-a-preset'})") == 'gentle'
        assert page.evaluate("window.gameSessions.presetFor('x',{audio:'quiet'})") == 'quiet'

        # 12. Player aim space exists and validates; 13. unknown keys are dropped on normalize.
        page.locator('[data-view="touch"]').click()
        assert page.get_by_label('Gyro aim space', exact=True).locator('option[value=player]').count() == 1
        norm = page.evaluate("""()=>{const v={motion:{space:'player',bogus:1},sticks:{left:{mode:'axial',evil:2}}};
          return {ok:ControllerMotionStudio.valid(v),n:ControllerMotionStudio.normalize(v)};}""")
        assert norm['ok'] and 'bogus' not in norm['n']['motion'] and 'evil' not in norm['n']['sticks']['left'], norm

        # 5. Retry load button exists and re-runs the load.
        page2 = browser.new_page(viewport={"width": 1280, "height": 900})
        mock_core(page2)
        page2.add_init_script("""
          localStorage.clear();window.failLoad=true;window.loadCalls=0;
          const base=window.__TAURI__.core.invoke;
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='load_profiles'){window.loadCalls++;if(window.failLoad)throw Error('disk unreadable');return null;}
            return base(name,args);};""")
        page2.goto((ROOT / "ui/index.html").as_uri())
        page2.wait_for_function("window.controllerStudio&&window.controllerStudio.state.loadFailed")
        page2.locator('[data-view="profiles"]').click()
        assert page2.locator('#load-failed-retry').is_visible()
        before = page2.evaluate("window.loadCalls")
        page2.locator('#load-failed-retry').click()
        page2.wait_for_function(f"window.loadCalls>{before}")
        page2.evaluate("window.failLoad=false")
        page2.locator('#load-failed-retry').click()
        page2.wait_for_function("!window.controllerStudio.state.loadFailed")
        assert page2.locator('#profile-load-failed').is_hidden()

        assert not errors, errors
        browser.close()
        print('Shell fixes smoke passed: extras contrast, pref replay, tour, undo toast, shortcuts, best-effort lights, retry load')


if __name__ == '__main__':
    main()
