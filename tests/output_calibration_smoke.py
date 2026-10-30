"""Native calibration ownership, saved/bypassed state and live output UI, without HID."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT, mock_core, pad_event


def main():
    with sync_playwright() as p:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={'width':1180,'height':850})
        mock_core(page)
        errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto((ROOT/'ui/index.html').as_uri())
        page.wait_for_function('!!window.coreHandler && !!window.outputCalibration')
        page.evaluate("""() => {
          localStorage.setItem('controller.deviceCalibration','7');
          const s=window.outputCalibration.defaults();s.calibration.axisCenters=[.1,0,0,0];s.axisNoise=[.02,.01,.03,.01];
          window.recordA={schemaVersion:1,controllerId:'pad-a',revision:1,settings:s};
          window.mockCalibrationRecords={'pad-a':structuredClone(window.recordA)};
          window.coreHandler({type:'device',connected:true,controllerId:'pad-a',name:'DualSense',transport:'USB',outputCalibration:{record:window.recordA}});
        }""")
        pad_event(page)
        page.locator('[data-view="calibration"]').click()
        assert page.locator('#output-calibration').is_visible()
        assert 'revision 1' in page.locator('#output-cal-status').inner_text()
        assert '2.0%' in page.locator('#output-noise-0').inner_text()
        page.evaluate("""() => window.coreHandler({type:'input',controllerId:'pad-a',rawAxes:[.3,.1,0,0],virtualAxes:[.2,0,0,0],rawTriggers:[.5,1],virtualTriggers:[.4,1],virtualOutputActive:true,input:{buttons:Array(23).fill(0),axes:[0,0,0,0],touch:[null,null],gyro:[0,0,0],fullReport:true}})""")
        assert abs(float(page.locator('#output-raw-0').get_attribute('cx'))-74.4)<.01
        assert abs(float(page.locator('#output-game-0').get_attribute('cx'))-69.6)<.01
        assert '50% → 40%' in page.locator('#output-trigger-0').inner_text()
        assert 'Sent to' in page.locator('#output-send-status').inner_text()
        page.evaluate("window.coreHandler({type:'outputInputHealth',controllerId:'pad-a',stale:true})")
        assert page.locator('#output-game-0').get_attribute('cx')=='60'
        assert 'neutral' in page.locator('#output-cal-status').inner_text()
        page.evaluate("window.coreHandler({type:'outputInputHealth',controllerId:'pad-a',stale:false})")
        page.locator('#output-cal-enabled').uncheck()
        page.wait_for_function('window.outputCalibration.record.revision===2')
        assert page.evaluate('window.mockCalibrationRecords["pad-a"].settings.enabled') is False
        page.locator('#output-cal-restore').click()
        page.wait_for_function('window.outputCalibration.record.revision===3')
        assert page.locator('#output-cal-enabled').is_checked()
        # Changing controllers must discard stale samples and never use A's calibration for B.
        page.evaluate("window.coreHandler({type:'device',connected:true,controllerId:'pad-b',name:'DualSense',transport:'USB',outputCalibration:{record:null}})")
        assert not page.evaluate('window.outputCalibration.record')
        page.evaluate("window.coreHandler({type:'device',connected:false});window.coreHandler({type:'device',connected:true,controllerId:'pad-b',name:'DualSense',transport:'USB',outputCalibration:{record:null,error:'Calibration file is invalid'}})")
        assert page.locator('#reset-calibration').is_enabled()
        assert page.locator('#output-cal-enabled').is_disabled()
        page.evaluate("window.coreHandler({type:'outputCalibration',controllerId:'pad-a',record:window.recordA})")
        assert not page.evaluate('window.outputCalibration.record')
        # A pending write is owned by its controller connection, including reconnect to the same ID.
        page.evaluate("""() => {
          const invoke=window.controllerStudio.invoke;
          window.controllerStudio.invoke=async(n,a)=>n==='save_output_calibration'?new Promise(resolve=>window.resolveCalSave=()=>resolve({schemaVersion:1,controllerId:a.controllerId,revision:1,settings:a.settings})):invoke(n,a);
          window.pendingSave=window.outputCalibration.save(window.outputCalibration.defaults().calibration).catch(e=>window.calSaveError=String(e));
          window.coreHandler({type:'device',connected:false});
          window.coreHandler({type:'device',connected:true,controllerId:'pad-b',name:'DualSense',transport:'USB',outputCalibration:{record:null}});
          window.resolveCalSave();
        }""")
        page.wait_for_function('!!window.calSaveError')
        assert 'previous controller' in page.evaluate('window.calSaveError')
        assert not page.evaluate('window.outputCalibration.record')
        for width in (780,1024,1440):
            page.set_viewport_size({'width':width,'height':850})
            assert page.locator('#output-calibration').evaluate('e=>e.scrollWidth<=e.clientWidth+2')
        assert not errors,errors
        browser.close()
    print('Output calibration smoke passed: ownership, recovery, bypass, plots, stale neutral and reconnect races')


if __name__=='__main__':main()
