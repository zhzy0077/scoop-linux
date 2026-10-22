#!/usr/bin/env python3
"""Download pinned upstream artifacts and regenerate the native Linux main manifests.
Run in a native Podman tooling container. Cache is external to the repository.
Review upstream release notes/licenses before changing versions, then verify both CPUs.
"""
import concurrent.futures
import hashlib
import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
CACHE = pathlib.Path(os.environ.get('SCOOP_ARTIFACT_CACHE', '/artifacts'))
# name, repository, version, license, filename template, extracted directory, executable
PACKAGES = [
 ('ripgrep','BurntSushi/ripgrep','14.1.1','Unlicense|MIT','ripgrep-{v}-{rust}.tar.gz','ripgrep-{v}-{rust}','rg'),
 ('fd','sharkdp/fd','10.2.0','MIT|Apache-2.0','fd-v{v}-{rust}.tar.gz','fd-v{v}-{rust}','fd'),
 ('jq','jqlang/jq','1.7.1','MIT','jq-linux-{go}',None,'jq'),
 ('bat','sharkdp/bat','0.24.0','MIT|Apache-2.0','bat-v{v}-{rust}.tar.gz','bat-v{v}-{rust}','bat'),
 ('fzf','junegunn/fzf','0.60.3','MIT','fzf-{v}-linux_{go}.tar.gz',None,'fzf'),
 ('delta','dandavison/delta','0.18.2','MIT','delta-{v}-{rust}.tar.gz','delta-{v}-{rust}','delta'),
 ('zoxide','ajeetdsouza/zoxide','0.9.8','MIT','zoxide-{v}-{rust}.tar.gz',None,'zoxide'),
 ('eza','eza-community/eza','0.21.0','MIT','eza_{rust}.tar.gz',None,'eza'),
 ('yq','mikefarah/yq','4.45.1','MIT','yq_linux_{go}',None,'yq'),
 ('just','casey/just','1.40.0','CC0-1.0','just-{v}-{rust}.tar.gz',None,'just'),
]

def generate(package):
    name, repo, version, license_id, template, directory, binary = package
    architectures = {}
    evidence = {}
    for arch, cpu, go in [('64bit','x86_64','amd64'), ('arm64','aarch64','arm64')]:
        # Prefer portable static upstream builds where available.
        libc = 'gnu' if name in ('ripgrep','fd','bat') else 'musl'
        if name in ('eza', 'delta') and arch == 'arm64':
            libc = 'gnu'
        if name == 'ripgrep' and arch == '64bit':
            libc = 'musl'
        variables = dict(v=version, rust=f'{cpu}-unknown-linux-{libc}', go=go)
        filename = template.format(**variables)
        tag = version if name in ('ripgrep', 'delta') else f'v{version}'
        if name == 'jq':
            tag = f'jq-{version}'
        if name in ('fzf','just'):
            tag = f'v{version}' if name == 'fzf' else version
        url = f'https://github.com/{repo}/releases/download/{tag}/{filename}'
        artifact = CACHE / name / version / arch / filename
        artifact.parent.mkdir(parents=True, exist_ok=True)
        if not artifact.exists():
            partial = artifact.with_suffix(artifact.suffix + '.partial')
            subprocess.run(['curl','--fail','--location','--retry','3','--max-time','180','-o',str(partial),url],check=True)
            partial.replace(artifact)
        digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
        checksum_name = {'fzf': f'fzf_{version}_checksums.txt', 'jq': 'sha256sum.txt', 'just': 'SHA256SUMS', 'yq': 'checksums'}.get(name)
        if name == 'ripgrep':
            checksum_name = filename + '.sha256'
        checksum_url = None
        if checksum_name:
            checksum_url = f'https://github.com/{repo}/releases/download/{tag}/{checksum_name}'
            checksums = CACHE / name / version / ('publisher-' + checksum_name)
            if not checksums.exists():
                subprocess.run(['curl','--fail','--location','--retry','3','--max-time','180','-o',str(checksums),checksum_url],check=True)
            lines = checksums.read_text().splitlines()
            matching = lines if name == 'ripgrep' else [line for line in lines if filename in [token.lstrip('*').removeprefix('./') for token in line.split()]]
            if not any(digest in line.split() for line in matching):
                raise ValueError(f'publisher SHA-256 mismatch: {name} {arch} {filename}')
        evidence[arch] = {'artifact': url, 'sha256': digest, 'verification': 'publisher-sha256' if checksum_url else 'github-https-tofu', 'checksum_source': checksum_url}
        entry = {'url': url, 'hash': digest}
        if directory:
            entry['extract_dir'] = directory.format(**variables)
        if not filename.endswith(('.tar.gz','.zip','.tar.xz')):
            entry['url'] += '#/' + binary
        architectures[arch] = entry
    manifest = {'version':version,'description':f'{name}: portable upstream Linux CLI release',
                'homepage':f'https://github.com/{repo}','license':license_id,
                'architecture':architectures,'bin':binary}
    output = ROOT / 'main' / 'bucket' / f'{name}.json'
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(manifest,indent=4)+'\n')
    print(f'Verified SHA-256: {name} {version} ({"publisher" if name in ("ripgrep","fzf","jq","just","yq") else "GitHub HTTPS TOFU"})',flush=True)
    return name, evidence

if __name__ == '__main__':
    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
        evidence = dict(pool.map(generate,PACKAGES))
    (ROOT / 'main' / 'VERIFICATION.json').write_text(json.dumps(evidence,indent=2,sort_keys=True)+'\n')
