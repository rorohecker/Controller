"""Browser smoke for the embedded UI. Run: python tests/ui_smoke.py"""
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


def mock_core(page):
    page.add_init_script("""
      window.sent = [];
      window.__TAURI__ = {
        core: {invoke: async (name, args) => {
          if (name === 'load_profiles') return null;
          if (name === 'save_profiles') window.saved = args.data;
          if (name === 'send_command') window.sent.push(args.command);
          return null;
        }},
        event: {listen: async (name, callback) => {
          window.coreHandler = payload => callback({payload});
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
        assert page.locator(".controller-photo").is_visible()
        assert page.locator("#connection-guide").is_visible()
        assert page.locator("#theme-select option").count() == 4
        for palette in ("cinder", "tide", "verdigris", "dusk"):
            page.locator("#theme-select").select_option(palette)
            assert page.locator("body").get_attribute("data-theme") == palette
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',path:'test'})")
        assert not page.locator("#connection-guide").is_visible()
        page.evaluate("""window.coreHandler({type:'input',input:{
          buttons:[1,...Array(22).fill(0)],axes:[0.02,-0.01,0.03,0],touch:[null,null],
          gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""")
        assert "is-pressed" in page.locator('.hotspot[data-button="0"]').get_attribute("class")
        assert page.locator("#battery-history .battery-history-bar").count() == 1
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
        assert page.locator(".pad-focused").get_attribute("id") == "theme-select"
        pad_event(page)
        pad_event(page, 15)
        assert page.locator("body").get_attribute("data-theme") == "cinder"
        page.locator("#theme-select").select_option("dusk")
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
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-haptics-new.png"), full_page=True)
        page.locator('[data-view="lights"]').click()
        page.locator("#telemetry-toggle").click()
        assert page.evaluate("window.sent.at(-1).port") == 5300
        page.evaluate("window.coreHandler({type:'telemetryStatus',enabled:true,port:5300,message:'Listening'})")
        page.locator("#telemetry-lan").check()
        assert page.evaluate("window.sent.at(-1).lan") is True
        page.evaluate("window.coreHandler({type:'telemetry',rpm:6100,maxRpm:8000,speedKmh:108,gear:4,source:'Horizon Dash',revColor:[200,160,30],estimatedBrakeSlip:true})")
        assert page.locator("#race-speed").inner_text() == "108"
        assert page.locator("#race-slip").inner_text().lower() == "brake slip estimated"
        page.locator("#telemetry-format").select_option("json")
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
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-telemetry-new.png"), full_page=True)
        page.locator('[data-view="triggers"]').click()
        page.locator('[data-mode="bow"]').click()
        page.locator("#apply-triggers").click()
        assert page.evaluate("window.sent.some(c=>c.action==='applyTriggers' && (c.left?.mode==='bow'||c.right?.mode==='bow'))")
        page.locator('[data-mode="resistance"]').click()
        page.locator("#click-enabled").check()
        page.locator("#click-threshold").fill("78")
        page.locator("#apply-triggers").click()
        page.wait_for_function("window.sent.some(c=>c.action==='setInputEffects' && c.threshold===.78)")
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-trigger-new.png"), full_page=True)
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
        page.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-profiles-check.png"), full_page=True)
        for view in ("live", "triggers", "haptics", "touch", "lights", "profiles", "diagnostics"):
            page.locator(f'[data-view="{view}"]').click()
            assert page.locator(f"#{view}").is_visible()
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth + 1"), view
        assert not errors, errors

        mobile = browser.new_page(viewport={"width": 390, "height": 844})
        mock_core(mobile)
        mobile_errors = []
        mobile.on("pageerror", lambda error: mobile_errors.append(str(error)))
        mobile.goto((ROOT / "ui" / "index.html").as_uri())
        for view in ("live", "triggers", "haptics", "touch", "lights", "profiles", "diagnostics"):
            mobile.locator(f'[data-view="{view}"]').click()
            assert mobile.locator(f"#{view}").is_visible()
            assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1"), view
        mobile.locator('[data-view="lights"]').click()
        mobile.locator("#telemetry-format").select_option("json")
        mobile.locator("#telemetry-json-help summary").click()
        assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1")
        mobile.locator('[data-view="touch"]').click()
        mobile.wait_for_timeout(450)
        mobile.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-touch-mobile.png"), full_page=True)
        assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1")
        assert not mobile_errors, mobile_errors
        browser.close()
        print("UI smoke passed: photograph, controller navigation, palettes, input, report health, battery, stick checks, remap, mobile")


if __name__ == "__main__":
    main()
