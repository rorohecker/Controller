"""Offline tools: waveform validation, timing evidence and conservative HUD/telemetry matching."""
import importlib.util
import json
from pathlib import Path
import random
import struct
import tempfile

ROOT=Path(__file__).resolve().parents[1]


def module(name,path):
    spec=importlib.util.spec_from_file_location(name,ROOT/path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def main():
    evaluation=module('evaluation','scripts/evaluate_haptics.py')
    hud=module('hud','examples/hud_weapon_adapter.py')
    telemetry=module('adapter','examples/weapon_adapter.py')
    rng=random.Random(7);reference=[rng.random() for _ in range(100)]
    alignment=evaluation.onset_alignment(reference,[0]*3+reference[:-3])
    assert alignment['envelopeLagMs']==30 and alignment['envelopeCorrelation']==1
    assert evaluation.onset_alignment([0]*100,[0]*100)['envelopeCorrelation'] is None
    with tempfile.TemporaryDirectory() as temp:
        path=Path(temp)/'sample.wav'
        samples=[0,.25,-.25,0]*500
        pcm=struct.pack('<'+'f'*len(samples),*samples)
        fmt=struct.pack('<HHIIHH',3,1,8000,32000,4,32)
        blob=b'WAVEfmt '+struct.pack('<I',len(fmt))+fmt+b'data'+struct.pack('<I',len(pcm))+pcm
        path.write_bytes(b'RIFF'+struct.pack('<I',len(blob))+blob)
        signal,rate,_=evaluation.read_wav(path)
        assert rate==8000 and signal==samples
        stats=evaluation.signal_stats(signal,rate)
        assert stats['comparisonGain']<=1 and stats['clippedFraction']==0
        path.write_bytes(path.read_bytes()[:-1])
        try:evaluation.read_wav(path)
        except ValueError:pass
        else:raise AssertionError('Truncated WAV accepted')
    values=[0,40,200,255]*128
    vector=hud.features(values)
    assert hud.match(values,[('smg',vector)])[0]=='smg'
    assert hud.match([80]*512,[('smg',vector)]) is None
    assert hud.match(values,[('smg',vector),('pistol',vector)]) is None
    config={'region':{'left':1,'top':1,'width':100,'height':50},'templates':[{'weapon':'smg','file':'example.png'}]}
    assert hud.validate(config)==config['region']
    config['region']['width']=4000
    try:hud.validate(config)
    except ValueError:pass
    else:raise AssertionError('Oversized HUD crop accepted')
    assert telemetry.packet({'weapon':{'id':17}},'weapon.id',{'17':'smg'},'test')['detection']['weapon']=='smg'
    assert telemetry.packet({'weapon':{'id':18}},'weapon.id',{'17':'smg'},'test') is None
    report=json.loads((ROOT/'docs/research/learned-audio-evaluation.json').read_text(encoding='utf-8'))
    assert len(report['pairs'])==4 and all(pair['durationErrorMs']==0 for pair in report['pairs'])
    print('Research tools smoke passed: bounded WAV parsing, lag evidence, blank/ambiguous HUD rejection and explicit telemetry IDs')


if __name__=='__main__':main()
