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
        assert page.locator("#theme-select option").count() == 4
        for palette in ("cinder", "tide", "verdigris", "dusk"):
            page.locator("#theme-select").select_option(palette)
            assert page.locator("body").get_attribute("data-theme") == palette
        page.evaluate("window.coreHandler({type:'device',connected:true,name:'DualSense',transport:'USB',path:'test'})")
        page.evaluate("""window.coreHandler({type:'input',input:{
          buttons:[1,...Array(22).fill(0)],axes:[0.02,-0.01,0.03,0],touch:[null,null],
          gyro:[0,0,0],battery:75,charging:false,fullReport:true}})""")
        assert "is-pressed" in page.locator('.hotspot[data-button="0"]').get_attribute("class")
        assert page.locator("#battery-history .battery-history-bar").count() == 1
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
        page.locator("#add-remap").click()
        page.locator("#add-remap").click()
        buttons = page.locator("#remap-bindings select:first-child")
        assert buttons.count() == 2 and buttons.nth(0).input_value() != buttons.nth(1).input_value()
        page.wait_for_function("!!window.saved")
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
        mobile.locator('[data-view="touch"]').click()
        mobile.wait_for_timeout(450)
        mobile.screenshot(path=str(Path.home() / "AppData" / "Local" / "Temp" / "aftertouch-touch-mobile.png"), full_page=True)
        assert mobile.evaluate("document.documentElement.scrollWidth <= innerWidth + 1")
        assert not mobile_errors, mobile_errors
        browser.close()
        print("UI smoke passed: photograph, palettes, input, report health, battery, stick checks, remap, mobile")


if __name__ == "__main__":
    main()
