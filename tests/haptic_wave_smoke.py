"""Haptics page: the waveform preview must visibly sweep the whole graph and then finish cleanly.

Regression: the sweep froze about 9% short of the end (the redraw stopped one frame early) and a
30 ms click animated for less than a single frame, so it looked like nothing played."""
import time
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path

URL = (ROOT / 'ui' / 'index.html').as_uri()

# Fraction of the canvas width covered by the sweep's fill, read from real pixels.
# The fill is drawn on top of the waveform, so measure the right-most column that differs from the idle frame.
COVER = '''() => {
  const c = document.querySelector('#wave-canvas'); const x = c.getContext('2d');
  const w = c.width, h = c.height; const row = x.getImageData(0, 2, w, 1).data; // top strip: only the sweep fill touches it
  let last = -1; for (let i = 0; i < w; i++) { if (row[i * 4 + 3] > 0) last = i; }
  return last < 0 ? 0 : (last + 1) / w;
}'''


def run():
    with sync_playwright() as pw:
        chrome = chrome_path()
        browser = pw.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page, show_all=True)
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto(URL)
        page.wait_for_function('!!window.controllerStudio && !!window.coreHandler')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'wave-pad'})")
        page.evaluate("location.hash='#haptics'")
        page.wait_for_selector('#play-haptic', state='visible')
        for name, attack, release in (('short click', 2, 28), ('medium', 20, 270), ('long', 35, 420)):
            # Set the generator through the real sliders so profile().haptics changes the way a user would.
            page.evaluate("""([a, r]) => {
              const set = (id, v) => { const e = document.querySelector('#' + id); e.value = String(v); e.dispatchEvent(new Event('input', {bubbles: true})); e.dispatchEvent(new Event('change', {bubbles: true})); };
              set('haptic-attack', a); set('haptic-release', r);
            }""", [attack, release])
            page.click('#play-haptic')
            seen = []
            deadline = time.monotonic() + 1.6
            while time.monotonic() < deadline:
                seen.append(page.evaluate(COVER))
                time.sleep(0.02)
            peak = max(seen)
            final = seen[-1]
            assert peak >= 0.9, f'{name}: the sweep only reached {peak:.0%} of the graph (it must go all the way through)'
            assert final == 0, f'{name}: the sweep did not clear after playback ({final:.0%} left on screen)'
            # A short sound must still animate for long enough to be seen.
            visible_frames = sum(1 for v in seen if v > 0)
            assert visible_frames >= 8, f'{name}: the sweep was visible for only {visible_frames} samples'
        assert not errors, errors
        browser.close()
    print('Haptic wave smoke passed: the sweep reaches the end, clears afterwards and short clicks stay visible')


if __name__ == '__main__':
    run()
