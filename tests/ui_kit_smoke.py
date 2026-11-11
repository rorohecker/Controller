"""UI kit: progressive disclosure, persistence, experimental gate, palette coverage, navigation group,
first-view control budgets, the first-run card and "nothing was lost" (every id present at HEAD still exists)."""
import json
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event
import panel_sections_smoke as sections
from browser_paths import chrome_path

URL = (ROOT / 'ui' / 'index.html').as_uri()
VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights', 'profiles', 'calibration', 'settings', 'diagnostics']
NAV_LABELS = ['Controller', 'Triggers', 'Vibration', 'Lights', 'Touch and motion', 'Desktop mode', 'Games and profiles', 'Settings', 'Calibration', 'Diagnostics']
# Visible interactive controls on a page's first view (default preferences, USB controller connected).
BUDGETS = {'live': 8, 'triggers': 55, 'haptics': 52, 'touch': 38, 'pc-controls': 40, 'lights': 48, 'profiles': 40, 'calibration': 12, 'settings': 24, 'diagnostics': 20}
SYNONYMS = ['bluetooth', 'gyro', 'forza', 'audio', 'export', 'weapon', 'theme', 'tour', 'update', 'rumble', 'drift', 'latency', 'xinput', 'battery', 'steam', 'script',
            'macro', 'keyboard', 'overwatch', 'microphone', 'experimental', 'expand', 'backup', 'hidhide']
INTERACTIVE = '''(root) => {
  const sel = 'button, a[href], input:not([type=hidden]), select, textarea, summary, [role=slider][tabindex], [tabindex="0"]';
  const seen = new Set();
  for (const e of root.querySelectorAll(sel)) {
    if (e.closest('.sr-only')) continue;
    if (!e.checkVisibility({visibilityProperty: true})) continue;
    const r = e.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue;
    seen.add(e);
  }
  return seen.size;
}'''


def new_page(browser, width=1280, show_all=False, seed=True, init=None):
    page = browser.new_page(viewport={'width': width, 'height': 900})
    mock_core(page, show_all=show_all, seed=seed)
    if init:
        page.add_init_script(init)
    page.errors = []
    page.on('pageerror', lambda e: page.errors.append(str(e)))
    page.goto(URL)
    page.wait_for_function('!!window.controllerStudio && !!window.coreHandler && !!window.studioHooks && !!window.controllerUI')
    return page


def connect(page, battery=75):
    page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'kit-pad'})")
    page.evaluate("""b=>window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],battery:b,charging:false,fullReport:true}})""", battery)


def go(page, view):
    page.evaluate('v=>window.controllerStudio.navigate(v)', view)
    page.wait_for_timeout(120)


def visible(page, selector):
    return page.evaluate("s=>{const e=document.querySelector(s);return !!e&&e.checkVisibility()&&e.getBoundingClientRect().height>0;}", selector)


def stored(page):
    return json.loads(page.evaluate("localStorage.getItem('controller.ui.v1')") or '{}')


def main():
    with sync_playwright() as p:
        chrome = chrome_path()
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)

        # ---- 1. Kit basics, persistence across reload (no seeding after the first load) ----
        page = new_page(browser, seed=False)
        assert page.evaluate('controllerUI.ready === true')
        go(page, 'triggers')
        wrapper = page.locator('details.ui-section[data-ui-wrap="triggers.bow-weapon"]')
        assert wrapper.count() == 1 and not wrapper.evaluate('d => d.open'), 'advanced sections start closed'
        assert wrapper.locator('> summary .ui-badge').first.inner_text().lower() == 'advanced'
        assert page.evaluate("document.querySelector('[data-ui-section=\"triggers.bow-weapon\"]').parentElement.classList.contains('ui-section-body')"), 'original element is wrapped, not cloned'
        assert page.locator('#break-start').count() == 1 and not visible(page, '#break-start'), 'controls stay in the DOM while collapsed'
        wrapper.locator('> summary').click()
        assert visible(page, '#break-start')
        page.wait_for_timeout(250)  # the toggle event persists asynchronously
        page.reload()
        page.wait_for_function('!!window.controllerUI && !!window.controllerStudio')
        go(page, 'triggers')
        assert page.locator('details.ui-section[data-ui-wrap="triggers.bow-weapon"]').evaluate('d => d.open'), 'open state survives a reload'
        assert stored(page)['open'].get('triggers.bow-weapon') is True
        page.locator('details.ui-section[data-ui-wrap="triggers.bow-weapon"] > summary').click()
        page.wait_for_timeout(250)
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        go(page, 'triggers')
        assert not page.locator('details.ui-section[data-ui-wrap="triggers.bow-weapon"]').evaluate('d => d.open')
        # Basic sections are plain and always open; unknown storage content never breaks the page.
        assert page.evaluate("document.querySelector('[data-ui-section=\"triggers.mode-settings\"]').classList.contains('ui-basic')")
        assert page.evaluate("!document.querySelector('[data-ui-wrap=\"triggers.mode-settings\"]')")
        page.evaluate("localStorage.setItem('controller.ui.v1','{not json')")
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        assert page.evaluate('controllerUI.prefs().showAll === false')
        # Only the selected trigger's curve card is drawn by default.
        go(page, 'triggers')
        assert visible(page, '[data-curve-card="left"]') and not visible(page, '[data-curve-card="right"]')
        assert page.locator('[data-curve-card]').count() == 2

        # ---- 2. Experimental gate ----
        hidden_exp = ['triggers.experimental-modes', 'haptics.bluetooth-audio', 'profiles.recognition', 'diag.wireless-lab', 'diag.native-packet', 'lights.mic-light']
        for sid in hidden_exp:
            assert page.locator(f'[data-ui-section="{sid}"]').count() == 1, sid
            assert page.evaluate(f"getComputedStyle(document.querySelector('[data-ui-section=\"{sid}\"]')).display") == 'none', f'{sid} must be hidden by default'
        assert page.evaluate("document.body.dataset.showExperimental") == 'false'
        go(page, 'settings')
        page.evaluate("controllerUI.openFor('settings.interface')")
        page.locator('#ui-show-experimental').check()
        assert page.evaluate("document.body.dataset.showExperimental") == 'true'
        go(page, 'triggers')
        assert page.locator('details[data-ui-section="triggers.experimental-modes"]').evaluate('d => getComputedStyle(d).display') != 'none'
        assert stored(page)['showExperimental'] is True
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        assert page.evaluate("document.body.dataset.showExperimental") == 'true'
        go(page, 'settings')
        page.evaluate("controllerUI.openFor('settings.interface')")
        assert page.locator('#ui-show-experimental').is_checked(), 'the checkbox reflects the stored preference'
        page.locator('#ui-show-experimental').uncheck()
        assert page.evaluate("getComputedStyle(document.querySelector('[data-ui-section=\"diag.wireless-lab\"]')).display") == 'none'
        # An active experimental feature stays visible, with an "On" badge, even with the gate off.
        go(page, 'triggers')
        page.evaluate("document.querySelector('[data-ui-section=\"triggers.experimental-modes\"]').dataset.uiActive='true'")
        page.wait_for_function("getComputedStyle(document.querySelector('[data-ui-section=\"triggers.experimental-modes\"]')).display!=='none'")
        assert page.locator('details[data-ui-section="triggers.experimental-modes"] > summary .ui-badge-on').is_visible()
        page.evaluate("document.querySelector('[data-ui-section=\"triggers.experimental-modes\"]').dataset.uiActive='false'")
        page.wait_for_function("getComputedStyle(document.querySelector('[data-ui-section=\"triggers.experimental-modes\"]')).display==='none'")
        # openFor reveals hidden, collapsed content, switches page and haptic tab.
        go(page, 'live')
        assert page.evaluate("controllerUI.openFor('diag.wireless-lab')")
        page.wait_for_function("window.controllerStudio.state.currentView==='diagnostics'")
        assert visible(page, '#link-test-start')
        assert page.evaluate("document.querySelector('#nav-tools').open")
        go(page, 'live')
        page.evaluate("controllerUI.openFor('#tactile-name')")
        page.wait_for_function("window.controllerStudio.state.currentView==='haptics'")
        assert visible(page, '#tactile-name'), 'openFor selects the right haptic tab'
        assert page.evaluate("document.querySelector('[data-haptic-tab=studio]').getAttribute('aria-selected')") == 'true'
        page.evaluate("localStorage.removeItem('controller.hapticTab')")
        page.close()

        # ---- 3. Haptics tabs: default "Try it", renamed labels, remembered choice ----
        page = new_page(browser, seed=False)
        go(page, 'haptics')
        labels = page.evaluate("[...document.querySelectorAll('#haptic-tabs [data-haptic-tab]')].filter(b=>!b.hidden).map(b=>b.textContent)")
        assert labels[-1] == 'All' and labels[0] == 'Try it' and 'Music and volume' in labels and 'PC sound to grips' in labels and 'Reminders' in labels, labels
        assert page.evaluate("document.querySelector('#haptic-tabs [aria-selected=true]').dataset.hapticTab") == 'preview'
        assert visible(page, '#play-haptic') and not visible(page, '#speaker-file-play') and not visible(page, '#bridge-toggle')
        assert page.locator('#haptic-strength-note, #haptic-level-note, #haptic-level').count() >= 1, 'haptic strength selector stays on the Try it tab'
        assert page.locator('#wave-shapes button').count() == 8, 'all eight shapes are still offered'
        page.locator('[data-haptic-tab="bridge"]').click()
        assert visible(page, '#bridge-toggle') and not visible(page, '#play-haptic')
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        go(page, 'haptics')
        assert page.evaluate("document.querySelector('#haptic-tabs [aria-selected=true]').dataset.hapticTab") == 'bridge', 'tab choice is remembered'
        page.close()

        # ---- 4. Expand all ----
        page = new_page(browser, seed=False)
        go(page, 'settings')
        page.evaluate("controllerUI.openFor('settings.interface')")
        page.locator('#ui-expand-all').click()
        assert page.evaluate('controllerUI.prefs().showAll') and stored(page)['showAll'] is True
        assert page.evaluate("[...document.querySelectorAll('details.ui-section:not([data-ui-all-open=false])')].every(d=>d.open)"), 'every section opens'
        assert page.evaluate("document.body.dataset.showExperimental") == 'true'
        assert page.evaluate("document.querySelector('#nav-tools').open")
        assert page.evaluate("document.querySelector('#session-status').open")
        assert page.locator('[data-curve-card="right"]').count() == 1
        go(page, 'triggers')
        assert visible(page, '[data-curve-card="right"]') and visible(page, '#break-start') and visible(page, '[data-ui-section="triggers.experimental-modes"]')
        page.evaluate('controllerUI.showAll(false)')
        assert page.evaluate("[...document.querySelectorAll('details.ui-section[data-ui-wrap]')].every(d=>!d.open)")
        page.evaluate("document.querySelector('#haptic-tabs')&&localStorage.removeItem('controller.hapticTab')")
        page.close()

        # ---- 5. Navigation: labels, no index numbers, collapsible Tools group ----
        page = new_page(browser, seed=False)
        labels = page.evaluate("[...document.querySelectorAll('#nav .nav-link')].map(b=>b.textContent.trim())")
        assert sorted(labels) == sorted(NAV_LABELS), labels
        assert not any(l[:1].isdigit() for l in labels), labels
        assert page.locator('.nav-index').count() == 0
        assert not page.evaluate("document.querySelector('#nav-tools').open")
        assert not visible(page, '#nav [data-view="diagnostics"]') and not visible(page, '#nav [data-view="calibration"]')
        assert page.locator('#nav-tools > summary').inner_text().strip().lower() == 'tools and diagnostics'
        page.locator('#nav-tools > summary').click()
        assert visible(page, '#nav [data-view="diagnostics"]')
        assert stored(page)['navTools'] is True
        page.locator('#nav-tools > summary').click()
        assert not page.evaluate("document.querySelector('#nav-tools').open") and stored(page)['navTools'] is False
        go(page, 'diagnostics')
        assert page.evaluate("document.querySelector('#nav-tools').open"), 'the group opens itself for Diagnostics'
        assert page.locator('#nav [data-view="diagnostics"][aria-current="page"]').count() == 1
        go(page, 'calibration')
        assert page.evaluate("document.querySelector('#nav-tools').open")
        go(page, 'live')
        assert not page.evaluate("document.querySelector('#nav-tools').open"), 'auto-opened state is not remembered'
        headings = page.evaluate("VIEWS=>VIEWS.map(v=>document.querySelector('#'+v+' .page-heading p')?.textContent.trim().length>10)", VIEWS)
        assert all(headings), 'every page has a purpose line'
        # Session rail keeps Profile, Using and Stop all; the rest hides in Session status.
        assert visible(page, '.session-profile') and visible(page, '#input-mode-toggle') and visible(page, '#stop-all')
        assert not visible(page, '#session-signal') and not visible(page, '#auto-apply-toggle') and not visible(page, '#quick-reset')
        page.locator('#session-status > summary').click()
        assert visible(page, '#session-signal') and visible(page, '#quick-reset') and 'Preview ends in' in page.locator('.session-lease').text_content()
        page.close()

        # ---- 6. Palette: every tagged section is searchable ----
        page = new_page(browser, seed=False)
        index = page.evaluate('controllerUI.index()')
        tagged = page.evaluate("[...document.querySelectorAll('[data-ui-section]')].map(e=>e.dataset.uiSection)")
        assert len(set(tagged)) == len(tagged), 'duplicate section ids'
        missing = sorted(set(tagged) - {e['id'] for e in index})
        assert not missing, f'sections without an index entry: {missing}'
        assert len(tagged) >= 80, len(tagged)
        for word in SYNONYMS:
            names = sections.palette_results(page, word)
            assert names and 'No matching command.' not in names, f'palette found nothing for "{word}"'
            sections.close_palette(page)
        for entry in index:
            names = sections.palette_results(page, entry['title'])
            assert any(entry['title'].lower() in n.lower() for n in names), f'no palette row for {entry["id"]} ({entry["title"]}): {names[:3]}'
            sections.close_palette(page)
        names = sections.palette_results(page, 'experimental')
        assert any('trigger modes' in n.lower() for n in names)
        page.locator('dialog[open] .studio-list button', has_text='More trigger modes').first.click()
        page.wait_for_function("window.controllerStudio.state.currentView==='triggers'")
        assert visible(page, '#experimental-trigger-mode'), 'choosing a hidden experimental result reveals it'
        page.close()

        # ---- 7. Disabled controls explain themselves, battery chip clears, toasts are capped ----
        page = new_page(browser, seed=False)
        go(page, 'triggers')
        assert page.locator('#apply-triggers').is_disabled()
        note = page.evaluate("(()=>{const b=document.querySelector('#apply-triggers');const n=document.getElementById(b.getAttribute('aria-describedby'));return n&&n.textContent})()")
        assert note and 'controller' in note.lower(), note
        assert page.locator('#triggers .ui-why').count() >= 1
        connect(page)
        assert page.evaluate("!document.querySelector('#apply-triggers').disabled && !document.querySelector('#apply-triggers').getAttribute('aria-describedby')")
        assert page.locator('#side-battery').is_visible()
        page.evaluate("window.coreHandler({type:'device',connected:false})")
        page.wait_for_timeout(100)
        assert not page.locator('#side-battery').is_visible(), 'battery chip clears on disconnect (B4)'
        go(page, 'haptics')
        page.locator('[data-haptic-tab="preview"]').click()
        page.locator('#play-haptic').click()
        assert 'Connect a controller to feel this' in page.locator('#toast').inner_text(), 'Play while disconnected explains itself (B5)'
        for n in range(8):
            page.evaluate('n=>window.controllerStudio.toast("Message number "+n)', n)
        assert page.locator('#toast .toast-item').count() <= 3, 'toast stack is capped'
        page.wait_for_function("document.querySelectorAll('#toast .toast-item').length===0", timeout=9000)
        page.close()

        # ---- 8. First-run card (scripted browsers skip it unless forced) ----
        page = new_page(browser, seed=False)
        assert not visible(page, '#get-started')
        page.close()
        page = new_page(browser, seed=False, init="localStorage.getItem('controller.getStarted.v1')||localStorage.setItem('controller.getStarted.v1','{\"force\":true}')")
        assert visible(page, '#get-started') and page.locator('#get-started li[data-step]').count() == 3
        assert page.locator('#get-started-feel').is_disabled()
        page.locator('#get-started-profiles button').first.click()
        assert page.locator('#get-started li[data-step="profile"].is-done').count() == 1
        connect(page)
        assert page.locator('#get-started li[data-step="connect"].is-done').count() == 1
        page.locator('#get-started-feel').click()
        page.wait_for_function("window.sent.some(c=>c.action==='applyTriggers')")
        page.wait_for_function("document.querySelector('#get-started').hidden", timeout=5000)
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        assert not visible(page, '#get-started'), 'stays hidden once finished'
        page.close()
        page = new_page(browser, seed=False, init="localStorage.getItem('controller.getStarted.v1')||localStorage.setItem('controller.getStarted.v1','{\"force\":true}')")
        page.locator('#get-started-dismiss').click()
        page.reload()
        page.wait_for_function('!!window.controllerUI')
        assert not visible(page, '#get-started')
        page.close()

        # ---- 9. First-view control budgets (default preferences, USB controller connected) ----
        page = new_page(browser, seed=False)
        connect(page)
        counts = {}
        for view in VIEWS:
            go(page, view)
            counts[view] = page.evaluate(f'({INTERACTIVE})(document.querySelector("#{view}"))')
        print('first-view controls:', ', '.join(f'{v}={n}' for v, n in counts.items()))
        over = {v: (n, BUDGETS[v]) for v, n in counts.items() if n > BUDGETS[v]}
        assert not over, f'first view shows too many controls (count, budget): {over}'
        page.close()

        # ---- 10. Nothing lost: every id at HEAD still exists; ids are unique; no page errors ----
        baseline = sections.head_ids(browser)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        current = sections.collect_ids(page, URL)
        if baseline is not None:
            lost = sorted(baseline - current - sections.INTENTIONALLY_GONE)
            assert not lost, f'ids lost since HEAD: {lost}'
        dupes = page.evaluate("""()=>{const c={};for(const e of document.querySelectorAll('[id]'))if(e.id)c[e.id]=(c[e.id]||0)+1;return Object.entries(c).filter(([k,v])=>v>1).map(([k])=>k)}""")
        assert not dupes, f'duplicate ids: {dupes}'
        empty = page.evaluate('document.querySelectorAll(\'[id=""]\').length')
        assert empty == 0, f'{empty} elements render an empty id attribute'
        assert not errors, errors
        # ---- 11. Discoverability, reveal consistency, focus, why(), Expand all overrides ----
        page = new_page(browser, seed=False)
        go(page, 'settings')
        # The experimental switch is a basic, default-open card at the top of Settings, with a one-line reason.
        assert visible(page, '#ui-show-experimental') and visible(page, '#ui-expand-all') and visible(page, '#ui-reset-open')
        assert page.evaluate("document.querySelector('#settings .settings-grid').firstElementChild.dataset.uiSection") == 'settings.experimental'
        assert page.locator('#ui-reset-open').text_content() == 'Reset section layout'
        assert page.locator('#ui-expand-all').text_content() == 'Expand all sections'
        page.locator('#ui-expand-all').click()
        assert page.locator('#ui-expand-all').text_content() == 'Expand all sections' and page.locator('#ui-expand-all').get_attribute('aria-pressed') == 'true', 'label is constant, aria-pressed carries state'
        # A section the user collapses during Expand all stays collapsed and persists once Expand all is off.
        go(page, 'triggers')
        wrap = page.locator('details.ui-section[data-ui-wrap="triggers.bow-weapon"]')
        assert wrap.evaluate('d => d.open')
        wrap.locator('> summary').click()
        page.wait_for_timeout(250)
        assert not wrap.evaluate('d => d.open'), 'the user collapse sticks under Expand all'
        assert stored(page)['open'].get('triggers.bow-weapon') is False
        page.evaluate('controllerUI.showAll(false)')
        assert not wrap.evaluate('d => d.open')
        page.close()

        # Experimental sections are reachable with the gate on (not left behind collapsed wrappers).
        page = new_page(browser, seed=False)
        page.evaluate('controllerUI.showExperimental(true)')
        go(page, 'diagnostics')
        assert visible(page, '#wireless-lab') and visible(page, '#link-test-start'), 'wireless lab shows its controls once experimental is on'
        go(page, 'lights')
        assert visible(page, '#mic-led-mode'), 'microphone light is reachable once experimental is on'
        page.close()
        # The palette/openFor turns the gate on (and says so) instead of forcing a single section.
        page = new_page(browser, seed=False)
        go(page, 'live')
        assert page.evaluate("controllerUI.openFor('lights.mic-light')")
        page.wait_for_function("window.controllerStudio.state.currentView==='lights'")
        assert page.evaluate('controllerUI.prefs().showExperimental') and stored(page)['showExperimental'] is True
        assert 'Experimental features are now shown' in page.locator('#toast').inner_text()
        page.wait_for_function("(()=>{const e=document.querySelector('#mic-led-mode');return !!e&&e.checkVisibility();})()")
        page.close()

        # why() keeps the element's own title and description, and restores them.
        page = new_page(browser, seed=False)
        go(page, 'triggers')
        result = page.evaluate("""()=>{
          const b=document.createElement('button');b.type='button';b.title='Own title';b.setAttribute('aria-describedby','own-desc');
          const d=document.createElement('p');d.id='own-desc';d.textContent='own';
          document.querySelector('#triggers').append(d,b);
          controllerUI.why(b,'Needs something');
          const during=[b.title,b.getAttribute('aria-describedby')];
          controllerUI.why(b,null);
          return {during,after:[b.title,b.getAttribute('aria-describedby')]};
        }""")
        assert result['during'][0] == 'Needs something' and result['during'][1].startswith('own-desc ui-why-'), result
        assert result['after'] == ['Own title', 'own-desc'], result
        page.close()

        # The palette hands focus back to where it was opened, hints at search and shows the page as a trailing label.
        page = new_page(browser, seed=False)
        go(page, 'live')
        page.locator('#command-palette-button').focus()
        page.keyboard.press('Control+k')
        page.wait_for_selector('dialog[open] .palette-search')
        assert page.locator('.palette-search').get_attribute('placeholder') == 'Search pages, settings and actions'
        hint = page.locator('dialog[open] .palette-hint').inner_text()
        assert 'Bluetooth' in hint and 'Ctrl K' in hint, hint
        page.keyboard.press('Escape')
        page.wait_for_function("!document.querySelector('dialog[open]')")
        assert page.evaluate("document.activeElement&&document.activeElement.id") == 'command-palette-button', 'focus returns to the opener'
        page.keyboard.press('Control+k')
        page.locator('.palette-search').fill('gyro')
        row = page.locator('dialog[open] .studio-list button', has=page.locator('.palette-page')).first
        assert row.locator('.palette-page').inner_text().strip(), 'the page is a trailing label'
        page.keyboard.press('Escape')
        page.wait_for_function("!document.querySelector('dialog[open]')")
        # Section rows are searched, never "recent".
        assert page.evaluate("JSON.parse(localStorage.getItem('controller.recentCommands')||'[]').every(n=>!n.includes('\\u00b7'))")
        page.close()

        # First-run card links to Calibration; Bluetooth users get a note instead of nothing.
        page = new_page(browser, seed=False, init="localStorage.getItem('controller.getStarted.v1')||localStorage.setItem('controller.getStarted.v1','{\"force\":true}')")
        page.locator('#get-started-calibrate').click()
        page.wait_for_function("window.controllerStudio.state.currentView==='calibration'")
        page.close()
        page = new_page(browser, seed=False)
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'Bluetooth',controllerId:'bt-pad'})")
        go(page, 'haptics')
        page.locator('[data-haptic-tab="bridge"]').click()
        assert visible(page, '#bluetooth-audio-note') and not visible(page, '#bluetooth-audio-panel')
        page.locator('#bluetooth-audio-show').click()
        page.wait_for_function("(()=>{const e=document.querySelector('#bluetooth-audio-panel');return !!e&&e.checkVisibility();})()")
        assert not visible(page, '#bluetooth-audio-note')
        page.close()

        # Remap opens by itself when the profile already has bindings (only until the user chooses).
        page = new_page(browser, seed=False)
        go(page, 'profiles')
        page.locator('details[data-ui-wrap="profiles.remap"], details[data-ui-section="profiles.remap"]').first.wait_for(state='attached')
        assert not page.locator('details[data-ui-wrap="profiles.remap"], details[data-ui-section="profiles.remap"]').first.evaluate('d => d.open'), 'empty remap stays folded'
        page.evaluate("window.controllerStudio.profile().remap.bindings.push({button:0,modifier:null,key:'KeyA'})")
        page.evaluate("window.controllerStudio.renderAllProfileControls ? window.controllerStudio.renderAllProfileControls() : null")
        page.evaluate("window.controllerStudio.navigate('profiles')")
        page.wait_for_timeout(150)
        opened = page.locator('details[data-ui-wrap="profiles.remap"], details[data-ui-section="profiles.remap"]').first.evaluate('d => d.open')
        assert opened, 'a profile that already remaps buttons shows them'
        page.close()

        # Mutation churn: one second of Play must not cause a flood of observer records or kit rescans.
        page = new_page(browser, seed=False)
        go(page, 'haptics')
        records = page.evaluate("""async()=>{
          let n=0;const o=new MutationObserver(r=>{n+=r.length;});o.observe(document.documentElement,{subtree:true,childList:true,attributes:true,characterData:true});
          document.querySelector('#play-haptic').click();
          await new Promise(r=>setTimeout(r,1000));o.disconnect();return n;}""")
        print('mutation records during 1 s of Play:', records)
        assert records < 200, f'playback causes too many DOM mutations: {records}'
        page.close()

        browser.close()
        print(f'UI kit smoke passed: persistence, experimental gate, expand all, nav group, palette ({len(tagged)} sections), budgets {counts}')


if __name__ == '__main__':
    main()
