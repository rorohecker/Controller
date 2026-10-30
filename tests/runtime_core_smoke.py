"""Real stdio core trace/replay/diagnostics checks with all HID discovery disabled."""
import json
import queue
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]


def main():
    exe=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT/'target'/'debug'/('controller.exe' if sys.platform=='win32' else 'controller')
    core=subprocess.Popen([str(exe),'--core','--no-hid'],cwd=ROOT,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
    events=queue.Queue();seen=[]
    def reader():
        for line in core.stdout:events.put(json.loads(line))
    threading.Thread(target=reader,daemon=True).start()
    def send(action,**kwargs):core.stdin.write(json.dumps({'action':action,**kwargs})+'\n');core.stdin.flush()
    def until(match,seconds=5):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            try:event=events.get(timeout=.1)
            except queue.Empty:continue
            seen.append(event)
            if match(event):return event
        raise AssertionError(f'Core timeout: {seen[-5:]}')
    def usb(sequence):
        data=[0]*64;data[0]=1;data[1:5]=[128]*4;data[7]=sequence;data[8]=8;return data
    try:
        send('setRuntimeDiagnostics',enabled=True)
        stats=until(lambda e:e.get('type')=='runtimeStats')
        assert stats['hidEnabled'] is False and stats['softwareOnly'] is True
        assert stats['core']['pid']==core.pid
        baseline=[stats]
        for _ in range(3):baseline.append(until(lambda e:e.get('type')=='runtimeStats'))
        first,last=baseline[0]['core'],baseline[-1]['core'];elapsed=last['sampledAtUs']-first['sampledAtUs']
        resources={'scope':'Debug headless stdio core with HID discovery disabled; includes passive diagnostics and cached process discovery',
          'platform':sys.platform,'seconds':elapsed/1e6,'oneCoreCpuPercent':(last['cpuTimeUs']-first['cpuTimeUs'])/elapsed*100,
          'residentBytes':last['residentBytes'],'lastProcessDiscoveryUs':baseline[-1]['processDiscovery']['durationUs']}
        output=ROOT/'target'/'batch9'/'core-resources.json';output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(resources,indent=2),encoding='utf-8')
        send('scan');until(lambda e:e.get('type')=='device' and 'HID disabled' in e.get('message',''))
        send('setHidRecording',enabled=True);until(lambda e:e.get('type')=='hidRecording' and e['active'])
        send('latencyProbe',id=11);until(lambda e:e.get('type')=='latencyEcho' and e['id']==11)
        send('setHidRecording',enabled=False);until(lambda e:e.get('type')=='hidRecording' and not e['active'])
        send('getHidTrace');trace=until(lambda e:e.get('type')=='hidTrace')['trace']
        assert trace['schemaVersion']==1 and not any(e['kind']=='input' for e in trace['events'])
        bt=bytearray(78);bt[0]=0x31;bt[9]=8
        bt[74:78]=zlib.crc32(bytes([0xa1])+bt[:74]).to_bytes(4,'little');bad=bytearray(bt);bad[4]^=1
        fixture={'schemaVersion':1,'events':[
            {'kind':'connection','atUs':0,'transport':'USB'},
            {'kind':'input','atUs':1000,'bytes':usb(255)},
            {'kind':'input','atUs':2000,'bytes':usb(0)},
            {'kind':'input','atUs':3000,'bytes':usb(1)+[0]},
            {'kind':'readError','atUs':4000},
            {'kind':'connection','atUs':5000,'transport':'Bluetooth'},
            {'kind':'input','atUs':6000,'bytes':list(bt)},
            {'kind':'input','atUs':7000,'bytes':list(bad)},
            {'kind':'input','atUs':8000,'bytes':[1]+[0]*9}]}
        send('inspectHidTrace',json=json.dumps(fixture),requestId=21)
        result=until(lambda e:e.get('type')=='hidReplay' and e['requestId']==21)['report']
        assert result['validReports']==4 and result['invalidReports']==2
        assert result['connections']==2 and result['readErrors']==1 and result['sequenceGaps']==0
        # A full, worst-case-byte trace crosses the former 450 kB stdio cap.
        bulk={'schemaVersion':1,'events':[{'kind':'connection','atUs':0,'transport':'USB'}]}
        for i in range(2047):
            data=[255]*64;data[0]=1;data[7]=i%256
            bulk['events'].append({'kind':'input','atUs':(i+1)*1000,'bytes':data})
        payload=json.dumps(bulk);assert 450_000<len(payload)<1024*1024
        send('inspectHidTrace',json=payload,requestId=25)
        assert until(lambda e:e.get('type')=='hidReplay' and e['requestId']==25)['report']['validReports']==2047
        fixture['output']={'action':'applyTriggers'}
        send('inspectHidTrace',json=json.dumps(fixture),requestId=22)
        assert 'error' in until(lambda e:e.get('type')=='hidReplay' and e['requestId']==22)
        send('inspectHidTrace',json=' '*(1024*1024+1),requestId=23)
        assert '1 MiB' in until(lambda e:e.get('type')=='hidReplay' and e['requestId']==23)['error']
        send('latencyProbe',id=24);until(lambda e:e.get('type')=='latencyEcho' and e['id']==24)
        assert not any(e.get('type')=='device' and e.get('connected') for e in seen)
        assert not any(e.get('type') in ['input','triggerLease','effectTimeline','speakerStatus'] for e in seen)
        send('shutdown');core.wait(timeout=5);assert core.returncode==0
    finally:
        if core.poll() is None:core.terminate();core.wait(timeout=5)
    print('Runtime core smoke passed: HID-disabled core, bounded trace/replay, CRC/reconnect fixtures, passive metrics and stdio round trip')


if __name__=='__main__':main()
