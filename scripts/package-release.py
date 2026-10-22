#!/usr/bin/env python3
"""Package native-built GNU Linux binaries without executing foreign artifacts.
--artifact TARGET BINARY MINIMUM_GLIBC_OR_auto HTTPS_OR_LOCAL_BINARY_URL (repeat).
Requires readelf and Git. Computes the ELF symbol floor and NEEDED libraries.
"""
import argparse
import gzip
import hashlib
import io
import json
import pathlib
import re
import subprocess
import tarfile
import urllib.parse

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--version',required=True)
parser.add_argument('--main-bucket',required=True)
parser.add_argument('--output',required=True,type=pathlib.Path)
parser.add_argument('--artifact',nargs=4,action='append',required=True,
                    metavar=('TARGET','BINARY','MINIMUM_GLIBC_OR_auto','BINARY_URL'))
args=parser.parse_args()

def location(value, main=False):
    parsed=urllib.parse.urlparse(value)
    if parsed.scheme=='https' and parsed.hostname:
        return False
    if parsed.scheme=='file' and parsed.netloc in ('','localhost'):
        path=pathlib.Path(urllib.parse.unquote(parsed.path))
    elif pathlib.Path(value).is_absolute():
        path=pathlib.Path(value)
    else:
        parser.error('release locations require HTTPS; local absolute paths/file URLs are allowed')
    if not path.is_absolute():
        parser.error('local release paths must be absolute')
    if main:
        result=subprocess.run(['git','-C',str(path),'rev-parse','--is-inside-work-tree'],capture_output=True,text=True)
        if result.returncode or result.stdout.strip()!='true':
            parser.error(f'main bucket must be a Git repository: {path}')
    return True

if not re.fullmatch(r'[A-Za-z0-9._+\-]+',args.version) or args.version in ('.','..','current'):
    parser.error('invalid release version')
local=location(args.main_bucket,main=True)
catalog=dict(format_version=1,version=args.version,main_bucket_url=args.main_bucket,targets={})
artifacts=[]
for target,source,minimum,url in args.artifact:
    local=location(url) or local
    architecture={'x86_64-unknown-linux-gnu':'64bit','aarch64-unknown-linux-gnu':'arm64'}.get(target)
    if not architecture or target in catalog['targets']:
        parser.error(f'unsupported or duplicate target: {target}')
    binary=pathlib.Path(source).read_bytes()
    if len(binary)<64 or binary[:6]!=b'\x7fELF\x02\x01' or binary[7] not in (0,3):
        parser.error('release must be a little-endian 64-bit Linux ELF binary')
    if int.from_bytes(binary[18:20],'little')!={'64bit':62,'arm64':183}[architecture]:
        parser.error(f'ELF architecture mismatch for {target}')
    versions=subprocess.check_output(['readelf','--version-info',source],text=True)
    floors=re.findall(r'GLIBC_(\d+(?:\.\d+)+)',versions)
    if not floors:
        parser.error('GNU release must declare glibc symbol requirements')
    floor=max(floors,key=lambda v:tuple(map(int,v.split('.'))))
    if minimum!='auto' and minimum!=floor:
        parser.error(f'minimum_libc {minimum!r} differs from required ELF floor {floor}; use auto')
    dynamic=subprocess.check_output(['readelf','--dynamic',source],text=True)
    libraries=sorted(set(re.findall(r'\(NEEDED\).*?\[([^]]+)\]',dynamic)))
    loader={'64bit':'ld-linux-x86-64.so.2','arm64':'ld-linux-aarch64.so.1'}[architecture]
    if not libraries or set(libraries)-{'libc.so.6','libm.so.6','libgcc_s.so.1',loader}:
        parser.error(f'unsupported runtime dependencies for {target}: {libraries}; static-link extra libraries')
    catalog['targets'][target]=dict(architecture=architecture,os='linux',libc='glibc',abi='gnu',
        minimum_libc=floor,required_libraries=libraries,url=url,sha256=hashlib.sha256(binary).hexdigest())
    artifacts.append((target,binary))
# Validate all inputs before publishing any output files.
args.output.mkdir(parents=True,exist_ok=True)
for target,binary in artifacts:
    name=f'scoop-{args.version}-{target}'
    (args.output/name).write_bytes(binary);(args.output/name).chmod(0o755)
    with (args.output/(name+'.tar.gz')).open('wb') as raw:
        with gzip.GzipFile(filename='',mode='wb',fileobj=raw,mtime=0) as zipped:
            with tarfile.open(fileobj=zipped,mode='w') as archive:
                entry=tarfile.TarInfo('scoop');entry.size=len(binary);entry.mode=0o755
                archive.addfile(entry,io.BytesIO(binary))
(args.output/'release-catalog.json').write_text(json.dumps(catalog,indent=2)+'\n')
label='LOCAL VERIFICATION METADATA: local/container paths; not a publishable catalog.\n' if local else 'Release metadata uses publisher-supplied HTTPS locations.\n'
(args.output/'README.txt').write_text(label+'GNU Linux x86-64/arm64. Requires the catalog glibc floor and listed system libraries. liblzma is statically linked.\n')
files=sorted(p for p in args.output.iterdir() if p.is_file() and p.name!='SHA256SUMS')
(args.output/'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in files))
print(args.output/'release-catalog.json')
