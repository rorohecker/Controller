"""Recovery, trigger history, macro editing, Steam assignment, and packet replay without hardware."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome = Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        page.add_init_script(r"""
        const original=window.__TAURI__.core.invoke;
        window.backups=[];window.replayed=[];
        window.__TAURI__.core.invoke=async(name,args)=>{
          if(name==='profile_backups'){if(args.create)window.backups.unshift({id:'profiles-1.json',createdMs:Date.now(),profiles:window.saved.profiles.length,data:structuredClone(window.saved)});return window.backups;}
          if(name==='restore_profile_backup')return window.backups.find(b=>b.id===args.id).data;
          if(name==='installed_steam_games')return [{appId:'1',name:'Test game',installDir:'C:\\Games',executables:['C:\\Games\\game.exe']}];
          if(name==='platform_diagnostics')return {checks:[{name:'Permissions',detail:'Test permission report',ok:false}]};
          if(name==='send_game_packet'){window.replayed.push(args);setTimeout(()=>window.coreHandler({type:'gameEvent',events:['controller_receiver_test'],format:args.format,packet:args.packet}),10);return null;}
          return original(name,args);
        };
        """)
        errors=[]
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto((ROOT/'ui/index.html').as_uri())
        page.wait_for_function('!!window.studioHooks && !!window.coreHandler')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB'})")
        page.locator('[data-view="profiles"]').click()
        page.locator('#profile-search').fill('rally')
        assert page.locator('.profile-row').count()==1
        page.locator('#profile-search').fill('')
        page.locator('#favorite-profile').click()
        page.wait_for_function('window.saved?.profiles.find(p=>p.id===window.saved.activeId).favorite')
        page.locator('#backup-now').click()
        page.wait_for_function('window.backups.length===1')
        original_name=page.locator('#profile-name').input_value()
        page.locator('#profile-name').fill('Changed after backup')
        page.locator('#save-profile').click()
        page.locator('#backup-list button').first.click()
        page.get_by_role('dialog').get_by_role('button',name='Restore',exact=True).click()
        page.wait_for_function(f"document.querySelector('#profile-name').value==={json.dumps(original_name)}")
        assert page.evaluate('window.sent.some(c=>c.action==="reset")')
        page.locator('#export-profile').click()
        page.wait_for_function('window.exported?.profiles.length===1')
        # Cancel imports before they can mutate the archive; then explicitly accept copies.
        archive=page.evaluate('window.exported')
        count=page.locator('.profile-row').count()
        upload={'name':'profile.json','mimeType':'application/json','buffer':json.dumps(archive).encode()}
        page.locator('#import-file').set_input_files(upload)
        page.get_by_role('dialog').get_by_role('button',name='Cancel',exact=True).click()
        assert page.locator('.profile-row').count()==count
        page.locator('#import-file').set_input_files(upload)
        page.locator('#confirm-import').click()
        page.wait_for_function(f'document.querySelectorAll(".profile-row").length==={count+1}')
        # Baseline history follows the newly selected profile.
        page.locator('[data-view="triggers"]').click()
        page.locator('#trigger-strength-input').fill('70')
        page.locator('#trigger-strength-input').press('Tab')
        page.locator('#trigger-undo').click()
        assert page.locator('#trigger-strength-input').input_value()=='40'
        page.locator('#trigger-redo').click()
        assert page.locator('#trigger-strength-input').input_value()=='70'
        page.locator('#copy-left-right').click()
        assert page.evaluate('JSON.stringify(window.controllerStudio.profile().triggers.left)===JSON.stringify(window.controllerStudio.profile().triggers.right)')
        page.locator('#trigger-preset-name').fill('Test comparison')
        page.locator('#save-trigger-preset').click()
        page.locator('#compare-trigger-preset').click()
        assert page.get_by_role('dialog').locator('table').count()==2
        page.get_by_role('dialog').get_by_role('button',name='Close',exact=True).click()
        page.locator('[data-view="profiles"]').click()
        page.locator('#scan-steam-games').click()
        page.locator('#steam-games-list select').select_option(index=1)
        page.locator('#steam-games-list button').click()
        assert page.locator('#profile-match').input_value()=='game.exe'
        page.locator('#add-game-shortcut').click()
        page.wait_for_function('window.saved.profiles.find(p=>p.id===window.saved.activeId).gameShortcuts.length===1')
        # Macro steps are persisted and sent, but never execute keys in this test.
        page.locator('[data-view="pc-controls"]').click()
        row=page.locator('.pc-binding').filter(has=page.locator('[aria-label="CREATE action"]'))
        row.locator('select').first.select_option('program')
        row.get_by_role('button',name='Edit 1 steps').click()
        page.get_by_role('dialog').locator('.macro-step select').first.select_option('chord')
        page.get_by_role('dialog').get_by_label('Step 1 keys').fill('Ctrl + Shift + K')
        page.get_by_role('dialog').get_by_label('Step 1 keys').press('Tab')
        page.locator('#add-macro-step').click()
        page.get_by_role('dialog').locator('.macro-step select').nth(1).select_option('scroll')
        page.locator('#save-macro').click()
        page.wait_for_function('window.sent.some(c=>c.action==="setPcBindings"&&c.bindings.some(b=>b.program?.steps[0].keys?.join(",")==="17,16,75"&&b.program.steps.length===2))')
        page.locator('#stop-macros').click()
        assert page.evaluate('window.sent.some(c=>c.action==="stopMacros")')
        # Command search respects keyboard shortcut recording and restores keyboard focus.
        page.keyboard.press('Control+k')
        page.get_by_role('dialog').get_by_label('Find a command').fill('Diagnostics')
        page.get_by_role('dialog').get_by_label('Find a command').press('Enter')
        assert page.locator('#diagnostics').is_visible()
        page.locator('#check-platform').click()
        page.wait_for_function('document.querySelector(".studio-check").textContent.includes("Test permission")')
        page.evaluate('window.controllerStudio.profile().gameLink.enabled=true')
        page.locator('#record-game-packets').click()
        page.locator('#test-game-receiver').click()
        page.wait_for_function('document.querySelector("#integration-monitor").textContent.includes("Receiver working")')
        page.locator('#record-game-packets').click()
        recording={'schemaVersion':1,'packets':[{'atMs':0,'format':'json','packet':'{"events":["test"]}'},{'atMs':300,'format':'json','packet':'{"events":["test2"]}'}]}
        before=page.evaluate('window.replayed.length')
        page.locator('#integration-monitor input[type=file]').set_input_files({'name':'replay.json','mimeType':'application/json','buffer':json.dumps(recording).encode()})
        page.wait_for_function(f'window.replayed.length==={before+2}')
        # Safety Reset cancels a pending replay before it can dispatch.
        recording['packets'][0]['atMs']=1000
        recording['packets'][1]['atMs']=2000
        page.locator('#integration-monitor input[type=file]').set_input_files({'name':'replay.json','mimeType':'application/json','buffer':json.dumps(recording).encode()})
        page.evaluate('window.coreHandler({type:"safetyReset"})')
        page.wait_for_timeout(1100)
        assert page.evaluate('window.replayed.length')==before+2
        for width in (390,640,900,1280):
            page.set_viewport_size({'width':width,'height':900})
            for view in ('profiles','pc-controls','diagnostics','triggers'):
                page.evaluate('v=>window.controllerStudio.navigate(v)',view)
                assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),(width,view)
        page.screenshot(path=str(ROOT/'target/studio-tools.png'),full_page=True)
        assert not errors,errors
        browser.close()
        print('Studio smoke passed: search, favorites, backup restore, import preview, undo/redo, comparison, Steam, macros, command palette, replay cancellation, responsive layout')


if __name__=='__main__':
    main()
