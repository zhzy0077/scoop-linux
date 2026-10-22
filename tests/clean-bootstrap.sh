#!/bin/sh
# Run in a fresh native Podman runtime with only documented bootstrap prerequisites.
set -eu
[ "$#" -eq 3 ] || { echo 'Usage: clean-bootstrap.sh LOCAL_CATALOG NATIVE_RAW_BINARY EXPECTED_VERSION' >&2; exit 2; }
catalog=$1 binary=$2 version=$3
uname -m
getconf GNU_LIBC_VERSION
for tool in cargo cc python3; do
    if command -v "$tool" >/dev/null 2>&1; then
        echo "Unexpected build/test tool in clean runtime: $tool" >&2
        exit 1
    fi
done
git --version
curl --version
ldd "$binary"
work=${SCOOP_CLEAN_WORK:-/root/scoop-clean-verification}
[ ! -e "$work" ] || { echo 'Clean bootstrap work directory must be new' >&2; exit 1; }
export HOME="$work/home" SCOOP="$work/root" XDG_CONFIG_HOME="$work/config"
mkdir -p "$HOME"
checksum=$(sha256sum "$binary" | cut -d ' ' -f 1)
sh /workspace/scripts/bootstrap.sh "$catalog" "file://$binary" "$checksum"
[ "$("$SCOOP/bin/scoop" --version)" = "scoop $version" ]
[ "$(sha256sum "$SCOOP/apps/scoop/$version/scoop" | cut -d ' ' -f 1)" = "$checksum" ]
export PATH="$SCOOP/bin:$PATH"
"$SCOOP/bin/scoop" checkup
echo 'PASS: clean native runtime bootstrap, trusted bytes, version and checkup; no Python/compiler/Rust toolchain.'
