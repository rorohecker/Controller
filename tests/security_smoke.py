"""Exercise the production content policy over HTTP, without native hardware."""
import functools
import http.server
import json
import threading
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event


def main():
    policy=json.loads((ROOT/'tauri.conf.json').read_text(encoding='utf-8'))['app']['security']['csp']
    assert "script-src 'self'" in policy and 'unsafe-eval' not in policy
    class Handler(http.server.SimpleHTTPRequestHandler):
        def end_headers(self):
            self.send_header('Content-Security-Policy',policy)
            super().end_headers()
        def log_message(self,*args):pass
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Handler,directory=str(ROOT/'ui')))
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    try:
        with sync_playwright() as p:
            chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
            browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
            page=browser.new_page();mock_core(page)
            errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
            page.goto(f'http://127.0.0.1:{server.server_port}/index.html')
            page.wait_for_function('!!window.coreHandler&&!!window.outputCalibration')
            page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'security-test'})")
            pad_event(page)
            for view in ['live','triggers','haptics','touch','pc-controls','lights','profiles','calibration','settings','diagnostics']:
                page.evaluate('v=>window.controllerStudio.navigate(v)',view)
                assert page.locator('#'+view).is_visible()
            page.evaluate("""() => {
                window.violations=[];document.addEventListener('securitypolicyviolation',e=>window.violations.push(e.effectiveDirective));
                const script=document.createElement('script');script.textContent='window.injectedScriptRan=true';document.body.append(script);
                const image=document.createElement('img');image.src='data:image/png,invalid';image.setAttribute('onerror','window.injectedHandlerRan=true');document.body.append(image);
            }""")
            page.wait_for_function('window.violations.length>=2')
            assert not page.evaluate('!!window.injectedScriptRan||!!window.injectedHandlerRan')
            assert not errors,errors
            browser.close()
    finally:
        server.shutdown();server.server_close();thread.join(timeout=2)
    print('Security smoke passed: production CSP, all tabs, blocked inline scripts and handlers')


if __name__=='__main__':main()
