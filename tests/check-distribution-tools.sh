#!/bin/sh
# Broad integration-suite prerequisites; these are not Scoop runtime dependencies.
set -eu
for tool in openssl python3 git cc bash zsh fish cmp; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Missing distribution-test prerequisite: %s. Install native test tools; see README.md "Native verification tools".\n' "$tool" >&2
        exit 1
    fi
done
