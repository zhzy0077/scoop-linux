# Native MVP verification

The checks below operate on the current source tree. [`MVP-CHECKLIST.md`](MVP-CHECKLIST.md) records acceptance and [`COMPATIBILITY-AUDIT.md`](COMPATIBILITY-AUDIT.md) maps each requirement to its regression. Exact review snapshot provenance and command transcripts are retained in the maintainer archive; they are not required to build or verify the repository.

## Validation matrix

All verification uses native Podman containers with Rust/Cargo 1.99.1 in the builders. Each CPU's Fedora-built executables are reused on its other distributions.

| CPU | Native builder | Validated runtimes |
|---|---|---|
| arm64 / aarch64 | Fedora 45 | Fedora 45, Ubuntu 26.04.1 LTS |
| x86-64 | Fedora 43 | Fedora 43, Fedora 45, Ubuntu 26.04.1 LTS |

The complete source suite contains **88 default-feature tests** and **97 hook-enabled tests across 26 integration suites**. The distribution gate also runs Bash/Zsh/Fish checks. The real-package, dual-target packaging and pristine-runtime gates exercise the same native production binaries. Recovery assertions execute the recovered stable binary after prepared, published, current, links and committed self-activation boundaries.

Reference production runtime requirements are GLIBC 2.39, libc/libm/libgcc_s and the native GNU ELF loader. liblzma is statically linked. Packaging computes the actual symbol floor and NEEDED libraries for its inputs; the catalog carries those requirements.

## Native tooling and test builds

Run these commands **inside a native Fedora Podman builder**, with the repository mounted at `/workspace`. Install the [native test tools](README.md#native-verification-tools) and Rust/Cargo with rustfmt/Clippy there. Mount the same repository and target directory at the same absolute paths in each compatible runtime container.

```sh
cd /workspace
export CARGO_TARGET_DIR=/workspace/target/native
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
sh tests/build-distribution.sh "$CARGO_TARGET_DIR"
sh tests/run-distribution.sh "$CARGO_TARGET_DIR"
# Produce the default-feature production binary after the secondary test build:
cargo build --release --locked
"$CARGO_TARGET_DIR/release/scoop" --version
```

`build-distribution.sh` records Cargo's exact feature-qualified test paths and builds an independent version-changed Rust release under `real-release2/`, reusing native dependencies. The secondary test build shares the builder's release output directory; the final production build above restores the default-feature binary.

In Ubuntu and any additional validated Fedora runtime, execute the recorded native artifacts:

```sh
cd /workspace
sh tests/run-distribution.sh /workspace/target/native
```

The runner checks OpenSSL CLI/Python/Git/compiler/shell tools and requires the independent Rust artifact. It sets `SCOOP_REAL_RELEASE2` and `SCOOP_DISTRIBUTION_GATE=1`, so the real-release test bodies execute. Hook builds provide process-crash injection and a private HTTPS-fixture CA; production binaries contain neither active hook nor private-CA overrides.

## Real-package lifecycle

Inside each native runtime, choose its exact Rust target and a new retained work directory:

```sh
cd /workspace
# x86-64; use aarch64-unknown-linux-gnu on arm64:
python3 tests/release-lifecycle.py \
  target/native/release/scoop x86_64-unknown-linux-gnu target/release-gate
```

The gate bootstraps the trusted native bytes, installs and executes all ten real packages, updates/resets retained jq metadata versions, checks exact current/saved-manifest/record identities, verifies cleanup and holds, recreates Version/Source/Info through export/import, checks held-update skipping, and uninstalls while preserving XDG data. Its old/future jq versions are isolated metadata fixtures around the same verified upstream bytes. Published manifests retain upstream versions.

`SCOOP_GATE_CACHE` can select an existing verified package cache; reuse revalidates its hashes. Every run needs a new work directory. Generated catalogs are labelled **LOCAL VERIFICATION METADATA** and contain explicit local paths.

## Two-target packaging

Collect the production binaries built on the two native CPUs. The packaging gate reads both ELF files and executes only the one matching its current host:

```sh
python3 tests/packaging-gate.py \
  /absolute/arm64/scoop /absolute/x86_64/scoop target/packaging-gate
```

It checks computed libc/library requirements, ELF/URL/source/version inputs, deterministic raw/tar/catalog/checksum outputs, tar ownership/mode/time, invalid-input rejection and correct native bootstrap from a two-target catalog. Run it on both native hosts. Hosted artifact/catalog locations are a separate release configuration; see [packaging instructions](README.md#packaging-a-release).

## Pristine runtime bootstrap

Use fresh native Fedora 45 and Ubuntu 26.04 containers provisioned with only the [documented bootstrap/runtime prerequisites](README.md#validated-gnu-runtime-matrix). The clean-bootstrap script requires Python, cc and Cargo to be absent. Fedora Git documentation weak dependencies can be excluded/removed while preserving Git; Ubuntu uses its runtime packages without build-essential.

With the retained local catalog from the lifecycle gate and matching production file mounted at `/workspace`:

```sh
sh /workspace/tests/clean-bootstrap.sh \
  /workspace/target/release-gate/local-release/release-catalog.json \
  /workspace/target/native/release/scoop 0.1.0
```

This checks native architecture/glibc, runtime libraries, trusted seed/installed byte identity, expected version and checkup. Run it in a new clean work directory on both distributions for each CPU. Independent review also installed, ran and removed the ten real packages on pristine runtimes.

## Main-bucket integrity and canonical defaults

The dedicated bucket payload is [`main/`](main/README.md). `main/VERIFICATION.json` records all 20 artifact hashes and their trust provenance: publisher checksum assets confirm fzf/jq/just/ripgrep/yq; bat/delta/eza/fd/zoxide are pinned GitHub HTTPS TOFU for these releases. Licenses and both architecture/bin entries are validated by `main_bucket`.

To independently repeat regeneration in a native tooling container:

```sh
python3 tests/verify-main.py --cache /absolute/fresh-artifact-cache
```

It downloads the pinned artifacts/checksums and requires byte-identical manifests/provenance. Reviewed unchanged payloads can reuse their existing verification records.

`main_bucket::main_default` tests canonical known/add/discovery, bootstrap fallback and explicit/environment/config/catalog priority through isolated local Git URL rewrites. Check the real canonical endpoint from a fresh Scoop root without those rewrites:

```sh
scoop_binary=/absolute/native/scoop
work=$(mktemp -d)
export SCOOP="$work/root" XDG_CONFIG_HOME="$work/config"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset SCOOP_MAIN_BUCKET SCOOP_RELEASE_CATALOG
"$scoop_binary" bucket known
"$scoop_binary" bucket add main
"$scoop_binary" cat main/jq > "$work/jq.json"
cmp main/bucket/jq.json "$work/jq.json"
"$scoop_binary" info main/jq
```

The canonical source is `https://github.com/zhzy0077/scoop-linux-pkgs`. Public source/bucket publication and hosted release artifact/catalog publication remain separate operations.
