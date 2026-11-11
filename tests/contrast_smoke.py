"""WCAG contrast gate: every visible text node on every page, in every theme, against its REAL rendered background.

Method: text is made transparent, the page is screenshotted (so gradients, images and stacked panels are
rendered exactly), and each text rect is sampled from that screenshot in a scratch page. Contrast is the
5th-percentile ratio over the sampled pixels. Normal text needs 4.5:1; large text (>=24px, or >=18.66px
bold) and disabled controls need 3:1.

Usage: python tests/contrast_smoke.py [--shots DIR] [--themes light,dark,contrast] [--views live,profiles]
"""
import argparse
import base64
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path

VIEWS = ['live', 'triggers', 'haptics', 'touch', 'pc-controls', 'lights', 'profiles', 'calibration', 'settings', 'diagnostics']
THEMES = ['light', 'dark', 'contrast']
SHOT_VIEWS = ['profiles', 'haptics', 'settings', 'diagnostics']

COLLECT = r"""()=>{
  const out=[],skip=new Set(['SCRIPT','STYLE','NOSCRIPT','TEMPLATE','CANVAS']);
  const parse=c=>{const m=c.match(/[\d.]+/g).map(Number);return [m[0],m[1],m[2],m.length>3?m[3]:1];};
  const sel=el=>{let s=el.tagName.toLowerCase();if(el.id)return s+'#'+el.id;if(el.className&&typeof el.className==='string')s+='.'+el.className.trim().split(/\s+/).slice(0,2).join('.');const p=el.parentElement;if(p){s=(p.id?'#'+p.id:p.tagName.toLowerCase()+(typeof p.className==='string'&&p.className?'.'+p.className.trim().split(/\s+/)[0]:''))+' > '+s;}return s;};
  const H=document.documentElement.scrollHeight;
  function consider(el,text,rect,colorOverride){
    if(rect.width<2||rect.height<2||rect.bottom<0||rect.top>6000)return;
    if(!el.checkVisibility({checkOpacity:true,checkVisibilityCSS:true}))return;
    if(el.closest('[aria-hidden="true"],[inert]'))return;
    const cs=getComputedStyle(el);
    if(cs.visibility!=='visible')return;
    let op=1;for(let n=el;n;n=n.parentElement)op*=parseFloat(getComputedStyle(n).opacity);
    if(op<0.05)return;
    const cx=rect.left+rect.width/2,cy=rect.top+rect.height/2;
    const stack=document.elementsFromPoint(cx,cy);
    /* The topmost hit-testable element must belong to this text, otherwise something is painted over it. */
    const top=stack[0];if(!top||!(top===el||el.contains(top)||top.contains(el)))return;
    let color=colorOverride||cs.color;
    if(!colorOverride&&el instanceof SVGElement){ /* SVG text is painted by fill, not color */
      if(!cs.fill||cs.fill==='none'||cs.fill.startsWith('url'))return;
      color=cs.fill;const fo=parseFloat(cs.fillOpacity);if(!isNaN(fo)&&fo<1){const q=parse(color);color=`rgba(${q[0]},${q[1]},${q[2]},${q[3]*fo})`;}
    }
    const fill=cs.webkitTextFillColor;if(!colorOverride&&!(el instanceof SVGElement)&&fill&&fill!=='rgba(0, 0, 0, 0)'&&fill!==cs.color)color=fill;
    const disabled=!!el.closest(':disabled,[aria-disabled="true"],.is-disabled,fieldset:disabled');
    out.push({text:text.trim().slice(0,50),sel:sel(el),x:rect.left,y:rect.top+scrollY,w:rect.width,h:rect.height,
      fg:parse(color),op,size:parseFloat(cs.fontSize),weight:parseInt(cs.fontWeight)||400,disabled,svg:el instanceof SVGElement,
      sTop:rect.top});
  }
  const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);
  for(let n;n=walker.nextNode();){
    const el=n.parentElement;if(!el||skip.has(el.tagName)||!n.nodeValue.trim())continue;
    if(el.closest('script,style,option'))continue;
    const r=document.createRange();r.selectNodeContents(n);
    const rects=[...r.getClientRects()].filter(q=>q.width>1&&q.height>1);
    if(!rects.length)continue;
    const b=r.getBoundingClientRect();
    consider(el,n.nodeValue,b);
  }
  for(const el of document.querySelectorAll('input,textarea')){
    if(['checkbox','radio','range','color','file','hidden','button','submit'].includes(el.type))continue;
    const q=getComputedStyle(el),bb=r0=>parseFloat(r0)||0;
    const b0=el.getBoundingClientRect();
    /* Sample only the text box: border and padding can be a different colour from the field fill. */
    const il=bb(q.borderLeftWidth)+bb(q.paddingLeft),it=bb(q.borderTopWidth)+bb(q.paddingTop);
    const r={left:b0.left+il+1,top:b0.top+it+1,width:Math.max(0,b0.width-2*il-2),height:Math.max(0,b0.height-2*it-2),bottom:b0.bottom-it-1};
    if(el.value){consider(el,el.value,r);}
    else if(el.placeholder){const ph=getComputedStyle(el,'::placeholder');const c=parse(ph.color);const o=parseFloat(ph.opacity);
      consider(el,el.placeholder,r,`rgba(${c[0]},${c[1]},${c[2]},${c[3]*(isNaN(o)?1:o)})`);}
  }
  return out;
}"""

HIDE_CSS = ("*,*::before,*::after,*::placeholder,::placeholder{color:transparent!important;-webkit-text-fill-color:transparent!important;"
            "text-shadow:none!important;caret-color:transparent!important;text-decoration-color:transparent!important}"
            "select{color:transparent!important}text,tspan{fill:transparent!important;stroke:none!important}")

SAMPLE = r"""async ({b64,items})=>{
  const img=new Image();img.src='data:image/png;base64,'+b64;await img.decode();
  const c=document.createElement('canvas');c.width=img.width;c.height=img.height;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(img,0,0);const D=g.getImageData(0,0,img.width,img.height).data;
  const f=v=>{v/=255;return v<=.03928?v/12.92:((v+.055)/1.055)**2.4};
  const L=(r,gg,b)=>.2126*f(r)+.7152*f(gg)+.0722*f(b);
  return items.map(it=>{
    const x0=Math.max(0,Math.floor(it.x)),y0=Math.max(0,Math.floor(it.y)),w=Math.min(img.width-x0,Math.max(1,Math.ceil(it.w))),h=Math.min(img.height-y0,Math.max(1,Math.ceil(it.h)));
    let worst=null;const padY=h>=14?2:(h>6?1:0),pad=w>10?2:(w>4?1:0),nx=Math.min(Math.max(1,w-2*pad),24),ny=Math.min(Math.max(1,h-2*padY),8),ratios=[];let bgSum=[0,0,0];
    const a=it.fg[3]*it.op;
    for(let i=0;i<nx;i++)for(let j=0;j<ny;j++){
      const o=((y0+padY+Math.floor((j+.5)*(h-2*padY)/ny))*img.width+x0+pad+Math.floor((i+.5)*(w-2*pad)/nx))*4;const px=[D[o],D[o+1],D[o+2]];
      const rr=a*it.fg[0]+(1-a)*px[0],gg=a*it.fg[1]+(1-a)*px[1],bb=a*it.fg[2]+(1-a)*px[2];
      const l1=L(rr,gg,bb),l2=L(px[0],px[1],px[2]);const r=(Math.max(l1,l2)+.05)/(Math.min(l1,l2)+.05);
      ratios.push(r);if(!worst||r<worst[0])worst=[r,px[0],px[1],px[2],x0+pad+Math.floor((i+.5)*(w-2*pad)/nx),y0+padY+Math.floor((j+.5)*(h-2*padY)/ny)];bgSum=bgSum.map((v,k)=>v+px[k]);
    }
    ratios.sort((p,q)=>p-q);
    /* SVG chart labels deliberately overlap plot lines, so they use the median; everything else the 5th percentile. */
    const ratio=ratios[Math.floor(ratios.length*(it.svg?.5:.05))];
    return {ratio,bg:bgSum.map(v=>Math.round(v/ratios.length)),worst};
  });
}"""


def fit_viewport(page):
    for _ in range(3):
        h = page.evaluate("Math.min(6000,Math.max(900,document.documentElement.scrollHeight))")
        page.set_viewport_size({"width": 1280, "height": int(h)})
        page.wait_for_timeout(120)


def full_shot(page):
    fit_viewport(page)
    return page.screenshot()


def audit(page, scratch, label, shot_path=None):
    """Return a list of failures for the current page state."""
    fit_viewport(page)
    items = page.evaluate(COLLECT)
    if shot_path:
        Path(shot_path).parent.mkdir(parents=True, exist_ok=True)
        page.screenshot(path=str(shot_path))
    style = page.add_style_tag(content=HIDE_CSS)
    page.wait_for_timeout(60)
    png = page.screenshot()
    page.evaluate("el=>el.remove()", style)
    res = scratch.evaluate(SAMPLE, {"b64": base64.b64encode(png).decode(), "items": items})
    fails, seen = [], set()
    for it, r in zip(items, res):
        large = it['size'] >= 24 or (it['size'] >= 18.66 and it['weight'] >= 700)
        need = 3.0 if (large or it['disabled']) else 4.5
        if r['ratio'] + 0.005 < need:
            key = (it['sel'], it['text'])
            if key in seen:
                continue
            seen.add(key)
            fails.append(f"{label} | {r['ratio']:.2f}<{need} | {it['sel']} | '{it['text']}' | fg={it['fg']} bg={r['bg']} worst={[round(v) for v in r['worst'][1:]]} {it['size']}px/{it['weight']}")
    return fails, len(items)


def open_details(page):
    # The session bar popover floats over the page; it is audited on its own below.
    page.evaluate("document.querySelectorAll('details:not(.daily-bar details)').forEach(d=>d.open=true)")


def states_for(page, view):
    """Yield (name) after putting the visible page into each sub-state (tabs, open details)."""
    page.locator(f'[data-view="{view}"]').click()
    page.wait_for_timeout(120)
    open_details(page)
    yield 'base'
    tabs = page.locator(f'#{view} [role=tab]:visible')
    for i in range(tabs.count()):
        try:
            tabs.nth(i).click(timeout=1500)
        except Exception:
            continue
        page.wait_for_timeout(80)
        open_details(page)
        yield f'tab{i}'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--shots')
    ap.add_argument('--shot-views', default=','.join(SHOT_VIEWS))
    ap.add_argument('--themes', default=','.join(THEMES))
    ap.add_argument('--views', default=','.join(VIEWS))
    args = ap.parse_args()
    themes, views = args.themes.split(','), args.views.split(',')
    all_fails, checked = [], 0
    with sync_playwright() as p:
        chrome = chrome_path()
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        ctx = browser.new_context(viewport={"width": 1280, "height": 900})
        page = ctx.new_page()
        scratch = ctx.new_page()
        scratch.goto('about:blank')
        mock_core(page)
        page.add_init_script("localStorage.clear()")
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function("!!window.coreHandler")
        page.evaluate("document.fonts.ready")
        # Freeze motion so colour transitions never leave a half-faded sample.
        page.add_style_tag(content="*,*::before,*::after{transition:none!important;animation:none!important;scroll-behavior:auto!important}")
        page.evaluate("""window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'pad-a'})""")

        page.wait_for_function("document.querySelectorAll('.toast-item').length===0", timeout=15000)

        # System theme must resolve through prefers-color-scheme to the same tokens.
        for scheme in ('light', 'dark'):
            page.emulate_media(color_scheme=scheme)
            page.evaluate("window.controllerTheme.set('auto')")
            assert page.evaluate("document.documentElement.dataset.theme") == scheme, scheme
        page.emulate_media(color_scheme='light')

        for theme in themes:
            page.evaluate(f"window.controllerTheme.set('{theme}')")
            page.wait_for_timeout(100)
            for view in views:
                for state in states_for(page, view):
                    shot = None
                    if args.shots and state == 'base' and view in args.shot_views.split(','):
                        shot = Path(args.shots) / f'{view}-{theme}.png'
                    f, n = audit(page, scratch, f'{theme}/{view}/{state}', shot)
                    all_fails += f
                    checked += n
            # Overlays: session bar popover, command palette, toast, tour.
            page.locator('[data-view="live"]').click()
            page.evaluate("document.querySelectorAll('.daily-bar details').forEach(d=>d.open=true)")
            f, n = audit(page, scratch, f'{theme}/daily-popover'); all_fails += f; checked += n
            page.evaluate("document.querySelectorAll('.daily-bar details').forEach(d=>d.open=false)")
            page.locator('[data-view="settings"]').click()
            page.keyboard.press('Control+k')
            page.wait_for_timeout(150)
            f, n = audit(page, scratch, f'{theme}/palette'); all_fails += f; checked += n
            page.keyboard.press('Escape')
            page.evaluate("window.controllerStudio.undoToast('Removed thing.',()=>{})")
            page.wait_for_timeout(150)
            f, n = audit(page, scratch, f'{theme}/toast'); all_fails += f; checked += n
            if page.locator('#replay-tour').count():
                page.locator('#replay-tour').click()
                page.wait_for_timeout(150)
                f, n = audit(page, scratch, f'{theme}/tour'); all_fails += f; checked += n
                page.keyboard.press('Escape')
                if page.locator('.tour').count():
                    page.get_by_role('button', name='Skip tour').click()
        browser.close()
    # De-duplicate across tab states (same element reported repeatedly).
    uniq = []
    seen = set()
    for line in all_fails:
        key = tuple(line.split(' | ')[0].split('/')[:1] + line.split(' | ')[2:4])
        if key in seen:
            continue
        seen.add(key)
        uniq.append(line)
    print(f'contrast: checked {checked} text nodes, {len(uniq)} below threshold')
    for line in uniq:
        print('  FAIL', line.encode('ascii', 'replace').decode())
    return 1 if uniq else 0


if __name__ == '__main__':
    raise SystemExit(main())
