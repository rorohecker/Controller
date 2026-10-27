"""OS labels and feature visibility follow native capabilities, not the test browser host."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core


def main():
    with sync_playwright() as p:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        for os in ('windows','macos','linux'):
            page=browser.new_page(viewport={'width':1280,'height':900})
            mock_core(page)
            capabilities={'os':os,'desktopInput':True,'xinput':os=='windows','screenColor':os=='windows','systemAudioBridge':os=='windows','updates':os=='windows'}
            page.add_init_script('const platformInvoke=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=async(name,args)=>name==="platform_capabilities"?'+json.dumps(capabilities)+':platformInvoke(name,args);')
            errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
            page.goto((ROOT/'ui/index.html').as_uri())
            page.wait_for_function('!!window.studioHooks && !!window.coreHandler')
            assert page.locator('body').get_attribute('data-platform')==os
            assert page.locator('#command-palette-button').text_content()==('⌘ K' if os=='macos' else 'Ctrl K')
            page.locator('[data-view="pc-controls"]').click()
            keys=page.locator('[aria-label="CROSS key"]')
            labels=keys.locator('option').all_text_contents()
            expected='Command' if os=='macos' else 'Windows key' if os=='windows' else 'Super'
            assert expected in labels,(os,labels)
            assert not any(label in labels for label in {'Command','Windows key','Super'}-{expected})
            own={'windows':'Windows','macos':'macOS','linux':'Linux'}[os]
            summary=page.locator('#platform-support').inner_text()
            assert all(other not in summary for other in {'Windows','macOS','Linux'}-{own})
            unbound=page.locator('.pc-binding').filter(has=page.locator('[aria-label="CREATE action"]'))
            unbound.locator('select').first.select_option('launch')
            placeholder=unbound.locator('[aria-label="CREATE app path"]').get_attribute('placeholder')
            assert ('/Applications/' in placeholder) if os=='macos' else ('C:' in placeholder) if os=='windows' else placeholder=='/usr/bin/app'
            page.locator('[data-view="touch"]').click()
            assert page.locator('.touch-compatibility').is_visible()==(os=='windows')
            page.locator('[data-view="haptics"]').click()
            assert page.locator('.bridge-panel').is_visible()==(os=='windows')
            page.locator('[data-view="lights"]').click()
            assert page.locator('#screen-light').is_visible()==(os=='windows')
            page.locator('[data-view="diagnostics"]').click()
            assert page.locator('.update-panel').is_visible()==(os=='windows')
            assert page.locator('#diag-xinput').is_visible()==(os=='windows')
            wrong='Control+k' if os=='macos' else 'Meta+k'
            right='Meta+k' if os=='macos' else 'Control+k'
            page.keyboard.press(wrong)
            assert page.locator('dialog[open]').count()==0
            page.keyboard.press(right)
            assert page.get_by_role('dialog').is_visible()
            page.keyboard.press('Escape')
            assert not errors,(os,errors)
            page.close()
        browser.close()
        print('Platform smoke passed: Windows/macOS/Linux labels, modifier keys, app paths, driver/audio/update feature visibility')


if __name__=='__main__':
    main()
