"""Bridge tuning commands, new audio diagnostics fields, conversion kinds and waveform previews."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core
from browser_paths import chrome_path


def main():
    with sync_playwright() as p:
        chrome = chrome_path()
        browser = p.chromium.launch(headless=True, executable_path=str(chrome) if chrome.exists() else None)
        page = browser.new_page(viewport={'width': 1280, 'height': 900})
        mock_core(page)
        page.add_init_script("""
          localStorage.clear();
          const base=window.__TAURI__.core.invoke;
          window.__TAURI__.core.invoke=async(name,args)=>{
            if(name==='platform_capabilities')return {os:'windows',desktopInput:true,nativeAudioFiles:true,xinput:true,screenColor:true,systemAudioBridge:true};
            if(name==='choose_audio_file')return {path:'C:/music/stereo.wav',name:'Stereo test.wav',duration:30,channels:2,sampleRate:44100};
            if(name==='edge_transactions')return [{id:'1',phase:'verified'},{id:'2',phase:'unreadable',error:'Skipped unreadable Edge transaction record: bad json'}];
            return base(name,args);
          };
        """)
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto((ROOT / 'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler')
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',controllerId:'test-pad'})")
        page.locator('[data-view="haptics"]').click()
        # Bridge tuning
        page.locator('#bridge-advanced').evaluate('d => { d.open = true; }')  # Expand-all already opens it; do not toggle it closed
        page.locator('#bridge-latency').select_option('low')
        assert page.evaluate("window.sent.findLast(c=>c.action==='setBridgeLatency').value") == 'low'
        assert not page.locator('#bridge-custom-controls').is_visible()
        page.locator('#bridge-preset').select_option('custom')
        assert page.locator('#bridge-custom-controls').is_visible()
        sent = page.evaluate("window.sent.findLast(c=>c.action==='setBridgeCustom')")
        assert 60 <= sent['cutoff'] <= 400 and 0 <= sent['gate'] <= 0.2
        page.locator('#bridge-custom-cutoff').fill('220')
        assert page.evaluate("window.sent.findLast(c=>c.action==='setBridgeCustom').cutoff") == 220
        page.locator('#bridge-mode').select_option('threeBand')
        page.locator('#bridge-band-1').fill('0.5')
        opts = page.evaluate("window.sent.findLast(c=>c.action==='setBridgeOptions')")
        assert opts['mode'] == 'threeBand' and opts['bandGains'][1] == 0.5
        # Diagnostics
        page.evaluate("""window.coreHandler({type:'audioMeters',active:true,bridge:true,peaks:[0,.2,.2],captureState:'reattaching',
          captureSource:{kind:'process',pid:1},captureDevice:{name:'Speakers',sampleRate:48000},captureStatus:'running',
          captureInput:{peak:.5,rms:.25},endpointHolder:'controller',keepOpenBy:['bridge','cue'],bluetoothOutput:{dropped:3,underruns:2},
          thermalLimitActive:true,outputRetrySeconds:12,outputFailure:'Device busy',outputUnderruns:4})""")
        assert 'because: the audio bridge, a haptic cue' in page.locator('#audio-endpoint-holder').text_content()
        assert page.locator('#audio-thermal-badge').is_visible()
        assert '12' in page.locator('#audio-output-failure').inner_text()
        page.locator('#audio-output-retry').click()
        assert page.evaluate("window.sent.findLast(c=>c.action==='setAudioBridge').enabled") is True
        assert 'reattaching' in page.locator('#audio-capture-status').text_content() or 'restart' in page.locator('#audio-capture-status').text_content()
        page.locator('.audio-details').first.evaluate('d => { d.open = true; }')
        assert 'Speakers' in page.locator('#audio-capture-device').text_content()
        assert '3 dropped' in page.locator('#audio-bluetooth-output').text_content()
        # Process reattach toggle
        page.locator('#capture-mode').select_option('process')
        page.locator('#capture-reattach').check()
        assert page.evaluate("window.sent.findLast(c=>c.action==='setProcessReattach').enabled") is True
        # Conversion kinds and envelope strip
        page.locator('#speaker-native-file').click()
        page.wait_for_function("document.querySelector('#speaker-file-status').textContent==='Ready'")
        kinds = page.locator('#speaker-content-kind option').evaluate_all('o=>o.map(x=>x.value)')
        assert 'beats' in kinds and any(k.startswith('bands:') for k in kinds), kinds
        page.wait_for_function("window.sent.some(c=>c.action==='previewEnvelope')")
        req = page.evaluate("window.sent.findLast(c=>c.action==='previewEnvelope')")
        assert req['path'] == 'C:/music/stereo.wav' and req['points'] == 256
        page.evaluate("(r)=>window.coreHandler({type:'envelope',id:r.id,path:r.path,kind:r.kind,points:Array.from({length:256},(_, i)=>i/255)})", req)
        page.wait_for_function("document.querySelector('.player-energy-strip').title.startsWith('Energy envelope')")
        page.locator('#speaker-content-kind').select_option('beats')
        page.wait_for_function("window.sent.findLast(c=>c.action==='previewEnvelope').kind==='beats'")
        # Edge journal
        page.locator('[data-view="profiles"]').click()
        page.locator('#edge-journal-refresh').click()
        page.wait_for_function("document.querySelector('#edge-journal-list').children.length===2")
        assert 'skipped unreadable record' in page.locator('#edge-journal-list li').nth(1).inner_text()
        assert not errors, errors
        print('Audio diagnostics smoke passed: bridge tuning, capture diagnostics, retry, reattach, conversion kinds, envelope strip and Edge journal')


if __name__ == '__main__':
    main()
