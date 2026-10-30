"""Run every UI/offline smoke gate, propagating any failure to CI."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
CHECKS=['ui_smoke','game_profiles_smoke','game_session_smoke','studio_smoke','platform_smoke',
        'reliability_smoke','audio_stream_smoke','haptic_authoring_smoke','layout_smoke',
        'motion_input_smoke','daily_use_smoke','integrations_smoke','hardware_research_smoke','research_tools_smoke']


def run(name):
    print(f'Running {name}',flush=True)
    result=subprocess.run([sys.executable,str(ROOT/'tests'/f'{name}.py')],cwd=ROOT,capture_output=True,text=True,encoding='utf-8',errors='replace')
    print(result.stdout+result.stderr,end='',flush=True)
    return result.returncode


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--jobs',type=int,choices=range(1,5),default=1);args=parser.parse_args()
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:results=list(pool.map(run,CHECKS))
    # Resource samples run alone, after all other browser/decoder checks.
    results.append(run('runtime_reliability_smoke'))
    return int(any(code!=0 for code in results))


if __name__=='__main__':raise SystemExit(main())
