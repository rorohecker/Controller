"""Controller-free integration gates; a failing child cannot be masked by the next."""
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]

if __name__=='__main__':
    for name in ['script_smoke','runtime_core_smoke']:
        subprocess.run([sys.executable,str(ROOT/'tests'/f'{name}.py'),*sys.argv[1:]],cwd=ROOT,check=True)
