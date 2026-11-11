"""Vibration > Try it: overall intensity vs per-vibration strength, and examples vs sliders."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path

URL = (ROOT / 'ui' / 'index.html').as_uri()


def sent(page, action):
    return page.evaluate("(a)=>window.sent.filter(c=>c.action===a)", action)


def setup(pw, connected):
    chrome = chrome_path()
    browser = pw.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
    page = browser.new_page(viewport={'width': 1280, 'height': 900})
    mock_core(page, show_all=True)
    page.errors = []
    page.on('pageerror', lambda e: page.errors.append(str(e)))
    page.goto(URL)
    page.wait_for_function('!!window.controllerStudio && !!window.coreHandler')
    if connected:
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'ux-pad'})")
    page.evaluate("location.hash='#haptics'")
    page.wait_for_selector('#play-haptic', state='visible')
    return browser, page


def run():
    with sync_playwright() as pw:
        browser, page = setup(pw, True)
        text = page.evaluate("document.querySelector('#haptics').textContent")
        assert 'Overall intensity' in text and 'Strength (this effect)'.lower() in text.lower()
        assert 'Haptic strength' not in text, 'old label must be gone'
        assert page.locator('#haptic-level').get_attribute('aria-label') == 'Overall intensity'
        assert page.locator('label[for=haptic-strength]').inner_text().lower().startswith('strength (this effect)')
        assert page.locator('#play-haptic').text_content() == 'Play vibration'
        # Normal is the default: the shipped behaviour, so existing users feel nothing change.
        assert page.locator('#haptic-level [aria-pressed=true]').get_attribute('data-level') == 'normal'
        labels = page.evaluate("[...document.querySelectorAll('#haptic-level [data-level]')].map(b=>b.getAttribute('aria-label'))")
        assert labels == ['Soft', 'Normal', 'Strong', 'Max'], labels
        assert 'Strong and Max are more intense and can warm the grips on long effects' in text
        levels = page.evaluate("[...document.querySelectorAll('#haptic-level [data-level]')].map(b=>b.dataset.level)")
        assert levels == ['soft', 'normal', 'strong', 'max'], levels
        assert 'Master limit for everything the grips can do' in text
        assert 'Inside the overall intensity' in text
        assert page.locator('#haptic-based-label').text_content() == 'Custom'
        assert page.locator('#haptic-reset-preset').is_disabled() and not page.locator('#haptic-edited-badge').is_visible()
        # Controls (Play + intensity) are within the first 900 px.
        for sel in ('#play-haptic', '#haptic-level [data-level=max]'):
            box = page.locator(sel).bounding_box()
            assert box and box['y'] + box['height'] <= 900, f'{sel} below the fold: {box}'
        # Preset fills the sliders and names itself.
        page.click('#texture-list [data-texture="2"]')
        assert page.locator('#haptic-based-label').text_content() == 'Based on: Heartbeat'
        assert page.input_value('#haptic-frequency') == '55' and page.input_value('#haptic-strength') == '70'
        assert 'Load' not in page.locator('#texture-list').text_content().replace('loads', '')
        assert 'Plays and loads it below' not in page.locator('#texture-list').text_content(), 'said once in the caption, not on every card'
        assert page.locator('#haptic-presets-panel .panel-cap').text_content().count('Plays and loads it below') == 1
        assert page.locator('#haptic-reset-preset').is_disabled()
        assert 'Heartbeat' in page.locator('#wave-formula').text_content()
        # Editing marks it Edited and enables Reset.
        page.evaluate("const e=document.querySelector('#haptic-frequency');e.value=80;e.dispatchEvent(new Event('input',{bubbles:true}))")
        assert page.locator('#haptic-edited-badge').is_visible()
        assert page.locator('#haptic-reset-preset').is_enabled()
        assert page.locator('#haptic-reset-preset').text_content() == 'Reset to Heartbeat'
        page.click('#haptic-reset-preset')
        assert page.input_value('#haptic-frequency') == '55'
        assert not page.locator('#haptic-edited-badge').is_visible() and page.locator('#haptic-reset-preset').is_disabled()
        # A shape change also counts as an edit.
        page.click('#wave-shapes [data-shape=sine]')
        assert page.locator('#haptic-edited-badge').is_visible()
        # Level click: setHapticOutputLevel, then the fixed reference pulse (connected).
        page.evaluate("window.sent.length=0")
        page.click('#haptic-level [data-level=max]')
        page.wait_for_function("window.sent.some(c=>c.action==='playHaptic')")
        order = page.evaluate("window.sent.map(c=>c.action)")
        assert order.index('setHapticOutputLevel') < order.index('playHaptic'), order
        assert sent(page, 'setHapticOutputLevel')[0]['level'] == 'max'
        pulse = sent(page, 'playHaptic')[0]
        assert (pulse['shape'], pulse['frequency'], pulse['strength']) == ('sine', 90, 70), pulse
        page.click('#haptic-level [data-level=soft]')
        page.wait_for_function("window.sent.filter(c=>c.action==='playHaptic').length===2")
        assert [c['level'] for c in sent(page, 'setHapticOutputLevel')] == ['max', 'soft']
        assert page.locator('#haptic-level [data-level=soft]').get_attribute('aria-pressed') == 'true'
        assert not page.errors, page.errors
        browser.close()
        # Disconnected: the level is remembered but nothing is sent or played.
        browser, page = setup(pw, False)
        page.click('#haptic-level [data-level=strong]')
        page.wait_for_timeout(300)
        assert not sent(page, 'setHapticOutputLevel') and not sent(page, 'playHaptic')
        assert page.locator('#haptic-level [data-level=strong]').get_attribute('aria-pressed') == 'true'
        assert 'Saved. It applies when a controller connects.' in page.locator('#toast').inner_text()
        assert not page.errors, page.errors
        browser.close()
    print('Haptic UX smoke passed: intensity vs strength labels, example/Edited/Reset flow, level reference pulse')


if __name__ == '__main__':
    run()
