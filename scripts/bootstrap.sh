#!/bin/sh
# Seed bootstrap: catalog, exact native raw-binary URL, trusted SHA-256.
# The seed validates the catalog's native target, ABI, version, checksum and main bucket.
set -eu
if [ "$#" -ne 3 ]; then
    echo "Usage: sh bootstrap.sh CATALOG BINARY_URL SHA256" >&2
    exit 2
fi
catalog=$1 url=$2 checksum=$3
case "$catalog" in
    https://*|file://*) ;;
    *://*) echo 'Remote release catalogs require HTTPS' >&2; exit 1 ;;
esac
[ "$(uname -s)" = Linux ] || { echo 'Linux is required' >&2; exit 1; }
case "$(uname -m)" in x86_64|aarch64) ;; *) echo 'No validated release for this architecture' >&2; exit 1 ;; esac
[ "${#checksum}" -eq 64 ] || { echo 'SHA-256 must contain 64 hex digits' >&2; exit 1; }
case "$checksum" in *[!0-9a-fA-F]*) echo 'Invalid SHA-256' >&2; exit 1 ;; esac
command -v git >/dev/null
command -v curl >/dev/null
command -v sha256sum >/dev/null
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
case "$url" in
    https://*|file://*) curl --fail --location --proto '=https,file' --proto-redir '=https' "$url" -o "$work/scoop" ;;
    *) echo 'Binary URL must use HTTPS or file' >&2; exit 1 ;;
esac
printf '%s  %s\n' "$checksum" "$work/scoop" | sha256sum -c -
magic=$(od -An -tx1 -N4 "$work/scoop" | tr -d ' \n')
[ "$magic" = 7f454c46 ] || { echo 'Seed must be a native Linux ELF binary' >&2; exit 1; }
chmod 755 "$work/scoop"
SCOOP_BOOTSTRAP_SHA256="$checksum" "$work/scoop" --bootstrap "$catalog"
