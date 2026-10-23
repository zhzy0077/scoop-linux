#!/usr/bin/env python3
"""Prepare native GitHub release assets consumed by Scoop's self-updater."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

TARGETS = {
    "x86_64-unknown-linux-gnu": ("64bit", "ld-linux-x86-64.so.2"),
    "aarch64-unknown-linux-gnu": ("arm64", "ld-linux-aarch64.so.1"),
}


def checksum(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
    tag = os.environ["RELEASE_TAG"]
    if tag not in (version, f"v{version}"):
        raise SystemExit(f"Tag {tag!r} must match Cargo.toml version {version} (optional v prefix)")
    command = sys.argv[1]
    if command == "version":
        return
    dist = Path("dist")
    dist.mkdir(exist_ok=True)
    if command == "package":
        target = os.environ["RELEASE_TARGET"]
        architecture, loader = TARGETS[target]
        binary = Path("target") / target / "release/scoop"
        actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
        if actual != f"scoop {version}":
            raise SystemExit(f"Unexpected binary version: {actual}")
        # Validate the actual dynamic dependencies and symbol floor before publishing.
        dynamic = subprocess.check_output(["readelf", "-d", str(binary)], text=True)
        libraries = sorted(set(re.findall(r"\(NEEDED\).*\[(.*?)\]", dynamic)))
        allowed = {"libc.so.6", "libm.so.6", "libgcc_s.so.1", loader}
        if not set(libraries) <= allowed:
            raise SystemExit(f"Unexpected runtime libraries: {libraries}")
        symbols = subprocess.check_output(["readelf", "--version-info", str(binary)], text=True)
        versions = re.findall(r"GLIBC_([0-9]+(?:\.[0-9]+)+)", symbols)
        minimum = max(versions, key=lambda value: tuple(map(int, value.split('.'))))
        name = f"scoop-{target}"
        artifact = dist / name
        shutil.copy2(binary, artifact)
        base = f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}"
        metadata = {
            "architecture": architecture, "os": "linux", "libc": "glibc", "abi": "gnu",
            "minimum_libc": minimum, "required_libraries": libraries,
            "url": f"{base}/releases/download/{tag}/{name}", "sha256": checksum(artifact),
        }
        (dist / f"{target}.json").write_text(json.dumps(metadata, indent=2) + '\n')
    elif command == "catalog":
        targets = {}
        for target in TARGETS:
            metadata = json.loads((dist / f"{target}.json").read_text())
            if checksum(dist / f"scoop-{target}") != metadata["sha256"]:
                raise SystemExit(f"Checksum mismatch for {target}")
            targets[target] = metadata
        catalog = {"format_version": 1, "version": version, "targets": targets}
        (dist / "release-catalog.json").write_text(json.dumps(catalog, indent=2) + '\n')
        assets = sorted(dist.glob("scoop-*")) + [dist / "release-catalog.json"]
        (dist / "SHA256SUMS").write_text(''.join(f"{checksum(p)}  {p.name}\n" for p in assets))
    else:
        raise SystemExit(f"Unknown command: {command}")


if __name__ == "__main__":
    main()
