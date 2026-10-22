#!/usr/bin/env python3
"""Regenerate from a chosen (preferably fresh) artifact cache and require no changes."""
import argparse
import os
import pathlib
import subprocess
import sys
root=pathlib.Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cache',required=True)
args=parser.parse_args()
def snapshot():
    return {str(path.relative_to(root)):path.read_bytes() for path in sorted((root/'main/bucket').glob('*.json'))}|{'main/VERIFICATION.json':(root/'main/VERIFICATION.json').read_bytes()}
before=snapshot()
env=os.environ.copy();env['SCOOP_ARTIFACT_CACHE']=str(pathlib.Path(args.cache).resolve())
subprocess.run([sys.executable,root/'scripts/refresh-main.py'],env=env,check=True)
after=snapshot()
if before!=after:
    raise SystemExit('Regeneration changed: '+', '.join(name for name in before.keys()|after.keys() if before.get(name)!=after.get(name)))
print('PASS: all 20 artifacts/publisher metadata regenerate byte-identical manifests and verification records.')
