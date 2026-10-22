#!/bin/sh
# Run the Fedora-built integration executables on a compatible native Linux image.
set -eu
sh "$(dirname "$0")/check-distribution-tools.sh"
build=${1:-/workspace/target/fedora}
export SCOOP_REAL_RELEASE2="$build/real-release2/scoop"
export SCOOP_DISTRIBUTION_GATE=1
test -x "$SCOOP_REAL_RELEASE2"
python3 - "$build/distribution-artifacts.jsonl" "$(dirname "$0")" <<'PY'
import json, pathlib, subprocess, sys
live_targets = {path.stem for path in pathlib.Path(sys.argv[2]).glob('*.rs')}
artifacts = [json.loads(line) for line in open(sys.argv[1])]
suites = {entry['target']['name']: entry for entry in artifacts
          if entry.get('reason') == 'compiler-artifact' and 'test' in entry['target']['kind']
          and entry['profile']['test'] and entry.get('executable')
          and entry['target']['name'] in live_targets}
assert set(suites) == live_targets, 'Run tests/build-distribution.sh for the current test targets'
assert suites and 'recovery' in suites, 'Run tests/build-distribution.sh first'
assert 'test-hooks' in suites['recovery']['features'], 'Recovery requires the test-hooks build'
for name, artifact in sorted(suites.items()):
    subprocess.run([artifact['executable']], check=True)
print(f"Executed {len(suites)} exact integration-suite artifacts.")
PY
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
export HOME="$work/home" SCOOP="$work/root" XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export SCOOP_TEST_BINARY="$build/debug/scoop"
mkdir -p "$XDG_DATA_HOME/application"
printf 'application-owned\n' > "$XDG_DATA_HOME/application/keep"
for shell in bash zsh fish; do
    "$shell" -c '"$SCOOP_TEST_BINARY" --version'
    "$shell" -c '"$SCOOP_TEST_BINARY" cat /workspace/tests/fixtures/portable.json' > "$work/cat.json"
    cmp /workspace/tests/fixtures/portable.json "$work/cat.json"
done
"$SCOOP_TEST_BINARY" help > "$work/help"
"$SCOOP_TEST_BINARY" config default_architecture aarch64
"$SCOOP_TEST_BINARY" config default_architecture | grep -qx aarch64
"$SCOOP_TEST_BINARY" config rm default_architecture
printf 'application-owned\n' | cmp - "$XDG_DATA_HOME/application/keep"
echo 'Distribution verification passed: integration suites, Bash/Zsh/Fish CLI, exact cat output, config, XDG preservation.'
