#!/usr/bin/env python3
"""Native Podman dual-target packaging/bootstrap gate; reads foreign ELF, executes native only.
Usage: packaging-gate.py ARM64_BINARY X86_64_BINARY NEW_WORK_DIRECTORY
"""
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tarfile
root=pathlib.Path(__file__).resolve().parent.parent
arm,x86,work=pathlib.Path(sys.argv[1]).resolve(),pathlib.Path(sys.argv[2]).resolve(),pathlib.Path(sys.argv[3]).resolve()
work.mkdir(parents=True,exist_ok=False)
main=work/'main';shutil.copytree(root/'main',main)
for args in [('init',),('add','.'),('-c','user.name=Gate','-c','user.email=gate@example.org','commit','-m','Linux main')]:
    subprocess.run(['git','-C',str(main),*args],check=True)
native={'aarch64':arm,'x86_64':x86}[platform.machine()]
version=subprocess.check_output([native,'--version'],text=True).strip().split()[-1]
artifacts={'aarch64-unknown-linux-gnu':arm,'x86_64-unknown-linux-gnu':x86}
output=work/'local-release'
base=['python3',str(root/'scripts/package-release.py'),'--version',version,'--main-bucket',str(main)]
def arguments(destination):
    result=base+['--output',str(destination)]
    for target,binary in artifacts.items():
        result+=['--artifact',target,str(binary),'auto',(output/f'scoop-{version}-{target}').as_uri()]
    return result
subprocess.run(arguments(output),check=True)
second=work/'reproduced';subprocess.run(arguments(second),check=True)
assert {p.name:p.read_bytes() for p in output.iterdir()}=={p.name:p.read_bytes() for p in second.iterdir()}
subprocess.run(['sh','-c','cd "$1" && sha256sum -c SHA256SUMS','verify',str(output)],check=True)
catalog=json.loads((output/'release-catalog.json').read_text());assert set(catalog['targets'])==set(artifacts)
assert 'LOCAL VERIFICATION' in (output/'README.txt').read_text()
for target,binary in artifacts.items():
    metadata=catalog['targets'][target]
    assert metadata['sha256']==hashlib.sha256(binary.read_bytes()).hexdigest()
    loader={'aarch64-unknown-linux-gnu':'ld-linux-aarch64.so.1','x86_64-unknown-linux-gnu':'ld-linux-x86-64.so.2'}[target]
    assert metadata['minimum_libc'] and set(metadata['required_libraries'])<= {'libc.so.6','libm.so.6','libgcc_s.so.1',loader}
    with tarfile.open(output/f'scoop-{version}-{target}.tar.gz') as archive:
        member=archive.getmember('scoop');assert member.mode==0o755 and member.uid==0 and member.mtime==0
        assert archive.extractfile(member).read()==binary.read_bytes()
# Validate input rejection without publishing any artifacts.
for label,index,value in [('floor',None,'banana'),('floor-too-low',None,'2.17'),('transport',None,'http://localhost/scoop'),('main','--main-bucket','/not-a-git-bucket'),('version','--version','../bad')]:
    args=arguments(work/('invalid-'+label))
    if index: args[args.index(index)+1]=value
    elif label=='transport': args[args.index('--artifact')+4]=value
    else: args[args.index('--artifact')+3]=value
    result=subprocess.run(args,capture_output=True,text=True);assert result.returncode!=0,(label,result.stdout,result.stderr)
    assert not (work/('invalid-'+label)).exists()
# Exact native selection from a two-target catalog; foreign bytes are never executed.
target={'aarch64':'aarch64-unknown-linux-gnu','x86_64':'x86_64-unknown-linux-gnu'}[platform.machine()]
metadata=catalog['targets'][target];env=os.environ.copy();env.update(HOME=str(work/'home'),SCOOP=str(work/'installed'),XDG_CONFIG_HOME=str(work/'config'))
subprocess.run(['sh',str(root/'scripts/bootstrap.sh'),str(output/'release-catalog.json'),metadata['url'],metadata['sha256']],env=env,check=True)
assert (work/f'installed/apps/scoop/{version}/scoop').read_bytes()==native.read_bytes()
assert subprocess.check_output([work/'installed/bin/scoop','--version'],env=env,text=True).strip()==f'scoop {version}'
print('PASS: both native targets packaged deterministically; truthful libc/libraries; negative inputs; trusted native catalog selection.')
