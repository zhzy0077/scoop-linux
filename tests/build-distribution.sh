#!/bin/sh
# Run inside the native Podman build image. Record exact feature-qualified test artifacts.
set -eu
build=${1:-/workspace/target/fedora}
export CARGO_TARGET_DIR="$build"
mkdir -p "$build"
manifest=$(mktemp "$build/distribution-artifacts.XXXXXX")
trap 'rm -f "$manifest"' EXIT HUP INT TERM
cargo test --locked --features test-hooks --no-run --message-format=json > "$manifest"
mv "$manifest" "$build/distribution-artifacts.jsonl"
python3 tests/build-real-release.py "$build"
