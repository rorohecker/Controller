"""Game profile persistence, event switching, shortcuts, and Touch layout regression checks."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome = Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        mock_core(page)
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto((ROOT / "ui/index.html").as_uri())
        page.wait_for_function("!!window.coreHandler")
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB'})")
        page.locator('[data-view="triggers"]').click()
        page.locator('#trigger-preset-name').fill('My sniper')
        page.locator('#save-trigger-preset').click()
        page.wait_for_function("window.saved?.triggerPresets?.length===1")
        preset_id = page.evaluate("window.saved.triggerPresets[0].id")
        page.locator('[data-view="profiles"]').click()
        page.locator('#add-game-action').click()
        page.locator('.game-action-card input').first.fill('Roll')
        page.locator('.game-action-card input').first.press('Tab')
        page.locator('.game-action-card select').first.select_option('double')
        page.locator('#game-actions-enabled').check()
        page.locator('#add-weapon-binding').click()
        page.locator('.weapon-binding input').fill('smg')
        page.locator('.weapon-binding input').press('Tab')
        page.locator('#game-link-enabled').check()
        page.locator('#save-game-actions').click()
        page.wait_for_function("window.sent.some(c=>c.action==='setGameActions'&&c.enabled&&c.rules[0].source==='double')")
        assert page.evaluate("window.saved.profiles.find(p=>p.id===window.saved.activeId).weaponBindings[0].presetId") == preset_id
        page.evaluate("window.coreHandler({type:'gameEvent',weapon:'smg',events:['dash']})")
        page.wait_for_function("document.querySelector('#game-link-status').textContent.includes('My sniper')")
        page.evaluate("window.coreHandler({type:'safetyReset'});window.coreHandler({type:'pcMode',enabled:false})")
        page.wait_for_function("window.sent.filter(c=>c.action==='setGameActions').at(-1)?.enabled===false")
        before=page.evaluate("window.sent.filter(c=>c.action==='applyTriggers').length")
        page.evaluate("window.coreHandler({type:'gameEvent',weapon:'smg',events:[]})")
        assert page.evaluate("window.sent.filter(c=>c.action==='applyTriggers').length") == before
        page.locator('#save-game-actions').click()
        page.wait_for_function("window.sent.filter(c=>c.action==='setGameActions').at(-1)?.enabled===true")
        page.locator('#export-profiles').click()
        page.wait_for_function("window.exported?.triggerPresets?.length===1")
        assert page.evaluate("window.exported.profiles.find(p=>p.id===window.exported.activeId).actions[0].name") == 'Roll'
        page.locator('[data-view="pc-controls"]').click()
        first = page.locator('.pc-binding').first
        first.locator('[aria-label="CROSS record shortcut"]').click()
        page.keyboard.press('Control+Shift+K')
        page.wait_for_function("window.sent.some(c=>c.action==='setPcBindings'&&c.bindings.some(b=>b.button===0&&b.chord?.join(',')==='17,16,75'))")
        unbound = page.locator('.pc-binding').filter(has=page.locator('[aria-label="CREATE action"]'))
        unbound.locator('select').first.select_option('launch')
        unbound.locator('[aria-label="CREATE app path"]').fill(r'C:\Windows\notepad.exe')
        unbound.locator('[aria-label="CREATE app path"]').press('Tab')
        page.wait_for_function("window.sent.some(c=>c.action==='setPcBindings'&&c.bindings.some(b=>b.button===8&&b.behavior==='launch'&&b.vk===0))")
        # Reload from the persisted archive rather than just checking an in-memory editor.
        saved = page.evaluate('window.saved')
        restored = browser.new_page(viewport={"width": 900, "height": 690})
        mock_core(restored)
        restored.add_init_script("window.__TAURI__.core.originalInvoke=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(name,args)=>name==='load_profiles'?" + __import__('json').dumps(saved) + ":window.__TAURI__.core.originalInvoke(name,args)")
        restored.goto((ROOT / "ui/index.html").as_uri())
        restored.locator('[data-view="profiles"]').click()
        assert restored.locator('.game-action-card').count() == 1
        assert restored.locator('.weapon-binding input').input_value() == 'smg'
        for width in (640, 900, 1280):
            page.set_viewport_size({"width": width, "height": 900})
            page.locator('[data-view="touch"]').click()
            assert page.evaluate("document.documentElement.scrollWidth<=innerWidth+1"), width
            for selector in ('#touch-mouse-toggle', '#touch-gesture-toggle', '#gyro-aim-toggle', '#xinput-toggle'):
                assert page.locator(selector).evaluate("e=>{const b=e.getBoundingClientRect(),p=e.parentElement.querySelector('p').getBoundingClientRect();return b.left>=p.right-1||b.top>=p.bottom-1||b.bottom<=p.top+1}"), (width, selector)
        page.screenshot(path=str(ROOT / 'target/touch-profile-check.png'), full_page=True)
        assert not errors, errors
        browser.close()
        print('Game profiles smoke passed: presets, action rules, weapon events, archive reload, launch, key chords, Touch layout')


if __name__ == '__main__':
    main()
