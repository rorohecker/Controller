"""Reduce locally produced REA metadata to source-labeled interoperability facts, without binary assets or code."""
import argparse
import json
from pathlib import Path


def summarize(data):
    x=data['normalized_result']
    names=['DualSenseX.DuelSense_Base_Updated','DualSenseX.DualSenseXSaveFile','DualSenseX.Main']
    methods={m['token']:m for m in x['methods']}
    facts=[]
    for edge in x['call_edges']:
        owner=edge.get('target_declaring_type','');name=edge.get('target_name','');caller=edge.get('caller','')
        relevant=(owner=='System.Threading.Thread' and name in ['Start','Sleep'] and 'DuelSense_Base_Updated' in caller
                  or owner.startswith('NAudio.') and name in ['.ctor','Init','Play','StartRecording'] and 'AudioHaptics' in caller
                  or owner=='Newtonsoft.Json.JsonConvert' and name in ['SerializeObject','DeserializeObject'] and 'DualSenseX.' in caller)
        if not relevant:continue
        method=methods.get(edge['caller_token'],{})
        constant=None
        anchors=method.get('body',{}).get('anchors',[])
        prior=[a for a in anchors if a['il_offset']<edge['il_offset']]
        if prior:
            previous=prior[-1]
            op=previous['opcode']
            if op.startswith('ldc.i4.') and op[-1:].isdigit():constant=int(op[-1])
            elif op in ['ldc.i4','ldc.i4.s']:constant=previous['operand']
        facts.append({'caller':caller,'methodToken':edge['caller_token'],'ilOffset':edge['il_offset'],'target':owner+'.'+name,
                      'precedingIntegerLiteral':constant})
    return {'schemaVersion':1,'tool':'REA 6.8.0; static managed metadata/CIL anchors; target never executed',
            'subject':{'name':x['assembly']['name'],'version':x['assembly']['version'],'sha256':x['artifact']['sha256'],'module':x['module']},
            'inventory':{'types':len(x['types']),'methods':len(x['methods']),'callEdges':len(x['call_edges'])},
            'facts':facts,'profileFieldNames':[f['name'] for f in x['fields'] if f.get('declaring_type')==names[1]][:200],
            'limits':['Build-local tokens require the recorded binary hash.','Static calls do not measure runtime latency, reachability or successful audio startup.',
                      'Current Steam DSX and its private profile format were not inspected.','No artwork coordinates, BAML transforms or current DSX assets were recovered or copied.']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('evidence',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    result=summarize(json.loads(a.evidence.read_text(encoding='utf-8-sig')))
    a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(f'DSX static evidence: {len(result["facts"])} call-site facts; no target execution')


if __name__=='__main__':main()
