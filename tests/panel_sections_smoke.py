"""Section tagging for the panel modules, id preservation, command palette coverage and Escape handling."""
import io
import re
import subprocess
import tarfile
import tempfile
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path

VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights', 'profiles', 'calibration', 'settings', 'diagnostics']
LEVELS = {'basic', 'advanced', 'experimental', 'tool'}
# Owner (b) rows of the section registry (SPEC.md appendix), plus the sub-sections the panel modules add themselves.
REGISTRY = {
    'haptics.player': 'basic', 'haptics.player-queue': 'advanced', 'haptics.mixer': 'advanced', 'haptics.mixer-snapshots': 'advanced',
    'haptics.audio-details': 'advanced', 'haptics.capture': 'advanced', 'haptics.bridge': 'basic', 'haptics.bridge-tuning': 'advanced',
    'haptics.bluetooth-audio': 'experimental', 'haptics.alerts': 'basic', 'haptics.workshop': 'basic', 'haptics.library': 'advanced',
    'haptics.tags': 'advanced', 'haptics.personal': 'advanced', 'haptics.conversion': 'tool', 'haptics.evaluation': 'tool',
    'lights.forza': 'tool', 'lights.rules': 'tool',
    'touch.motion': 'tool', 'touch.aiming': 'advanced', 'touch.sticks': 'advanced', 'touch.radial': 'advanced', 'touch.input-save': 'basic',
    'pc.access': 'tool',
    'profiles.session': 'basic', 'profiles.session-history': 'advanced', 'profiles.recovery': 'advanced', 'profiles.steam': 'advanced',
    'profiles.session-settings': 'advanced', 'profiles.compat': 'tool', 'profiles.native': 'tool', 'profiles.edge': 'tool',
    'profiles.edge-journal': 'tool', 'profiles.recognition': 'experimental', 'profiles.shortcuts': 'advanced',
    'calibration.output': 'advanced', 'settings.controllers': 'advanced',
    'diag.drift': 'advanced', 'diag.guided': 'basic', 'diag.wireless-lab': 'experimental', 'diag.native-packet': 'experimental',
    'diag.research': 'tool', 'diag.runtime': 'tool', 'diag.integration': 'tool', 'diag.packet-inspector': 'tool',
    'triggers.encoded': 'tool', 'triggers.gallery': 'tool', 'triggers.guide': 'advanced', 'triggers.toolbar': 'basic',
}
REQUIRED_WORDS = ['bluetooth', 'gyro', 'forza', 'audio', 'export', 'weapon', 'rumble', 'drift', 'latency', 'xinput', 'battery', 'steam', 'script']
SHELL_WORDS = ['theme', 'tour', 'update']  # keywords live on sections the shell tags; reported, not required
# Ids that disappeared for a documented, intentional reason (not by this work).
INTENTIONALLY_GONE = {'display-preferences'}  # studio.js fallback card; daily-use.js now supplies #layout-preferences
DYNAMIC = re.compile(r'^(haptic-section-\d+|dialog-.*|.*[0-9a-f]{8}-[0-9a-f]{4}-.*|forget-.*|.*-why)$')


def collect_ids(page, url):
    page.goto(url)
    page.wait_for_function('!!window.controllerStudio && !!window.coreHandler && !!window.studioHooks')
    page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'ids-pad'})")
    ids = set()
    for view in VIEWS:
        page.evaluate('v=>window.controllerStudio.navigate(v)', view)
        page.wait_for_timeout(60)
        ids.update(page.evaluate("[...document.querySelectorAll('[id]')].map(e=>e.id).filter(Boolean)"))
    return {i for i in ids if not DYNAMIC.match(i)}


def head_ids(browser):
    """Ids rendered by the committed UI, loaded from a temporary export so the working tree is untouched."""
    try:
        data = subprocess.run(['git', 'archive', '--format=tar', 'HEAD', 'ui'], cwd=ROOT, capture_output=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    with tempfile.TemporaryDirectory() as tmp:
        tarfile.open(fileobj=io.BytesIO(data)).extractall(tmp, filter='data')
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        try:
            return collect_ids(page, (Path(tmp) / 'ui' / 'index.html').as_uri())
        finally:
            page.close()


def palette_results(page, query):
    page.keyboard.press('Control+k')
    page.wait_for_selector('dialog[open] .palette-search')
    page.locator('.palette-search').fill(query)
    names = page.evaluate("[...document.querySelectorAll('dialog[open] .studio-list button')].map(b=>b.getAttribute('aria-label')||b.textContent)")
    return names


def close_palette(page):
    page.keyboard.press('Escape')
    page.wait_for_function("!document.querySelector('dialog[open]')")


def main():
    with sync_playwright() as p:
        chrome = chrome_path()
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        baseline = head_ids(browser)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        current = collect_ids(page, (ROOT / 'ui/index.html').as_uri())

        # 1. Every owner-b section exists exactly once with a valid level and title.
        sections = page.evaluate("[...document.querySelectorAll('[data-ui-section]')].map(e=>({id:e.dataset.uiSection,level:e.dataset.level,title:e.dataset.title,hint:e.dataset.hint||''}))")
        by_id = {}
        for s in sections:
            by_id.setdefault(s['id'], []).append(s)
        dupes = {k: len(v) for k, v in by_id.items() if len(v) != 1}
        assert not dupes, f'duplicate section ids: {dupes}'
        for sid, level in REGISTRY.items():
            assert sid in by_id, f'missing section {sid}'
            entry = by_id[sid][0]
            assert entry['level'] in LEVELS, (sid, entry)
            assert entry['level'] == level, f'{sid}: level {entry["level"]} != registry {level}'
            assert entry['title'].strip(), f'{sid}: empty title'
            assert re.fullmatch(r'[a-z]+\.[a-z0-9-]+', sid), sid
        for sid, entries in by_id.items():
            assert entries[0]['level'] in LEVELS and entries[0]['title'].strip(), f'invalid tagging on {sid}'

        # 2. No element id was lost compared with the committed UI.
        if baseline is not None:
            lost = sorted(baseline - current - INTENTIONALLY_GONE)
            assert not lost, f'ids lost since HEAD: {lost}'

        # 3. The palette indexes sections by keyword and finds the listed synonyms.
        page.locator('[data-view="live"]').click()
        for word in REQUIRED_WORDS:
            names = palette_results(page, word)
            assert names and 'No matching command.' not in names, f'palette found nothing for "{word}"'
            close_palette(page)
        missing_shell = []
        for word in SHELL_WORDS:
            names = palette_results(page, word)
            if not names:
                missing_shell.append(word)
            close_palette(page)

        # Page rows use the plain label, never the nav index ("01Input").
        names = palette_results(page, '')
        assert not any(re.match(r'^\d\d', n) for n in names), names
        close_palette(page)

        # 4. Selecting a section row navigates to its page and shows it.
        names = palette_results(page, 'forza')
        target = next(n for n in names if 'Forza' in n)
        page.locator('dialog[open] .studio-list button', has_text=target.split(' \u00b7 ')[0]).first.click()
        page.wait_for_function("window.controllerStudio.state.currentView==='lights'")
        page.wait_for_function("(()=>{const e=document.querySelector('[data-ui-section=\"lights.forza\"]');return !!e&&e.getBoundingClientRect().height>0;})()")

        # 5. Escape closes the palette with a single press, also after typing.
        page.keyboard.press('Control+k')
        page.wait_for_selector('dialog[open] .palette-search')
        page.locator('.palette-search').fill('gyro')
        page.keyboard.press('Escape')
        assert page.evaluate("!document.querySelector('dialog[open]')"), 'Escape needed more than one press'

        # 6. Disabled controls explain themselves while no controller is connected.
        page.evaluate("window.coreHandler({type:'device',connected:false})")
        page.locator('[data-view="diagnostics"]').click()
        page.wait_for_timeout(100)
        assert page.locator('#runtime-ping').is_disabled() and page.locator('#runtime-ping').get_attribute('title'), 'runtime ping must explain why it is off'
        page.locator('[data-view="calibration"]').click()
        page.wait_for_timeout(100)
        assert page.locator('#output-circ-start').is_disabled() and page.locator('#output-circ-start').get_attribute('title')

        assert not errors, errors
        browser.close()
        if missing_shell:
            print(f'Note: shell-owned keywords not indexed yet: {missing_shell}')
        print(f'Panel sections smoke passed: {len(REGISTRY)} sections, {len(current)} ids kept, palette synonyms and Escape')


if __name__ == '__main__':
    main()
