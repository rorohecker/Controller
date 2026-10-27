"""Browser smoke for the embedded UI. Run: python tests/ui_smoke.py"""
from pathlib import Path
import io
import math
import struct
import wave
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


def mock_core(page):
    page.add_init_script("""
      window.sent = [];
      window.mockAppSettings={closeToTray:true,startOnLogin:false};
      Object.defineProperty(navigator, 'clipboard', {configurable:true,value:{writeText:async text => {window.copiedDiagnostics=text;}}});
      window.__TAURI__ = {
        core: {invoke: async (name, args) => {
          if (name === 'load_profiles') return null;
          if (name === 'get_app_settings') return window.mockAppSettings;
          if (name === 'set_close_to_tray') { window.mockAppSettings.closeToTray=args.enabled; return null; }
          if (name === 'set_start_on_login') { window.mockAppSettings.startOnLogin=args.enabled; return null; }
          if (name === 'platform_capabilities') return {os:'windows',desktopInput:true,xinput:true,screenColor:true,systemAudioBridge:true};
          if (name === 'check_update') { window.updateChecks=(window.updateChecks||0)+1; return {version:window.mockUpdateVersion||null,currentVersion:'0.14.0'}; }
          if (name === 'install_update') { window.installedUpdate=true; return null; }
          if (name === 'send_demo_telemetry') { window.demoPort=args.port; return null; }
          if (name === 'open_driver_setup') { window.driverOpened=args.kind; return 'Driver setup opened'; }
          if (name === 'save_profiles') window.saved = args.data;
          if (name === 'export_profiles') { window.exported = args.data; return 'CONTROLLER-profiles.json'; }
          if (name === 'send_command') window.sent.push(args.command);
          return null;
        }},
        event: {listen: async (name, callback) => {
          if(name==='core-event')window.coreHandler = payload => callback({payload});
          if(name==='update-progress')window.updateHandler = payload => callback({payload});
        }}
      };
    """)


def pad_event(page, *pressed):
    buttons = [1 if i in pressed else 0 for i in range(23)]
    page.evaluate("""buttons => window.coreHandler({type:'input',input:{
      buttons,axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],battery:75,
      charging:false,fullReport:true}})""", buttons)


def main():
    with sync_playwright() as playwright:
        chrome = Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
        browser = playwright.chromium.launch(
            headless=True, executable_path=str(chrome) if chrome.exists() else None
        )
        page = browser.new_page(viewport={"width": 1440, "height": 900})
        mock_core(page)
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto((ROOT / "ui" / "index.html").as_uri())
        page.wait_for_function("!!window.coreHandler")
        assert page.title() == "CONTROLLER"
        assert page.locator("#clock").count() == 0
        page.evaluate("document.fonts.ready")
        assert page.evaluate("document.fonts.check('400 14px Geist')")
        page.evaluate("window.scrollTo({top:80,behavior:'instant'})")
        assert page.locator(".masthead").bounding_box()["y"] >= 0
        scan_width = page.locator("#scan-button").bounding_box()["width"]
        page.locator("#scan-button").click()
        assert page.locator("#scan-button").inner_text() == "Scan"
        assert page.locator("#scan-button").get_attribute("aria-busy") == "true"
        assert abs(page.locator("#scan-button").bounding_box()["width"] - scan_width) <= 1
        assert page.locator(".masthead").bounding_box()["y"] >= 0
        page.evaluate("window.coreHandler({type:'devices',items:[]})")
        assert page.locator("#scan-button").get_attribute("aria-busy") == "false"
        assert page.evaluate("document.querySelector('.brand strong').getBoundingClientRect().right <= document.querySelector('.sidebar').getBoundingClientRect().right")
        assert page.locator(".photo-stage .controller-map").is_visible()
        assert page.locator(".photo-stage .map-art").count() == 1
        assert page.locator(".photo-stage .map-art").get_attribute("mask") is None
        assert page.locator(".photo-stage .map-front-shoulder").count() == 2
        assert page.locator(".photo-stage .map-shoulder-well").count() == 0
        assert page.locator(".photo-stage .map-shoulder-recess").count() == 2
        assert page.locator('.photo-stage .map-shoulder[data-button="4"] .map-shoulder-motion').get_attribute("clip-path") == "url(#map-left-shoulder-clip)"
        assert page.locator('.photo-stage .map-shoulder[data-button="4"] .map-press-overlay').evaluate("e => e.localName") == "use"
        assert page.locator('.photo-stage .map-shoulder[data-button="4"] .map-press-overlay').evaluate("e => getComputedStyle(e).filter") == "none"
        assert page.locator('#light-controller-map .map-shoulder .map-shoulder-motion').first.get_attribute("clip-path") == "url(#light-map-left-shoulder-clip)"
        assert page.locator("#connection-guide").is_visible()
        assert "No controller connected" in page.locator("#connection-guide").inner_text()
        page.locator('[data-goto="profiles"]').last.click()
        page.locator("#profile-description").fill("Edited while disconnected")
        page.locator("#save-profile").click()
        page.wait_for_function("window.saved?.profiles.some(p=>p.description==='Edited while disconnected')")
        page.locator("#profile-list .profile-row").nth(1).click()
        assert "Connect a controller" not in page.locator("#toast").inner_text()
        page.locator("#profile-list .profile-row").first.click()
        page.locator('[data-view="haptics"]').click()
        assert "PC audio preview" in page.locator("#haptic-route").inner_text()
        page.locator("#play-haptic").click()
        assert not page.evaluate("window.sent.some(c=>c.action==='playHaptic')")
        page.locator('[data-view="diagnostics"]').click()
        page.locator("#check-update").click()
        page.wait_for_function("document.querySelector('#update-status').textContent.includes('Version 0.14.0 is current')")
        assert page.locator("#install-update").is_hidden()
        page.locator('[data-view="settings"]').click()
        assert page.locator('#close-to-tray').is_checked()
        page.locator('#close-to-tray').uncheck()
        page.wait_for_function("window.mockAppSettings.closeToTray===false")
        page.locator('#start-on-login').check()
        page.wait_for_function("window.mockAppSettings.startOnLogin===true")
        page.locator('[data-view="diagnostics"]').click()
        page.evaluate("window.mockUpdateVersion='0.14.0'")
        page.locator("#check-update").click()
        page.wait_for_function("document.querySelector('#install-update').textContent.includes('0.14.0')")
        assert page.locator("#install-update").is_visible()
        page.locator("#install-update").click()
        page.wait_for_function("window.installedUpdate===true")
        page.locator('[data-view="live"]').click()
        assert page.locator("#theme-select").count() == 0
        assert page.locator("body").get_attribute("data-theme") is None
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',path:'test'})")
        assert page.evaluate("window.sent.some(c=>c.action==='setPcBindings')")
        assert page.evaluate("window.sent.some(c=>c.action==='setCalibration')")
        assert not page.locator("#connection-guide").is_visible()
        with io.BytesIO() as sound:
            with wave.open(sound, "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(48_000)
                wav.writeframes(b"".join(struct.pack("<h", int(12_000 * math.sin(2 * math.pi * 100 * i / 48_000))) for i in range(48_000 * 12)))
            page.locator('[data-view="haptics"]').click()
            page.locator('#speaker-file').set_input_files({"name":"tone.wav","mimeType":"audio/wav","buffer":sound.getvalue()})
        page.wait_for_function("document.querySelector('#speaker-time').textContent==='0:00 / 0:12'")
        assert page.locator('#speaker-start').is_enabled()
        page.locator('#speaker-start').fill('1')
        assert page.locator('#speaker-start-value').inner_text() == '0:01'
        page.locator('#speaker-route').select_option('both')
        page.locator('#speaker-file-play').click()
        page.wait_for_function("window.sent.some(c=>c.action==='playSpeakerSound')")
        assert page.evaluate("window.sent.findLast(c=>c.action==='playSpeakerSound').route") == 'both'
        page.evaluate("window.coreHandler({type:'speakerStatus',source:'file',message:'Sound sent to test pad'})")
        page.wait_for_function("document.querySelector('#speaker-progress').value>1")
        page.locator('[data-view="pc-controls"]').click()
        page.wait_for_function("window.sent.filter(c=>c.action==='playSpeakerSound'&&c.append).length>=3",timeout=8000)
        page.locator('[data-view="haptics"]').click()
        page.locator('#speaker-route').select_option('grips')
        page.locator('#speaker-file-play').click()
        assert page.evaluate("window.sent.findLast(c=>c.action==='playSpeakerSound').route==='grips'&&!window.sent.findLast(c=>c.action==='playSpeakerSound').append")
        page.evaluate("window.coreHandler({type:'speakerStatus',source:'file',message:'Playing grips'})")
        page.wait_for_function("window.sent.some(c=>c.action==='playSpeakerSound'&&c.append&&c.route==='grips')")
        page.locator('#speaker-file-play').click()
        assert page.evaluate("window.sent.at(-1).action==='setSpeakerPaused'&&window.sent.at(-1).paused===true")
        page.evaluate("window.coreHandler({type:'speakerStatus',source:'transport',paused:true,position:1,message:'Paused'})")
        assert page.locator('#speaker-file-play').inner_text().lower()=='resume'
        paused_progress=page.locator('#speaker-progress').evaluate('el=>el.value')
        page.wait_for_timeout(500)
        assert page.locator('#speaker-progress').evaluate('el=>el.value')==paused_progress
        page.locator('#speaker-file-play').click()
        assert page.evaluate("window.sent.at(-1).action==='setSpeakerPaused'&&window.sent.at(-1).paused===false")
        page.evaluate("window.coreHandler({type:'speakerStatus',source:'transport',paused:false,position:1,message:'Playing'})")
        page.wait_for_function("document.querySelector('#speaker-progress').value>2")
        page.locator('#speaker-file-stop').click()
        page.evaluate("window.coreHandler({type:'microphoneStatus',muted:true})")
        assert page.locator('#controller-mic').inner_text().lower()=='controller mic: muted'
        page.locator('#controller-mic').click()
        assert page.evaluate("window.sent.at(-1).action==='setMicrophoneMute'&&window.sent.at(-1).enabled===false")
        page.locator('[data-view="pc-controls"]').click()
        assert page.locator("#pc-bindings .pc-binding").count() == 21
        page.locator("#pc-click-threshold").fill("70")
        assert page.evaluate("window.sent.at(-1).click_threshold") == .7
        assert page.locator("#pc-reverse-scroll").is_checked()
        page.locator("#pc-pointer-sensitivity").fill("60")
        assert page.evaluate("window.sent.at(-1).pointer_sensitivity") == .6
        page.locator("#pc-reverse-scroll").uncheck()
        assert page.evaluate("window.sent.at(-1).reverse_scroll") is False
        page.locator('[data-view="touch"]').click()
        page.locator("#setup-vigem").click()
        assert page.evaluate("window.driverOpened==='vigem'")
        page.locator('[data-view="calibration"]').click()
        assert page.locator("#start-calibration").is_enabled()
        page.locator("#start-calibration").click()
        page.wait_for_function("document.querySelector('#calibration-step').textContent.startsWith('1.')")
        assert page.locator("#next-calibration").is_disabled()
        page.evaluate("""() => { for(let i=0;i<35;i++)window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}}); }""")
        assert page.locator("#next-calibration").is_enabled()
        page.locator("#next-calibration").click()
        for axes in ([-1,-1,-1,-1],[1,1,1,1]):
            page.evaluate("""axes => window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes,touch:[null,null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""",axes)
        assert page.locator("#next-calibration").is_enabled()
        page.locator("#next-calibration").click()
        page.evaluate("""() => window.coreHandler({type:'input',input:{buttons:[0,0,0,0,0,0,1,1,...Array(15).fill(0)],axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""")
        assert page.locator("#next-calibration").is_disabled()
        assert page.locator("#cal-l2-adc").inner_text() == "255 / 255"
        pad_event(page)
        assert "255/255" in page.locator("#cal-range-feedback").inner_text()
        assert page.locator("#next-calibration").is_enabled()
        page.locator("#next-calibration").click()
        for x,y in ((.1,.1),(.9,.1),(.1,.9),(.9,.9)):
            for _ in range(6):
                page.evaluate("""point => window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[point,null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""",{"x":x,"y":y})
        assert page.locator("#next-calibration").is_enabled()
        page.locator("#next-calibration").click()
        assert page.locator("#calibration-step").inner_text() == "Calibration saved"
        assert page.evaluate("window.sent.at(-1).action==='setCalibration'")
        page.locator('[data-view="lights"]').click()
        assert page.locator("#light-controller-map .controller-map").count() == 1
        assert page.locator("#light-controller-map .map-art").get_attribute("href") == "./assets/dualsense-front.png"
        assert page.locator("#lights .light-left-column .telemetry-panel").count() == 1
        assert page.locator("#lights > .script-panel").count() == 1
        assert page.locator("#light-presets [data-color]").count() == 24
        for led in (0, 2, 4):
            page.locator(f'#player-leds [data-player-led="{led}"]').click()
        assert page.locator('.map-player-indicators .is-active').count() == 3
        page.locator("#set-player-leds").click()
        assert page.evaluate("window.sent.at(-1).action==='setPlayerLeds' && window.sent.at(-1).mask===21")
        page.locator("#light-hex-input").fill("#42A5F5")
        page.locator("#light-hex-input").press("Tab")
        page.locator("#save-light-color").click()
        assert page.locator("#light-favorites [data-color]").count() == 1
        assert page.locator("#light-hex").inner_text() == "#42A5F5"
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-lightbar-preview.png"), full_page=True)
        page.locator('[data-view="live"]').click()
        assert page.locator("#input-mode-toggle").inner_text().endswith("Game mode")
        page.add_style_tag(content=".controller-map .map-shoulder-motion{transition:none!important}")
        pad_event(page, 4)
        assert page.locator('.photo-stage .map-shoulder[data-button="4"] .map-shoulder-motion').evaluate("e => new DOMMatrixReadOnly(getComputedStyle(e).transform).m42") >= 3
        assert page.locator(".photo-stage .map-shoulder-label").count() == 0
        pad_event(page, 5)
        assert page.locator('.photo-stage .map-shoulder[data-button="5"] .map-shoulder-motion').evaluate("e => new DOMMatrixReadOnly(getComputedStyle(e).transform).m42") >= 3
        pad_event(page)
        pad_event(page, 5)
        assert page.locator("#live").is_visible()
        assert page.locator(".pad-focused").count() == 0
        page.evaluate("""window.coreHandler({type:'input',input:{
          buttons:Array(23).fill(0),axes:[0,0,1,0],touch:[null,null],gyro:[0,0,0],
          battery:75,charging:false,fullReport:true}})""")
        assert page.locator("#activity-scope").get_attribute("aria-label") == "Input activity 25 percent"
        assert page.locator("#right-stick-cap").get_attribute("transform") == "translate(38 0)"
        page.evaluate("""window.coreHandler({type:'input',input:{
          buttons:[0,0,0,0,0,0,.5,.75,...Array(15).fill(0)],axes:[-1,1,.5,-.5],
          touch:[{x:.25,y:.75},null],gyro:[0,0,0],battery:75,
          charging:false,fullReport:true}})""")
        assert page.locator("#left-stick-cap").get_attribute("transform") == "translate(-38 38)"
        assert abs(float(page.locator("#map-l2 .map-trigger-motion").get_attribute("transform").split()[-1][:-1]) - 9) < .01
        assert abs(float(page.locator("#map-r2 .map-trigger-motion").get_attribute("transform").split()[-1][:-1]) - 13.5) < .01
        assert page.locator("#map-r2 .map-press-overlay").get_attribute("x") == "1105"
        assert page.locator("#map-touch-0").get_attribute("class").endswith("is-active")
        page.evaluate("""window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[{x:0,y:1},null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""")
        assert page.locator("#map-touch-0").get_attribute("cx") == "516"
        assert page.locator("#map-touch-0").get_attribute("cy") == "425"
        page.wait_for_timeout(450)
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-input-map.png"), full_page=True)
        page.locator("#input-mode-toggle").click()
        assert page.evaluate("window.sent.at(-1).action") == "setPcMode"
        assert page.evaluate("window.sent.at(-1).enabled") is True
        page.evaluate("window.coreHandler({type:'pcMode',enabled:true})")
        assert page.locator("#input-mode-toggle").get_attribute("aria-pressed") == "true"
        page.evaluate("""window.coreHandler({type:'input',input:{
          buttons:[1,...Array(22).fill(0)],axes:[0.02,-0.01,0.03,0],touch:[null,null],
          gyro:[0,0,0],battery:75,charging:false,
          batteryInfo:{chargeState:'discharging',sensorByte:1},fullReport:true}})""")
        assert "is-pressed" in page.locator('.map-control[data-button="0"]').get_attribute("class")
        assert page.locator("#battery-history .battery-history-bar").count() == 1
        assert "Charge rate available" in page.locator("#battery-charge-rate").inner_text()
        assert page.locator("#diag-temp").inner_text() == "1"
        assert page.evaluate("document.hasFocus()")
        pad_event(page)
        pad_event(page, 5)
        assert page.locator("#triggers").is_visible()
        assert page.locator(".pad-focused").count() == 1
        pad_event(page)
        pad_event(page, 13)
        assert page.locator(".pad-focused").get_attribute("data-side") == "right"
        pad_event(page)
        pad_event(page, 0)
        assert page.locator("#curve-side-label").inner_text() == "R2"
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-trigger-zones.png"), full_page=True)
        for _ in range(40):
            pad_event(page)
            pad_event(page, 13)
            if page.locator('.pad-focused[data-point]').count():
                break
        assert page.locator('.pad-focused[data-point]').count() == 1
        point = page.locator('.pad-focused[data-point]')
        old_cy = float(point.get_attribute('cy'))
        pad_event(page)
        pad_event(page, 12)
        assert float(page.locator('.pad-focused[data-point]').get_attribute('cy')) < old_cy
        pad_event(page)
        pad_event(page, 1)
        assert page.locator("#live").is_visible()
        pad_event(page)
        pad_event(page, 13)
        assert page.locator(".pad-focused").get_attribute("id") == "scan-button"
        pad_event(page)
        pad_event(page, 15)
        assert "session-profile" in page.locator(".pad-focused").get_attribute("class").split()
        page.locator("#input-mode-toggle").click()
        assert page.evaluate("window.sent.at(-1).action") == "setPcMode"
        assert page.evaluate("window.sent.at(-1).enabled") is False
        page.evaluate("window.coreHandler({type:'pcMode',enabled:false})")
        pad_event(page)
        pad_event(page, 5)
        assert page.locator("#live").is_visible()
        assert page.locator(".pad-focused").count() == 0
        page.locator('[data-view="haptics"]').click()
        assert page.locator("#speaker-chirp").is_enabled()
        page.locator("#speaker-chirp").click()
        assert page.evaluate("window.sent.at(-1).action") == "playSpeakerChirp"
        page.locator("#alert-low-battery").click()
        assert page.evaluate("window.sent.at(-1).action") == "setLowBatteryAlert"
        page.evaluate("window.coreHandler({type:'hapticAlertStatus',lowBattery:true})")
        assert page.locator("#alert-low-battery").get_attribute("aria-pressed") == "true"
        page.locator("#alert-timer-minutes").select_option("1")
        page.locator("#alert-timer-start").click()
        assert page.evaluate("window.sent.at(-1).seconds") == 60
        page.evaluate("window.coreHandler({type:'hapticAlertStatus',timerSeconds:60})")
        assert page.locator("#alert-timer-cancel").is_enabled()
        page.locator("#alert-timer-cancel").click()
        assert page.evaluate("window.sent.at(-1).seconds") == 0
        page.evaluate("window.coreHandler({type:'hapticAlertStatus',timerSeconds:0})")
        assert page.locator("#bridge-toggle").is_enabled()
        page.locator("#bridge-toggle").click()
        assert page.evaluate("window.sent.at(-1).action") == "setAudioBridge"
        page.evaluate("window.coreHandler({type:'audioStatus',route:'voiceCoil',bridge:true,message:'Audio bridge on'})")
        assert page.locator("#bridge-toggle").inner_text().lower() == "stop bridge"
        page.evaluate("window.coreHandler({type:'audioStatus',route:'voiceCoil',bridge:true,callbacks:10,glitches:2})")
        assert "2 audio glitches" in page.locator("#bridge-status").inner_text()
        page.evaluate("window.coreHandler({type:'audioStatus',route:'voiceCoil',bridge:true,bridgeFrames:100,glitches:0})")
        assert "PC audio detected" in page.locator("#bridge-status").inner_text()
        page.evaluate("window.coreHandler({type:'audioStatus',route:'voiceCoil',bridge:true,bridgeFrames:100,glitches:0})")
        assert "play audio" in page.locator("#bridge-status").inner_text()
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-haptics-new.png"), full_page=True)
        page.locator('[data-view="lights"]').click()
        page.locator("#temperature-light").click()
        assert page.evaluate("window.sent.at(-1).action") == "setTemperatureLightbar"
        page.evaluate("window.coreHandler({type:'lightMode',battery:false,sensor:true})")
        page.evaluate("window.coreHandler({type:'sensorColor',sensorByte:41,rgb:[140,145,90]})")
        assert "byte 41" in page.locator("#temperature-light-status").inner_text()
        page.evaluate("window.coreHandler({type:'lightMode',battery:false,sensor:false})")
        page.locator("#screen-light").click()
        assert page.evaluate("window.sent.at(-1).action") == "setScreenLightbar"
        page.evaluate("window.coreHandler({type:'lightMode',battery:false,screen:true})")
        page.evaluate("window.coreHandler({type:'screenColor',rgb:[30,90,150]})")
        assert "#1E5A96" in page.locator("#screen-light-status").inner_text()
        page.evaluate("window.coreHandler({type:'lightMode',battery:false,screen:false})")
        page.locator("#telemetry-toggle").click()
        assert page.evaluate("window.sent.at(-1).port") == 5300
        page.evaluate("window.coreHandler({type:'telemetryStatus',enabled:true,port:5300,message:'Listening'})")
        page.locator("#telemetry-lan").check()
        assert page.evaluate("window.sent.at(-1).lan") is True
        page.evaluate("window.coreHandler({type:'telemetry',rpm:6100,maxRpm:8000,speedKmh:108,gear:4,source:'Horizon Dash',revColor:[200,160,30],estimatedBrakeSlip:true})")
        assert page.locator("#race-speed").inner_text() == "108"
        assert page.locator("#race-slip").inner_text().lower() == "brake slip estimated"
        page.locator("#telemetry-format").select_option("json")
        page.locator("#telemetry-demo").click()
        assert page.evaluate("window.demoPort") == 5300
        assert page.locator("#telemetry-json-help").is_visible()
        page.locator("#telemetry-json-help summary").click()
        assert page.locator("#telemetry-json-help code").is_visible()
        assert page.evaluate("window.sent.at(-1).format") == "json"
        page.evaluate("window.coreHandler({type:'telemetryStatus',enabled:true,port:5300,format:'json',message:'Listening'})")
        page.evaluate("window.coreHandler({type:'telemetry',rpm:6200,maxRpm:8000,speedKmh:110,gear:5,source:'Local JSON',revColor:[220,170,30],estimatedBrakeSlip:false})")
        assert page.locator("#race-source").text_content() == "Local JSON"
        page.locator("#script-source").fill('if rpm > 7000 { #{ light: [200, 30, 30] } } else { #{} }')
        page.locator("#script-toggle").click()
        assert page.evaluate("window.sent.at(-1).action") == "setScript"
        page.evaluate("window.coreHandler({type:'scriptStatus',enabled:true,message:'Script armed'})")
        assert page.locator("#script-source").get_attribute("readonly") is not None
        page.evaluate("window.coreHandler({type:'scriptOutput',light:[200,30,30],cue:'pulse'})")
        assert "#C81E1E" in page.locator("#script-output").inner_text()
        page.locator("#script-toggle").click()
        assert page.evaluate("window.sent.at(-1).enabled") is False
        page.evaluate("window.coreHandler({type:'scriptStatus',enabled:false,message:'Script off'})")
        page.evaluate("window.coreHandler({type:'scriptStatus',enabled:true,message:'Script armed'})")
        page.evaluate("window.coreHandler({type:'core',status:'restarting'})")
        assert page.locator("#script-source").get_attribute("readonly") is None
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-telemetry-new.png"), full_page=True)
        page.locator('[data-view="triggers"]').click()
        page.locator('.side-tab[data-side="left"]').click()
        page.locator('#trigger-modes [data-mode="bow"]').click()
        assert page.locator('[data-curve-card="left"]').evaluate("el => el.classList.contains('is-mode-inactive')")
        assert page.locator('#curve-presets button').first.is_disabled()
        assert page.locator(".trigger-curve-svg").count() == 2
        assert page.locator('.trigger-curve-svg[data-curve-side="left"] [data-point]').count() == 10
        page.locator('.side-tab[data-side="right"]').click()
        page.locator('#trigger-modes [data-mode="resistance"]').click()
        right_zone = page.locator('.trigger-curve-svg[data-curve-side="right"] [data-point="5"]')
        before = float(right_zone.get_attribute("cy"))
        right_zone.focus()
        right_zone.press("ArrowUp")
        assert float(page.locator('.trigger-curve-svg[data-curve-side="right"] [data-point="5"]').get_attribute("cy")) < before
        assert page.locator("#curve-side-label").inner_text() == "R2"
        page.locator('#trigger-modes [data-mode="bow"]').click()
        page.locator("#break-start").fill("3")
        page.locator("#break-end").fill("8")
        assert not page.locator("#break-snap-control").is_visible()
        assert page.locator("#break-start-value").inner_text() == "Zone 4"
        assert page.locator("#break-preview-track .is-draw").count() == 5
        page.locator("#apply-triggers").click()
        assert page.evaluate("window.sent.some(c=>c.action==='applyTriggers' && [c.left,c.right].some(t=>t?.mode==='bow'&&t.startZone===3&&t.endZone===8&&t.releasePoint===0.8))")
        page.locator("#break-force-cap").select_option("8")
        page.locator("#zone-cal-start").click()
        page.wait_for_function("window.sent.some(c=>c.action==='applyTriggers' && c.right?.mode==='resistance' && c.right.zoneForces?.[1]===1)")
        for zone in range(1, 10):
            page.evaluate("value => window.coreHandler({type:'input',input:{buttons:Array.from({length:23},(_,i)=>i===7?value:0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],battery:75,charging:false,fullReport:true}})", zone / 10)
            page.locator("#zone-cal-capture").click()
        page.wait_for_function("!!JSON.parse(localStorage.getItem('controller.motorZones')).right")
        assert "is-uncalibrated" not in (page.locator("#break-preview-track").get_attribute("class") or "")
        page.locator('#trigger-modes [data-mode="resistance"]').click()
        page.locator("#click-enabled").check()
        page.locator("#click-threshold").fill("78")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='setInputEffects' && c.threshold===.78)")
        page.locator('[data-weapon-preset="sniper"]').click()
        assert page.locator("#break-preload-number").input_value() == "15"
        assert page.locator("#trigger-force-step").input_value() == "8"
        page.locator("#break-preload-number").fill("30")
        page.locator("#break-preload-number").press("Tab")
        page.locator("#break-start-number").fill("5")
        page.locator("#break-start-number").press("Tab")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='applyTriggers' && c.right.preloadStrength===30 && c.right.startZone===4)")
        page.locator('[data-weapon-preset="burst"]').click()
        assert page.locator("#click-threshold-number").input_value() == "32.9"
        assert page.locator("#burst-spacing").input_value() == "220"
        assert page.locator("#burst-kick-style").input_value() == "strike"
        assert page.locator("#burst-zone-start").input_value() == "2"
        assert page.locator("#burst-grip-strength").input_value() == "100"
        assert page.locator("#burst-grip-frequency").input_value() == "100"
        assert page.locator("#burst-grip-duration").input_value() == "78"
        assert not page.locator("#burst-kick-control").is_visible()
        assert not page.locator("#weapon-rate-row").is_visible()
        assert "5 hz" in page.locator("#burst-style-help").inner_text().lower()
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='setInputEffects' && c.kind==='burst' && Math.abs(c.threshold-.329)<.000001 && c.strength===100 && c.spacingMs===220 && c.startZone===1 && c.hapticStrength===100 && c.hapticFrequency===100 && c.hapticDurationMs===78)")
        page.locator("#click-threshold-number").fill("90.1")
        page.locator("#click-threshold-number").press("Tab")
        page.locator("#burst-grip-strength").fill("92")
        page.locator("#burst-grip-strength").press("Tab")
        page.locator("#burst-grip-frequency").fill("125")
        page.locator("#burst-grip-frequency").press("Tab")
        page.locator("#burst-grip-duration").fill("70")
        page.locator("#burst-grip-duration").press("Tab")
        page.locator("#burst-zone-start").fill("4")
        page.locator("#burst-zone-start").press("Tab")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='setInputEffects' && c.kind==='burst' && Math.abs(c.threshold-.901)<.000001 && c.kickStyle==='strike' && c.startZone===3 && c.hapticStrength===92 && c.hapticFrequency===125 && c.hapticDurationMs===70)")
        page.locator("#burst-kick-style").select_option("return")
        assert page.locator("#burst-kick-control").is_visible()
        page.locator("#burst-return-strength").fill("73")
        page.locator("#burst-return-strength").press("Tab")
        page.locator("#burst-spacing").fill("252")
        page.locator("#burst-spacing").press("Tab")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='setInputEffects' && c.kind==='burst' && c.spacingMs===252 && c.returnStrength===73 && c.kickStyle==='return')")
        page.locator("#burst-spacing").fill("50")
        page.locator("#burst-spacing").press("Tab")
        assert page.locator("#burst-kick").input_value() == "30"
        page.locator('[data-weapon-preset="smg"]').click()
        assert page.locator("#weapon-rate-row").is_visible()
        assert not page.locator("#burst-timing").is_visible()
        page.locator('#trigger-modes [data-mode="vibration"]').click()
        page.locator("#trigger-frequency").fill("17")
        page.locator("#trigger-frequency").press("Tab")
        page.locator("#break-force-cap").select_option("5")
        page.locator("#trigger-strength-input").fill("75")
        page.locator("#trigger-strength-input").press("Tab")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='applyTriggers' && c.right.frequencyHz===17 && c.right.strength===75 && c.right.forceCap===5)")
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-trigger-new.png"), full_page=True)
        page.locator('[data-view="touch"]').click()
        assert "circle" in page.locator("#touch-gesture-toggle").locator("xpath=..//p").inner_text().lower()
        page.locator("#gyro-calibrate").click()
        for _ in range(28):
            page.evaluate("window.coreHandler({type:'input',input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[12,-5,35],battery:75,charging:false,fullReport:true}})")
        page.wait_for_timeout(2300)
        assert "12 / -5 / 35" in page.locator("#gyro-calibration-status").inner_text()
        assert page.evaluate("window.sent.some(c=>c.action==='setGyroAim' && c.bias?.[2]===35)")
        page.locator('[data-view="diagnostics"]').click()
        page.evaluate("window.coreHandler({type:'rate',reportsPerSecond:249,sequenceGaps:2,invalidReports:1})")
        assert page.locator("#diag-rate").inner_text() == "249 / S"
        assert page.locator("#diag-gaps").inner_text() == "2"
        assert page.locator("#diag-invalid").inner_text() == "1"
        page.locator("#copy-diagnostics").click()
        page.wait_for_function("window.copiedDiagnostics?.includes('249/s · gaps 2 · invalid 1')")
        assert 'test' not in page.evaluate("window.copiedDiagnostics"), "Device paths should stay out of copied diagnostics"
        page.locator("#start-drift").click()
        assert page.locator("#start-circularity").is_disabled()
        page.evaluate("window.coreHandler({type:'device',connected:false})")
        assert page.locator("#start-drift").is_disabled()
        page.locator('[data-view="profiles"]').click()
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',path:'test'})")
        page.locator("#profile-name").fill("")
        before_apply = page.evaluate("window.sent.length")
        page.locator("#apply-profile-now").click()
        assert page.evaluate("window.sent.length") == before_apply
        page.locator("#profile-name").fill("Standard")
        page.locator("#profile-description").fill("Export includes this unsaved note")
        page.locator("#export-profiles").click()
        page.wait_for_function("window.exported?.profiles.some(p=>p.description==='Export includes this unsaved note')")
        page.locator("#add-remap").click()
        page.locator("#add-remap").click()
        buttons = page.locator("#remap-bindings select:first-child")
        assert buttons.count() == 2 and buttons.nth(0).input_value() != buttons.nth(1).input_value()
        page.locator(".remap-row .remap-modifier").first.select_option("4")
        page.locator(".remap-row .remap-behavior").first.select_option("macro")
        page.locator(".remap-row .remap-macro").first.fill("Q, R")
        page.locator("#profile-name").click()
        page.wait_for_function("window.saved?.schemaVersion===3 && window.saved.profiles.some(p=>p.remap.bindings.some(b=>b.behavior==='macro'&&b.sequence?.length===2))")
        page.locator("#profile-match").fill("game.exe")
        page.locator("#profile-match-title").fill("Rally")
        page.locator("#profile-match-path").fill(r"C:\Games\game.exe")
        page.locator("#save-profile").click()
        page.wait_for_function("window.saved?.profiles.some(p=>p.matchTitle==='Rally'&&p.matchPath==='C:\\\\Games\\\\game.exe')")
        page.locator("#new-profile").click()
        page.locator("#profile-match").fill("game.exe")
        page.locator("#save-profile").click()
        page.evaluate("window.coreHandler({type:'foreground',exe:'game.exe',title:'Rally 2026',path:'C:\\\\Games\\\\game.exe'})")
        assert page.locator("#profile-match-title").input_value() == "Rally"
        page.evaluate("window.coreHandler({type:'foreground',exe:'game.exe',title:'Another game',path:'C:\\\\Games\\\\game.exe'})")
        assert page.locator("#profile-match-title").input_value() == ""
        page.wait_for_timeout(450)
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-profiles-check.png"), full_page=True)
        for hero, expected_mode in (("hanzo", "bow"), ("genji", "resistance"), ("soldier", "vibration")):
            page.locator(f'[data-hero-template="{hero}"]').click()
            assert page.locator("#profile-auto-apply").is_checked() is False
            assert page.locator("#profile-match").input_value() == ""
            page.wait_for_function("mode => window.saved?.profiles[0]?.triggers.right.mode === mode", arg=expected_mode)
        for view in ("live", "triggers", "haptics", "touch", "pc-controls", "lights", "profiles", "calibration", "diagnostics"):
            page.locator(f'[data-view="{view}"]').click()
            assert page.locator(f"#{view}").is_visible()
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth + 1"), view
        assert not errors, errors

        mobile = browser.new_page(viewport={"width": 390, "height": 844})
        mock_core(mobile)
        mobile_errors = []
        mobile.on("pageerror", lambda error: mobile_errors.append(str(error)))
        mobile.goto((ROOT / "ui" / "index.html").as_uri())
        for view in ("live", "triggers", "haptics", "touch", "pc-controls", "lights", "profiles", "calibration", "diagnostics"):
            mobile.locator(f'[data-view="{view}"]').click()
            assert mobile.locator(f"#{view}").is_visible()
            assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1"), view
        mobile.locator('[data-view="lights"]').click()
        mobile.locator("#telemetry-format").select_option("json")
        mobile.locator("#telemetry-json-help summary").click()
        assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1")
        mobile.locator('[data-view="touch"]').click()
        mobile.wait_for_timeout(450)
        mobile.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "controller-touch-mobile.png"), full_page=True)
        assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1")
        assert not mobile_errors, mobile_errors
        for width, height in ((640, 560), (900, 690), (1060, 690), (1920, 1080)):
            sized = browser.new_page(viewport={"width": width, "height": height})
            mock_core(sized)
            sized.goto((ROOT / "ui" / "index.html").as_uri())
            sized.wait_for_function("!!window.coreHandler")
            scan_before = sized.locator("#scan-button").bounding_box()
            sized.locator("#scan-button").click()
            scan_busy = sized.locator("#scan-button").bounding_box()
            assert abs(scan_busy["x"] - scan_before["x"]) <= 1, width
            assert abs(scan_busy["width"] - scan_before["width"]) <= 1, width
            assert scan_busy["x"] + scan_busy["width"] <= width + 1, width
            sized.evaluate("window.coreHandler({type:'devices',items:[]})")
            if width <= 900:
                assert sized.locator('[data-view="live"] span').last.is_visible()
            views = ("live", "triggers", "haptics", "touch", "pc-controls", "lights", "profiles", "calibration", "diagnostics") if width == 1060 else ("live", "lights", "triggers", "profiles")
            for view in views:
                sized.locator(f'[data-view="{view}"]').click()
                assert sized.evaluate("document.documentElement.scrollWidth <= innerWidth + 1"), (width, view)
                if width == 1060:
                    assert sized.evaluate("""() => [...document.querySelectorAll('main *')].every(e => {
                        const s=getComputedStyle(e);
                        return s.display==='none' || s.visibility==='hidden' || !e.clientWidth || e.scrollWidth<=e.clientWidth+6 || !e.textContent.trim() || ['TEXTAREA','CODE','INPUT'].includes(e.tagName);
                    })"""), view
            if width == 640:
                assert sized.locator(".session-rail").bounding_box()["height"] < 150
            sized.close()
        browser.close()
        print("UI smoke passed: live map, light preview, Game/PC modes, input, battery, profiles, responsive layouts")


if __name__ == "__main__":
    main()
