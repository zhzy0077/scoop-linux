#!/usr/bin/env python3
"""Native Podman real-package gate: BINARY TARGET NEW_WORK_DIRECTORY.
Retains explicitly LOCAL verification catalogs; never produces publication metadata.
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile

ROOT=pathlib.Path(__file__).resolve().parent.parent
binary,target,work=pathlib.Path(sys.argv[1]).resolve(),sys.argv[2],pathlib.Path(sys.argv[3]).resolve()
work.mkdir(parents=True,exist_ok=False)
repo=work/'main';shutil.copytree(ROOT/'main',repo)
def run(args,env=None,capture=False,input=None):
    print('+',' '.join(map(str,args)),flush=True)
    return subprocess.run(list(map(str,args)),env=env,check=True,text=True,input=input,stdout=subprocess.PIPE if capture else None)
def git(*args):
    return run(['git','-C',repo,'-c','user.name=MVP','-c','user.email=mvp@example.org',*args])
git('init');git('add','.');git('commit','-m','Verified Linux main')
original=(repo/'bucket/jq.json').read_text();original_jq=json.loads(original)['version']
def publish_jq(version,message):
    manifest=json.loads(original);manifest['version']=version
    (repo/'bucket/jq.json').write_text(json.dumps(manifest));git('add','.');git('commit','-m',message)
publish_jq('0.0.0-mvp','Retained-version fixture: same publisher-verified jq bytes')
version=run([binary,'--version'],capture=True).stdout.strip().split()[-1]
release=work/'local-release';raw=release/f'scoop-{version}-{target}'
run(['python3',ROOT/'scripts/package-release.py','--version',version,'--main-bucket',repo,'--output',release,'--artifact',target,binary,'auto',raw.as_uri()])
with tarfile.open(str(raw)+'.tar.gz','r:gz') as archive:
    member=archive.getmember('scoop');assert member.mode==0o755 and archive.extractfile(member).read()==binary.read_bytes()
run(['sh','-c','cd "$1" && sha256sum -c SHA256SUMS','verify',release])
catalog=release/'release-catalog.json';metadata=json.loads(catalog.read_text());checksum=metadata['targets'][target]['sha256']
assert 'LOCAL VERIFICATION' in (release/'README.txt').read_text()
assert 'liblzma.so.5' not in metadata['targets'][target]['required_libraries']
def environment(name):
    base=work/name;base.mkdir();env=os.environ.copy()
    env.update(HOME=str(base),SCOOP=str(base/'root'),XDG_CONFIG_HOME=str(base/'config'),XDG_DATA_HOME=str(base/'data'),PATH=str(base/'root/bin')+':'+os.environ['PATH'])
    return env
a=environment('original');b=environment('restored')
def scoop(env,*args,capture=False):
    return run([pathlib.Path(env['SCOOP'])/'bin/scoop',*args],env,capture)
def bootstrap(env):
    run(['sh',ROOT/'scripts/bootstrap.sh',catalog,raw.as_uri(),checksum],env)
    assert (pathlib.Path(env['SCOOP'])/f'apps/scoop/{version}/scoop').read_bytes()==binary.read_bytes()
    scoop(env,'config','cache_path',os.environ.get('SCOOP_GATE_CACHE',str(work/'package-cache')))
def assert_app(env,name,expected,held=None):
    app=pathlib.Path(env['SCOOP'])/'apps'/name
    assert os.readlink(app/'current')==expected,(name,os.readlink(app/'current'),expected)
    manifest=json.loads((app/expected/'scoop-manifest.json').read_text());assert manifest['version']==expected
    record=json.loads((app/expected/'scoop-install.json').read_text())
    assert record['format_version']==1 and record['bucket']=='main'
    assert record['architecture']=={'aarch64-unknown-linux-gnu':'arm64','x86_64-unknown-linux-gnu':'64bit'}[target]
    if held is not None: assert record['hold']==held
    assert scoop(env,'prefix',name,capture=True).stdout.strip()==str(app/'current')
    assert any(line.startswith(name+'\t'+expected+'\t') for line in scoop(env,'list',capture=True).stdout.splitlines())
bootstrap(a)
apps=sorted(path.stem for path in (repo/'bucket').glob('*.json'))
expected={name:json.loads((repo/f'bucket/{name}.json').read_text())['version'] for name in apps}
scoop(a,'install',*[f'main/{name}' for name in apps],'-u')
for name in apps: assert_app(a,name,expected[name],False)
(repo/'bucket/jq.json').write_text(original);git('add','.');git('commit','-m','Advance jq to pinned release')
scoop(a,'update');scoop(a,'update','*');expected['jq']=original_jq
for name in apps: assert_app(a,name,expected[name],False)
scoop(a,'reset','jq@0.0.0-mvp');assert_app(a,'jq','0.0.0-mvp',False)
scoop(a,'reset','jq@'+original_jq);assert_app(a,'jq',original_jq,False)
scoop(a,'cleanup','--all');assert not (pathlib.Path(a['SCOOP'])/'apps/jq/0.0.0-mvp').exists()
scoop(a,'hold','jq');assert_app(a,'jq',original_jq,True);scoop(a,'status','-l');scoop(a,'checkup')
exported=scoop(a,'export','-c',capture=True).stdout;document=json.loads(exported)
assert 'release_catalog' not in document['config'] and 'main_bucket_url' not in document['config']
assert 'Held package' in next(app for app in document['apps'] if app['Name']=='jq')['Info']
scoopfile=work/'scoopfile.json';scoopfile.write_text(exported)
# Bootstrap the restored machine after origin advanced, as on a fresh machine.
bootstrap(b);scoop(b,'import',scoopfile)
for name in apps: assert_app(b,name,expected[name],name=='jq')
restored=json.loads(scoop(b,'export',capture=True).stdout)
assert {app['Name']:(app['Version'],app['Source'],app['Info']) for app in restored['apps']}=={app['Name']:(app['Version'],app['Source'],app['Info']) for app in document['apps']}
# Make jq genuinely outdated in isolated metadata, proving the restored hold skips it.
publish_jq('99.0.0-mvp','Held-upgrade fixture: same publisher-verified jq bytes')
scoop(b,'update');out=scoop(b,'update','*',capture=True).stdout;assert 'held' in out.lower();assert_app(b,'jq',original_jq,True)
for env in (a,b):
    for name in apps:
        run([pathlib.Path(env['SCOOP'])/'bin'/('rg' if name=='ripgrep' else name),'--version'],env)
    result=run([pathlib.Path(env['SCOOP'])/'bin/jq','-r','.gate'],env,capture=True,input='{"gate":"real-package"}');assert result.stdout=='real-package\n'
    scoop(env,'checkup')
    data=pathlib.Path(env['XDG_DATA_HOME'])/'application/keep';data.parent.mkdir(parents=True);data.write_text('application owned')
    scoop(env,'uninstall',*apps);assert data.read_text()=='application owned'
    assert json.loads(scoop(env,'export',capture=True).stdout)['apps']==[]
print(f'PASS: {target}: exact records/versions/holds + 10 real packages + trusted bootstrap + update/reset/export/import/uninstall',flush=True)
