"""Check responsive card layout, readable labels, and rendered slider fills."""
import base64
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event

VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights',
         'profiles', 'calibration', 'settings', 'diagnostics']

# Test line boxes, not just whole element widths. Parent clipping checks miss
# adjacent labels occupying the same space and text crossing into a control.
TEXT_COLLISIONS = '''root => {
  const rows=[],walk=document.createTreeWalker(root,NodeFilter.SHOW_TEXT);
  while(walk.nextNode()) {
    const n=walk.currentNode,e=n.parentElement;
    if(!n.textContent.trim()||!e||e.closest('svg,canvas,textarea,option,.sr-only,.touch-plane,[aria-hidden=true]')||!e.checkVisibility({opacityProperty:true,visibilityProperty:true}))continue;
    const r=document.createRange();r.selectNode(n);
    for(const b of r.getClientRects())if(b.width>3&&b.height>3)rows.push({n,e,b,t:n.textContent.trim().slice(0,60)});
  }
  const bad=[],intersect=(a,b)=>Math.min(a.right,b.right)-Math.max(a.left,b.left)>3&&Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)>3;
  for(let i=0;i<rows.length;i++)for(let j=i+1;j<rows.length;j++)if(rows[i].n!==rows[j].n&&intersect(rows[i].b,rows[j].b))bad.push([rows[i].t,rows[j].t]);
  for(const control of root.querySelectorAll('button,input,select,textarea,summary')) {
    if(control.closest('.touch-plane,.timeline-lane,svg')||!control.checkVisibility({opacityProperty:true,visibilityProperty:true}))continue;
    const box=control.getBoundingClientRect();
    for(const row of rows)if(!control.contains(row.e)&&intersect(box,row.b))bad.push([control.id||control.tagName,row.t]);
  }
  return bad.slice(0,12);
}'''


def main():
    with sync_playwright() as p:
        chrome = Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1440, 'height': 1000})
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        mock_core(page)
        page.emulate_media(reduced_motion='reduce')
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
        for width,zoom in [(640,1),(900,1),(1280,1),(1600,1),(900,1.25),(1280,1.5),(1280,2)]:
            page.set_viewport_size({'width':width,'height':900})
            page.evaluate('z=>document.body.style.zoom=z',zoom)
            for view in VIEWS:
                page.evaluate('v=>window.controllerStudio.navigate(v)',view)
                page.wait_for_timeout(30)
                bad=page.locator('#'+view).evaluate(TEXT_COLLISIONS)
                assert not bad,(width,zoom,view,bad)
                if view=='calibration':
                    guide=page.locator('.calibration-guide').bounding_box()
                    readings=page.locator('.calibration-measurements').bounding_box()
                    layout=page.locator('.calibration-layout').bounding_box()
                    if abs(guide['y']-readings['y'])>4:
                        assert abs(guide['width']-layout['width'])<2,(width,zoom,'unused guide width')
                rail_bad=page.locator('.session-rail').evaluate(TEXT_COLLISIONS)
                assert not rail_bad,(width,zoom,'status rail',rail_bad)
        page.evaluate("window.controllerStudio.navigate('triggers')")
        for preset in ['pistol','sniper','burst','smg']:
            page.locator(f'[data-weapon-preset="{preset}"]').click()
            assert not page.locator('#triggers').evaluate(TEXT_COLLISIONS),preset
        page.locator('#trigger-modes [data-mode="bow"]').click()
        assert not page.locator('#triggers').evaluate(TEXT_COLLISIONS),'bow controls'
        page.evaluate("document.body.style.zoom='1'")
        page.set_viewport_size({'width': 1440, 'height': 1000})
        page.evaluate("window.controllerStudio.navigate('pc-controls')")
        page.wait_for_timeout(500)
        label_color = page.locator('label[for=pc-click-threshold]').evaluate('e=>getComputedStyle(e).color')
        assert label_color == 'rgb(64, 82, 91)', label_color
        # Hide only the thumb paint, preserving its geometry, to inspect the
        # fill boundary within 3 px of its center without sampling its shadow.
        page.add_style_tag(content="""#pc-click-threshold::-webkit-slider-thumb {
          background:transparent!important;border-color:transparent!important;box-shadow:none!important;
        }""")
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
              return [-3,3].map(offset=>Array.from(ctx.getImageData(Math.round(center+offset),Math.floor(img.height/2),1,1).data).slice(0,3));
            }''', data)
            # Permit small material/raster color differences; an unfilled
            # track remains far outside this band. Position is the assertion.
            assert sum(abs(a-b) for a,b in zip(pixels[0], [98,108,156])) < 24, (value, pixels, 'fill misses thumb')
            assert sum(abs(a-b) for a,b in zip(pixels[1], [98,108,156])) > 80, (value, pixels, 'fill extends past thumb')
        assert not errors, errors
        browser.close()
    print('Layout smoke passed: 10 tabs, 5 widths, 100-200% scaling, text/control collisions, weapon/Bow layouts and slider alignment')


if __name__ == '__main__':
    main()
