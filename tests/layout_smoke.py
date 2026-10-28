"""Check responsive card layout, readable labels, and rendered slider fills."""
import base64
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event

VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights',
         'profiles', 'calibration', 'settings', 'diagnostics']


def main():
    with sync_playwright() as p:
        chrome = Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1440, 'height': 1000})
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        mock_core(page)
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function('!!window.controllerStudio && !!window.coreHandler && !!window.studioHooks')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB'})")
        pad_event(page)
        for width in [390, 640, 900, 1280, 1600]:
            page.set_viewport_size({'width': width, 'height': 1000})
            for view in VIEWS:
                page.evaluate('(view) => window.controllerStudio.navigate(view)', view)
                page.wait_for_timeout(100)
                assert page.evaluate('document.documentElement.scrollWidth <= innerWidth + 1'), (width, view, 'horizontal overflow')
                assert page.locator('#command-palette-button').text_content() == 'Ctrl K'
                assert page.locator('#command-palette-button').evaluate("e => ['none', 'normal', '\"\"'].includes(getComputedStyle(e, '::after').content)"), 'Duplicate shortcut label'
                # The text of controls must fit their nearest clipping surface.
                clipped = page.locator(f'#{view}').evaluate('''root => {
                  const bad=[];
                  for(const el of root.querySelectorAll('button,label,.panel-cap span')) {
                    if(!el.getClientRects().length||el.closest('.sr-only'))continue;
                    const range=document.createRange();range.selectNodeContents(el);
                    const r=range.getBoundingClientRect();
                    if(!r.width)continue;
                    for(let parent=el;parent&&parent!==root;parent=parent.parentElement){
                      const css=getComputedStyle(parent);
                      if(css.overflowX!=='hidden'&&css.overflowX!=='clip')continue;
                      const b=parent.getBoundingClientRect();
                      if(r.left<b.left-2||r.right>b.right+2)bad.push(el.textContent.trim().slice(0,80));
                    }
                  }
                  return [...new Set(bad)];
                }''')
                assert not clipped, (width, view, clipped)
                if view == 'calibration' and width >= 900:
                    card = page.locator('.calibration-work').bounding_box()
                    screen = page.locator('#calibration').bounding_box()
                    assert abs(card['width'] - screen['width']) <= 2, 'Calibration leaves unused width'
        page.set_viewport_size({'width': 1440, 'height': 1000})
        page.evaluate("window.controllerStudio.navigate('pc-controls')")
        page.wait_for_timeout(500)
        label_color = page.locator('label[for=pc-click-threshold]').evaluate('e=>getComputedStyle(e).color')
        assert label_color == 'rgb(64, 82, 91)', label_color
        # Check pixels on each side of the thumb, including a programmatic change.
        slider = page.locator('#pc-click-threshold')
        for value in [25, 61, 85]:
            slider.evaluate('(e,value)=>{e.value=value}', value)
            page.wait_for_timeout(120)
            data = base64.b64encode(slider.screenshot()).decode()
            pixels = page.evaluate('''async data => {
              const img=new Image();img.src='data:image/png;base64,'+data;await img.decode();
              const canvas=document.createElement('canvas');canvas.width=img.width;canvas.height=img.height;
              const ctx=canvas.getContext('2d');ctx.drawImage(img,0,0);
              const el=document.querySelector('#pc-click-threshold');
              const ratio=(+el.value-+el.min)/(+el.max-+el.min);
              const center=8+(img.width-16)*ratio;
              return [-14,14].map(offset=>Array.from(ctx.getImageData(Math.round(center+offset),Math.floor(img.height/2),1,1).data).slice(0,3));
            }''', data)
            assert sum(abs(a-b) for a,b in zip(pixels[0], [98,108,156])) < 8, (value, pixels, 'fill misses thumb')
            assert sum(abs(a-b) for a,b in zip(pixels[1], [98,108,156])) > 40, (value, pixels, 'fill extends past thumb')
        assert not errors, errors
        browser.close()
    print('Layout smoke passed: 10 tabs at 5 widths, unclipped control text, calibration width, shortcut label, rendered slider alignment')


if __name__ == '__main__':
    main()
