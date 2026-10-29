"""Controller assignments, Edge backup failures, recognition and async weapon races; no hardware."""
import copy
import json
from pathlib import Path
import struct
import zlib
from playwright.sync_api import sync_playwright
from ui_smoke import ROOT,mock_core


def snapshot():
    slots=[]
    for slot in range(4):
        chunks=[bytearray(64) for _ in range(3)]
        for i,b in enumerate(chunks):b[0]=0x70+slot*3+i
        chunks[0][2]=1;chunks[2][5]=chunks[2][7]=255
        name=f'Slot {slot}'.encode('utf-16-le');chunks[0][6:6+len(name)]=name
        crc=zlib.crc32(chunks[0][2:60]+chunks[1][2:60]+chunks[2][2:56])
        chunks[2][56:60]=struct.pack('<I',crc)
        slots.append([list(b) for b in chunks])
    return {'schemaVersion':1,'controllerId':'pad-edge','firmware':'test','capturedAt':1793220000000,'slots':slots}


def main():
    with sync_playwright() as p:
        chrome=Path(r'C:\Program Files\Google\Chrome\Application\chrome.exe')
        browser=p.chromium.launch(headless=True,executable_path=str(chrome) if chrome.exists() else None)
        page=browser.new_page(viewport={'width':1280,'height':900});mock_core(page)
        page.add_init_script("""const hardwareBase=window.__TAURI__.core.invoke;
        window.hardwareLibrary={schemaVersion:1,controllers:[]};window.hardwareBackups=[];
        window.hardwareEndpoints=[{id:'audio-a',name:'Desk controller'},{id:'audio-b',name:'Sofa controller'}];
        window.__TAURI__.core.invoke=async(n,a)=>{
          if(n==='controller_library')return structuredClone(window.hardwareLibrary);
          if(n==='controller_audio_endpoints')return structuredClone(window.hardwareEndpoints);
          if(n==='save_controller_library'){if(window.failRosterSave)throw Error('Disk full');window.hardwareLibrary=structuredClone(a.data);return null;}
          if(n==='edge_snapshots')return structuredClone(window.hardwareBackups);
          if(n==='store_edge_snapshot'){if(window.failEdgeSave)throw Error('Read-only storage');const id=String(window.hardwareBackups.length+1);window.hardwareBackups.push({id,data:structuredClone(a.data)});return id;}
          if(n==='delete_edge_snapshot'){window.hardwareBackups=window.hardwareBackups.filter(b=>b.id!==a.id);return null;}
          if(n==='inspect_edge_snapshot')return a.data.slots.map((s,slot)=>({slot,assigned:true,revision:1,checksumValid:true,name:'Slot '+slot,leftRange:[s[2][4],s[2][5]],rightRange:[s[2][6],s[2][7]],gripLevel:s[2][8],triggerLevel:s[2][9]}));
          if(n==='edit_edge_snapshot'){window.lastEdgeEdit=structuredClone(a);return{format:'controller-edge-local-draft-v1',hardwareWritten:false,edit:a.edit};}
          const result=await hardwareBase(n,a);
          if(n==='send_command'&&a.command.action==='applyControllerAssignment'&&window.currentHardware){const row=window.hardwareLibrary.controllers.find(c=>c.controllerId===a.command.controllerId);setTimeout(()=>window.coreHandler({...window.currentHardware,name:row.name,assignment:row}),0);}
          if(n==='send_command'&&a.command.action==='applyTriggers'&&window.blockWeapon){window.blockWeapon=false;await new Promise(resolve=>window.releaseWeapon=resolve);}
          return result;
        };""")
        errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto((ROOT/'ui/index.html').as_uri());page.wait_for_function('!!window.coreHandler&&window.hardwareResearch?.loaded')
        device={'type':'device','connected':True,'name':'DualSense','hardwareName':'DualSense','transport':'USB','path':'pad-a-usb','controllerId':'pad-a'}
        page.evaluate('(d)=>{window.currentHardware=d;window.coreHandler(d)}',device)
        page.locator('[data-view="settings"]').click()
        page.locator('#controller-alias').fill('Desk pad');page.locator('#controller-player').select_option('2');page.locator('#controller-audio-endpoint').select_option('audio-a')
        page.evaluate('window.failRosterSave=true');page.locator('#controller-save').click()
        page.wait_for_function("document.querySelector('#controller-library-status').textContent.includes('Disk full')")
        assert not page.evaluate('window.hardwareLibrary.controllers.length')
        page.evaluate('window.failRosterSave=false')
        before=page.evaluate('window.sent.length');page.locator('#controller-save').click()
        page.wait_for_function("document.querySelector('#controller-library-status').textContent.includes('applied')")
        saved=page.evaluate('window.hardwareLibrary.controllers[0]')
        assert saved=={'controllerId':'pad-a','name':'Desk pad','player':2,'endpointId':'audio-a'}
        # Renaming an already identified pad does not resend calibration or rearm game rules.
        assert not page.evaluate("i=>window.sent.slice(i).some(c=>['setInputCalibration','setGameActions'].includes(c.action))",before)
        page.locator('#controller-alias').fill('Unfinished name');page.evaluate('window.hardwareEndpoints=[]');page.locator('#controller-refresh').click()
        page.wait_for_function("!document.querySelector('#controller-refresh').disabled")
        assert page.locator('#controller-alias').input_value()=='Unfinished name'
        assert 'unavailable' in page.locator('#controller-audio-endpoint option:checked').inner_text()
        assert page.locator('#edge-read').is_disabled()
        edge={**device,'name':'DualSense Edge','hardwareName':'DualSense Edge','controllerId':'pad-edge','path':'edge-usb','isEdge':True}
        page.evaluate('(d)=>{window.currentHardware=d;window.coreHandler(d)}',edge)
        page.locator('[data-view="profiles"]').click();page.locator('#edge-read').click()
        request=page.evaluate("window.sent.filter(c=>c.action==='readEdgeProfiles').at(-1)")
        assert request['controllerId']=='pad-edge' and page.locator('#edge-read').is_disabled()
        original=snapshot()
        page.evaluate('(data)=>window.coreHandler({type:"edgeSnapshot",requestId:999,data})',original)
        assert not page.evaluate('window.hardwareBackups.length')
        page.evaluate('window.failEdgeSave=true')
        page.evaluate('x=>window.coreHandler({type:"edgeSnapshot",requestId:x.request,data:x.data})',{'request':request['requestId'],'data':original})
        page.wait_for_function("document.querySelector('#edge-status').textContent.includes('could not be saved')")
        assert not page.evaluate('window.hardwareBackups.length')
        page.evaluate('window.failEdgeSave=false');page.locator('#edge-read').click()
        request=page.evaluate("window.sent.filter(c=>c.action==='readEdgeProfiles').at(-1).requestId")
        page.evaluate('x=>window.coreHandler({type:"edgeSnapshot",requestId:x.request,data:x.data})',{'request':request,'data':original})
        page.wait_for_function("document.querySelector('#edge-status').textContent.includes('Original backup saved')")
        assert page.evaluate('window.hardwareBackups[0].data')==original
        page.locator('#edge-slot').select_option('0');assert page.locator('#edge-export-draft').is_disabled()
        page.locator('#edge-slot').select_option('1');page.locator('#edge-name').fill('Local bow');page.locator('#edge-left-min').fill('8')
        with page.expect_download():page.locator('#edge-export-draft').click()
        assert page.evaluate('window.lastEdgeEdit.edit.name')=='Local bow'
        assert page.evaluate('window.hardwareBackups[0].data')==original
        assert not page.evaluate("window.sent.some(c=>/writeEdge|sendFeature/i.test(c.action))")
        page.locator('#recognition-enabled').check();page.locator('#recognition-hud').check();page.locator('#recognition-threshold').fill('95');page.locator('#recognition-confirmations').fill('3');page.locator('#recognition-save').click()
        page.wait_for_function("window.saved?.profiles.some(p=>p.gameLink.recognition?.threshold===.95)")
        page.locator('#game-link-port').fill('6971');page.locator('#game-link-port').dispatch_event('change')
        assert page.evaluate('window.controllerStudio.profile().gameLink.recognition.confirmations')==3
        assert not page.evaluate('window.ControllerHardwareResearch.validPolicy({threshold:.1})')
        page.evaluate("""()=>{const a=window.controllerStudio,p=a.profile();p.gameLink.enabled=true;p.matchExe='';a.state.autoSuppressed=false;
        const make=(id,strength,kind)=>({id,name:id,triggers:{left:structuredClone(p.triggers.left),right:{...structuredClone(p.triggers.right),strength}},inputEffects:{...structuredClone(p.inputEffects),kind}});
        a.getPresets().push(make('race-a',51,'sniper'),make('race-b',72,'smg'));p.weaponBindings=[{weapon:'sniper',presetId:'race-a'},{weapon:'smg',presetId:'race-b'}];a.renderGameProfiles();}""")
        for _ in range(2):
            page.evaluate("()=>{window.releaseWeapon=null;window.blockWeapon=true;window.weaponPending=window.controllerStudio.useWeapon('sniper',true);}")
            page.wait_for_function('!!window.releaseWeapon')
            page.evaluate("()=>window.controllerStudio.useWeapon('smg',true)")
            page.evaluate('async()=>{window.releaseWeapon();await window.weaponPending}')
            assert page.evaluate("window.sent.filter(c=>c.action==='setInputEffects').at(-1).kind")=='smg'
            assert page.evaluate("window.sent.filter(c=>c.action==='applyTriggers').at(-1).right.strength")==72
        page.evaluate("()=>{window.releaseWeapon=null;window.blockWeapon=true;window.weaponPending=window.controllerStudio.useWeapon('sniper',true);}")
        page.wait_for_function('!!window.releaseWeapon');page.evaluate("()=>window.controllerStudio.send({action:'reset'})")
        reset_index=page.evaluate('window.sent.length')
        page.evaluate('async()=>{window.releaseWeapon();await window.weaponPending}')
        assert not page.evaluate("i=>window.sent.slice(i).some(c=>c.action==='setInputEffects')",reset_index)
        for width in [390,640,900,1280,1600]:
            page.set_viewport_size({'width':width,'height':1000})
            for view in ['settings','profiles','diagnostics']:
                page.locator(f'[data-view="{view}"]').click();page.wait_for_timeout(60)
                assert page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),(width,view)
                assert page.locator('.hardware-card:visible input:visible,.hardware-card:visible select:visible').evaluate_all('(els)=>els.every(e=>{const r=e.getBoundingClientRect(),p=e.closest(".hardware-card").getBoundingClientRect();return r.left>=p.left-1&&r.right<=p.right+1})'),(width,view)
        page.screenshot(path=str(ROOT/'target/hardware-research-check.png'),full_page=True)
        assert not errors,errors
        browser.close()
        print('Hardware research smoke passed: saved assignments, read-only Edge backups, scoped failures, recognition, weapon races and responsive layouts')


if __name__=='__main__':main()
