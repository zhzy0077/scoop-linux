#!/usr/bin/env python3
"""Build one genuinely independent version-changed Rust release per native builder.
Reuses compiled dependencies, copies only source inputs, and publishes a stable test
artifact path. Runtime-only Ubuntu/Fedora images execute this artifact unchanged.
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys

root=pathlib.Path(__file__).resolve().parent.parent
build=pathlib.Path(sys.argv[1]).resolve()
source=build/'real-release2-source'
source.mkdir(parents=True,exist_ok=True)
for name in ['Cargo.toml','Cargo.lock','build.rs','schema.json','release-catalog.schema.json','known-buckets.json']:
    shutil.copy2(root/name,source/name)
shutil.copytree(root/'src',source/'src',dirs_exist_ok=True)
version='0.2.0-mvp-review'
manifest=(source/'Cargo.toml').read_text()
manifest=re.sub(r'(?m)^version = "[^"]+"$',f'version = "{version}"',manifest,count=1)
(source/'Cargo.toml').write_text(manifest)
lock=(source/'Cargo.lock').read_text()
lock=re.sub(r'(\[\[package\]\]\nname = "scoop-linux"\nversion = ")[^"]+',rf'\g<1>{version}',lock,count=1)
(source/'Cargo.lock').write_text(lock)
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(build)
subprocess.run(['cargo','build','--release','--locked','--features','test-hooks','--manifest-path',str(source/'Cargo.toml')],env=env,check=True)
output=build/'real-release2';output.mkdir(exist_ok=True)
shutil.copy2(build/'release/scoop',output/'scoop')
assert subprocess.check_output([output/'scoop','--version'],text=True).strip()==f'scoop {version}'
(output/'version').write_text(version+'\n')
print(f'Independent Rust release: {output / "scoop"}',flush=True)
