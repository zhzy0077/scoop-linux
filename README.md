# Scoop on Linux

A user-scoped Linux package manager in Rust, installed as `scoop`. [`DESIGN.md`](DESIGN.md) is the unchanged locked contract; [`MVP-CHECKLIST.md`](MVP-CHECKLIST.md) records reviewed acceptance.

## Linux MVP

All 24 scoped commands are implemented: discovery, verified native package lifecycle, maintenance/recovery, bootstrap/self-update, Scoopfiles and diagnostics. The canonical [`main` bucket](https://github.com/zhzy0077/scoop-linux-pkgs) supplies ten reviewed Linux CLI manifests. [`VERIFICATION.md`](VERIFICATION.md) contains the native validation matrix and reproducible checks; [`COMPATIBILITY-AUDIT.md`](COMPATIBILITY-AUDIT.md) maps requirements to tests.

Build with `cargo build --locked`. Run `scoop help` or `scoop <command> --help` for syntax and options. All project verification runs inside native Podman containers; see the evidence in the checklist.

### Command contract

| Command | Syntax |
| --- | --- |
| help | `help [command]` |
| config | `config [rm] [name] [value]` |
| bucket | `bucket add <name> [repo]`, `bucket list`, `bucket known`, `bucket rm <name>` |
| search | `search [regex]` |
| info | `info <app> [-v/--verbose]` |
| cat | `cat <app>` |
| install | `install <apps>... [-i/--independent] [-k/--no-cache] [-s/--skip-hash-check] [-u/--no-update-scoop] [-a/--arch <architecture>]` |
| uninstall | `uninstall <apps>...` |
| list | `list [query]` |
| prefix | `prefix <app>` |
| which | `which <command>` |
| depends | `depends <app> [-a/--arch <architecture>]` |
| download | `download <apps>... [-f/--force] [-s/--skip-hash-check] [-u/--no-update-scoop] [-a/--arch <architecture>]` |
| update | `update [apps]... [-f/--force] [-i/--independent] [-k/--no-cache] [-s/--skip-hash-check] [-q/--quiet] [-a/--all]` |
| status | `status [-l/--local]` |
| hold | `hold <apps>...` |
| unhold | `unhold <apps>...` |
| reset | `reset <apps[@version]>...` or `reset -a/--all` |
| cleanup | `cleanup <apps>... [-k/--cache]` or `cleanup -a/--all [-k/--cache]` |
| cache | `cache [show [apps]...]`, `cache rm <apps>...`, `cache rm -a/--all` |
| export | `export [-c/--config]` |
| import | `import <file-or-url>` |
| checkup | `checkup` |
| home | `home <app>` |

Use quotes for wildcard operands, for example `scoop update '*'`. `alias`, `create`, and `virustotal` report deferred unsupported features. `--global` and uninstall `--purge` report explicit unsupported-feature errors. The upstream `shim` command is excluded.

### Configuration and buckets

Configuration lives at `$XDG_CONFIG_HOME/scoop/config.json`, with `~/.config` as the config-home default. The package root is selected by `SCOOP`, then `root_path`, then `~/scoop`; `cache_path` defaults to `<root>/cache`. Config values retain strings, with `true`/`false` converted to booleans. `config rm <name>` and the `null`/`$null` sentinels remove a value. Numeric, array-like and object-like arguments remain strings, matching upstream CLI conversion. Empty `SCOOP` and empty/relative `XDG_CONFIG_HOME` fall back to their defaults; an empty `HOME` uses the OS account home. Malformed configuration reports the exact file to repair and preserves its original bytes. Supported options include `default_architecture`, `use_git_history`, `force_update`, `show_update_log`, and the upstream `hold_update_until` name. Deferred operational integrations report unsupported config features.

Add a local Git repository with `scoop bucket add fixtures /absolute/repo`, or supply a remote repository URL. Manifests are scanned recursively in `bucket/`, or the repository root for legacy layouts. Bucket clones are staged before publication and mutations use the root lock. Removal requires a registered clean repository; unrelated directories, local modifications, and untracked files are preserved.

`known-buckets.json` is the Linux repository registry. The canonical `main` bucket is [`https://github.com/zhzy0077/scoop-linux-pkgs`](https://github.com/zhzy0077/scoop-linux-pkgs), with lookup priority. `scoop bucket add main` uses that default. An explicit repository argument takes precedence, followed by `SCOOP_MAIN_BUCKET`, `main_bucket_url`, and the registry default. `bucket known` displays the selected configured/default source; unknown names require an explicit repository argument.

### Manifest and source resolution

Publish/use [`schema.json`](schema.json) as the Linux JSON Schema. Runtime validation embeds this same schema and adds semantic checks: safe relative paths, exact basenames, duplicate/reserved executable names, canonical architecture keys, dependency names, resolved URL/hash pairing, and archive-only extraction mapping order. `checkver` and `autoupdate` remain inert authoring metadata. Architecture-less manifests assert architecture-independent artifacts; native package authors must declare architecture entries.

Operands accept `package`, `bucket/package`, JSON paths, and HTTP(S) manifest URLs. Add `@version` to select the current matching manifest or an exact version in bucket Git history. File/URL qualifiers require their supplied manifest to declare the version. Source information includes bucket, commit and repository-relative path, or file/URL provenance. `cat` preserves manifest bytes, including BOMs and historical content; valid JSON with unsupported installation fields is displayed with a validation warning on stderr. `info -v` displays metadata independently of installability and uses stable human-readable provenance.

Explicit bucket operands select that bucket. Bare installed names use the saved bucket/URL source; other bare names use known-registry precedence followed by alphabetical local buckets. Multiple matches are exposed by `info`. A selected bucket's version is resolved within that bucket's history. There is no autoupdate execution for missing historical versions.

Architecture selection uses `--arch`, then `default_architecture`, then native host detection. Linux aliases normalize to Scoop names; 32-bit ARM remains distinct. Manifest entries match exactly and inherit missing fields from top level. Install activation requires the selected native architecture; download and dependency inspection accept foreign targets. Native ELF content additionally requires an architecture entry and a matching known ELF CPU/class; Windows MZ artifacts are rejected before activation.

## Shared modules

- `cli` and `commands`: documented syntax, rendering and command dispatch.
- `config`: root/cache resolution, typed config, atomic metadata and mutation locks.
- `buckets`: Git subprocesses, layouts, discovery and safe management.
- `manifest`: schema validation, supported features, target artifacts and extraction mapping.
- `platform`: canonical architectures and native activation policy.
- `resolve`: source precedence, exact manifests, path-filtered NUL-separated Git-history walks, and clean/dirty provenance.
- `network` and `download`: bounded manifest requests, streaming artifacts, SHA-256 verification, cookies, atomic cache publication and verified-inode readers.
- `archive`: ZIP/tar.gz/tar.xz, raw files, archive-only extraction mappings, contained links and hardlinks, mode preservation.
- `state`, `lifecycle`, and `links`: dependency planning, versioned snapshots/records, native activation, basename symlinks and ownership-aware removal.
- `journal`: durable prepared/committed activation records, force-reinstall backup, rollback and startup recovery.
- `maintenance` and `version`: refresh timing, upstream-compatible version comparison, updates/status/holds, retained-version activation, cleanup and cache controls.
- `self_update`: catalog compatibility, verified bootstrap, native release activation and updated-executable continuation.
- `scoopfile` and `diagnostics`: source/config/hold round trips, per-entry import errors, installation checks and homepage opening.

## Installing and running packages

`install` plans dependencies in topological order, deduplicates package names and diagnoses cycles before artifact fetching. `--independent` selects only the requested app. Already installed unqualified packages are skipped; `app@version` or a qualified manifest path can install a different version while retaining the old one. Each completed package commits independently and later failures are reported together.

Downloads accept HTTPS, HTTP and local `file://` URLs. HTTPS is the release/bucket-authoring default; HTTP supports upstream-compatible endpoints and local HTTP fixtures, and `file://` supports explicit offline repositories. Manifests are trusted package inputs. Manifest responses have a 4 MiB limit; redirects are bounded and HTTPS-to-HTTP downgrade redirects are rejected. Artifacts stream to temporary files and are verified before atomic cache publication. Cache reuse revalidates SHA-256 and holds the verified inode open during extraction. `download --force` refreshes a cache entry; `install --no-cache` neither reads nor publishes cache entries. `--skip-hash-check` explicitly disables checksum verification. Automatic refresh is due after three hours (or an absent/invalid `last_update`) and runs before manifest planning; install/download support `--no-update-scoop` and warn when refresh is due. Each refresh records its attempt time, including failures, to bound retries. Catalog/bucket failures warn while package operations continue from local manifests; explicit operand-free `update` reports a failing refresh status after attempting all buckets.

ZIP, tar.gz and tar.xz archives, including uppercase extensions, retain executable file modes. Raw downloads retain their exact URL/fragment basename. Archive extraction rejects escaping paths, special files, escaping/cyclic links, and writes through symlinks. Archive entry names use forward-slash paths and reject backslashes; duplicate regular-file entries conflict and preserve the first entry. Hardlinks remain within the managed tree. Extraction mappings consume entries only for archives. Declared binaries gain execute permission, preserve their exact basenames/extensions, and receive ordinary relative symlinks in `<root>/bin`. Package-supplied executable wrappers run directly with inherited arguments, environment, working directory, streams, signals and status.

Installed versions save the exact manifest and a format-versioned `scoop-install.json` with source, Git commit/path/dirty status, architecture, hold state, timestamp and declared binaries. Current-link replacement and executable reconciliation are atomic per link. Before activation, files/directories and a prepared transaction are synced to disk. Prepared transactions roll back on ordinary errors or the next invocation; committed transactions finish backup/staging cleanup. Forced replacement retains the original directory in a journal-owned backup until commit. Recovery takes the root lock, ignores unpublished temporary files, isolates corrupt/conflicting journals per app, and continues recovering healthy transactions. Warnings name the journal and recovery steps. Mutations of an affected app wait for the root lock and remain blocked while its journal needs repair; read-only commands and unrelated apps remain usable. Conflicting user replacements are preserved for explicit resolution. Replacing a command owned by another installed package is reported. Unmanaged files/symlinks and unrelated root directories are preserved. Uninstall removes all owned versions and executable links; dependencies and external XDG application data remain owned by their users/applications.

`prefix` prints only the absolute active directory. `which` prints the resolved managed executable path and returns status 2 for missing/broken commands. `depends` prints the dependency installation order, including the requested package. `list` uses familiar Name/Version/Source/Updated/Info columns with RFC 3339 timestamps and per-entry warnings for stray or corrupt app entries. The CLI does not modify shell environment settings.

## Maintenance and recovery

`update` refreshes Scoop and clean Git buckets; bucket pulls require fast-forward and preserve dirty worktrees. Local authoring repositories without an origin are reported and skipped. `update app` selects its saved source, installs missing dependencies, preserves previous versions, and reconciles added/removed commands. `update '*'` and `--all` select installed apps. `--force` reinstalls the selected version; held apps stay held even with force. `force_update=true` also permits version downgrades. `--independent`, `--no-cache`, `--skip-hash-check` and `--quiet` retain their scoped meanings.

`status --local` uses local metadata. Ordinary status checks release metadata and fetches bucket refs, warns about remote changes, and prints the local package table with holds, missing dependencies and unavailable manifests. It preserves bucket worktrees, Scoop activation and `last_update`; network failures warn and fall back to the local table. `hold`/`unhold` atomically update installation records. `hold scoop` sets the upstream `hold_update_until` key one day ahead; self-holds still allow bucket refresh. `reset app@version` switches to a validated native retained version; `reset app` restores/reclaims its command links. `reset '*'` and `--all` cover installed apps. `cleanup` removes inactive recognized versions; `--cache` also removes those apps' non-current downloads. Cache show/rm selects managed cache filenames, preserving unrelated files. Cleanup/reset/recovery leave external XDG application data untouched.

Fault injection is compiled only with `--features test-hooks`. Tests can select `SCOOP_TEST_FAILPOINT=prepared|backup|published|current|links|committed` and `SCOOP_TEST_CRASH=1` to interrupt activation. Normal builds contain no active failpoint handling or private-CA test override. Hook builds can use `SCOOP_TEST_CA_CERT` to trust the HTTPS fixture's certificate; TLS verification stays enabled.

## Bootstrap and self-update

Run a seed executable with `./scoop --bootstrap /absolute/release-catalog.json` (HTTPS and file catalog locations also work). This installer mode preserves the 24-command interface. It downloads and verifies the exact compatible native release, installs `apps/scoop/<version>/scoop`, creates the stable `bin/scoop` symlink, initializes the configured Linux main bucket, saves the catalog/main-bucket configuration and prints the absolute PATH directory.

[`release-catalog.schema.json`](release-catalog.schema.json) defines the versioned catalog. Entries are indexed by the compiler's exact Rust target triple and include canonical architecture, Linux OS, libc, ABI, minimum libc and SHA-256. Remote catalogs and release artifacts require HTTPS; local catalogs may use local absolute artifact paths or `file://` URLs. The same policy applies to environment/config-selected catalogs, and HTTPS redirects never downgrade to HTTP. The current build target is embedded at compile time. GNU targets check the running glibc version; static musl entries identify their ABI explicitly. Unsupported targets, incompatible CPU/libc/ABI, invalid checksums and mismatched executable versions fail before activation.

A local catalog example (replace all local paths and the checksum with real release inputs):

```json
{
  "format_version": 1,
  "version": "1.0.0",
  "main_bucket_url": "/absolute/linux-main-git-repository",
  "targets": {
    "aarch64-unknown-linux-gnu": {
      "architecture": "arm64", "os": "linux", "libc": "glibc", "abi": "gnu",
      "minimum_libc": "2.39", "url": "file:///absolute/releases/scoop-arm64",
      "sha256": "<64 hex SHA-256 digits>"
    }
  }
}
```

`release_catalog` or `SCOOP_RELEASE_CATALOG` selects update metadata; `SCOOP_INTERNAL_REFRESH_CONTINUE` is the process-handoff marker used to finish bucket refresh under the updated executable. Bootstrap selects its main source from `SCOOP_MAIN_BUCKET`, config `main_bucket_url`, catalog `main_bucket_url`, then the canonical registry default. A catalog can omit `main_bucket_url` to use that default. Bootstrap persists the selected source in config `main_bucket_url`; `scoop config rm main_bucket_url` removes that override. Self-update stages and verifies the ELF executable and its declared version, journals activation, then continues the original command under the updated executable. Bucket refresh and `last_update` completion occur in that updated process. Old Scoop releases remain until explicit cleanup. Hosted release artifact/catalog locations are separate release configuration. The checked-in `main/` payload forms the dedicated `scoop-linux-pkgs` repository; packaging tools also support independent local verification.

Tests use actual temporary Git histories, files, HTTP servers, malicious ZIP/tar fixtures and subprocess CLI invocation. Inside the native build container, `sh tests/build-distribution.sh /workspace/target/fedora` records exact Cargo test artifacts with test hooks enabled and builds an independent version-changed Rust release under `real-release2/`, reusing native compiled dependencies. Its HTTPS/multi-target/hold/concurrent/crash tests run in the distribution gate; they verify the actual updated Rust executable is the ancestor performing bucket refresh. Build the production release afterward with `cargo build --release --locked`, since the secondary build shares the builder's release output directory. `sh tests/run-distribution.sh /workspace/target/fedora` runs those integration artifacts and Bash/Zsh/Fish CLI checks in each compatible native Linux container; the lifecycle suites execute package wrappers and native ELF packages through each shell, check I/O/signals, and verify external XDG data survives actual uninstall.

## Scoopfiles and diagnostics

`export [-c]` writes JSON only to stdout, with upstream `buckets`/`apps` and optional `config`. App fields are `Name`, `Version`, `Source`, RFC 3339 `Updated`, and comma-separated `Info` (native architecture and optional `Held package`). Machine-specific root/cache/alias/refresh keys and the release-channel keys `release_catalog`/`main_bucket_url` are omitted and refused on import. Export returns nonzero if an app/bucket cannot be represented, while stdout remains valid JSON. File/URL source paths remain explicit; a local source must be accessible on the destination machine.

`import <file-or-http(s)-url>` applies supported config values, adds buckets, installs apps and restores holds. Bucket apps use the bucket's current manifest; `<auto-generated>` entries request their recorded version through normal history resolution. Import validates native architecture and source/name consistency before activation. Unsupported global/Info/config entries and individual failures are reported while other entries continue; any failed entry gives a failing exit status. Existing bucket names remain available for import; differing remotes warn and keep the existing clone. Failed adds produce explicit per-app bucket errors. Import uses the shared due-refresh timing policy before app planning. Configuration is reloaded before package planning.

`checkup` checks Git, the absolute bin directory in PATH, root/cache/config write access, managed directories, current/retained installation records, executable permissions and links. It prints each issue and fails when repair is required. `reset app` repairs managed executable links. `home app` passes its HTTP(S) homepage as one argument to `xdg-open` and reports launcher failures.

## Packaging a release

Build once on each native CPU in Podman, then execute those same bytes on both compatible distributions. Packaging derives the glibc symbol floor and required system libraries with readelf, validates target ELF headers, release URLs and the Git main source, and rejects a supplied libc floor that differs from the ELF. A package artifact URL serves the **raw executable**; packaging additionally emits a deterministic tar.gz and SHA256SUMS. Local catalogs are labelled `LOCAL VERIFICATION METADATA` in their generated README and use explicit local paths; they are not publication metadata.

```sh
# Inside each native builder:
cargo build --release --locked
readelf --version-info target/release/scoop
# With actual artifact locations and a real/local Git main bucket:
python3 scripts/package-release.py --version 0.1.0 \
  --main-bucket /absolute/linux-main --output /absolute/releases \
  --artifact x86_64-unknown-linux-gnu /absolute/x86/scoop auto file:///absolute/releases/scoop-0.1.0-x86_64-unknown-linux-gnu \
  --artifact aarch64-unknown-linux-gnu /absolute/arm/scoop auto file:///absolute/releases/scoop-0.1.0-aarch64-unknown-linux-gnu
(cd /absolute/releases && sha256sum -c SHA256SUMS)
# Supply the checksum for the native target's raw binary from trusted release metadata:
sh scripts/bootstrap.sh /absolute/releases/release-catalog.json file:///absolute/releases/scoop-0.1.0-x86_64-unknown-linux-gnu NATIVE_BINARY_SHA256
```

The bootstrap script requires Git, curl, CA certificates, POSIX sh/coreutils (including sha256sum, od and tr), and the native GNU runtime below. It verifies the trusted seed SHA, requires the catalog's native SHA to match it, and installs those seed bytes through the shared verification service. `SCOOP_BOOTSTRAP_SHA256` is its internal trust-binding parameter. A downloaded script and checksum must come from a trusted release channel. Direct seed mode (`scoop --bootstrap CATALOG`) uses the explicitly selected HTTPS/local catalog as its trust source.

Run the complete native real-package gate in an isolated new work directory:

```sh
python3 tests/release-lifecycle.py target/release/scoop x86_64-unknown-linux-gnu target/release-gate
# On arm64, use aarch64-unknown-linux-gnu; reuse that native binary on Ubuntu.
```

The canonical main-bucket URL is configured independently of release artifact/catalog hosting. Local catalogs and explicit Git sources exercise packaging/bootstrap/self-update without publishing release binaries. To publish the main bucket, use the contents of `main/` as the dedicated repository root; see [`main/README.md`](main/README.md).

### Native verification tools

The broad distribution integration suites additionally require the **OpenSSL CLI** for generating the private HTTPS fixture certificate, Python 3 for its TLS server/runner, Git, a native C compiler for the retained ELF-front-end fixtures, Bash/Zsh/Fish and `cmp` (diffutils). These are test-image tools; the Scoop/bootstrap runtime prerequisites are listed separately below. OpenSSL libraries alone do not provide the `openssl` command.

```sh
# Fedora native test image:
dnf install -y openssl python3 git gcc bash zsh fish diffutils
# Ubuntu native test image (no build-essential or Rust compiler needed for runtime checks):
apt-get install -y openssl python3 git gcc bash zsh fish diffutils
sh tests/check-distribution-tools.sh
```

Native builders also need Cargo/Rust with rustfmt and Clippy. `tests/run-distribution.sh` checks test-tool availability before executing any suite. Direct hook-enabled HTTPS test invocation reports the missing OpenSSL CLI explicitly. All HTTPS/update assertions remain required.

### Validated GNU runtime matrix

Supported validation targets are native x86-64 and arm64 on Fedora 45 and Ubuntu 26.04.1 LTS. X86-64 also runs on the Fedora 43 builder. Each native binary is reused unchanged on its other distributions. Release requirements are the catalog's computed glibc floor (currently 2.39), `libc.so.6`, `libm.so.6`, the native GNU ELF loader, and `libgcc_s.so.1`. liblzma is statically linked. Fedora supplies these through `glibc`/`libgcc`; Ubuntu through `libc6`/`libgcc-s1`. Clean-runtime bootstrap checks install only Git, curl, CA certificates and these baseline system/coreutils packages, with no compiler or Python required. `xdg-open` is additionally required for `home`.

`export -c` includes user configuration such as proxy credentials or tokens, matching the reference's explicit config export. Store the output as a private file and review it before sharing.
