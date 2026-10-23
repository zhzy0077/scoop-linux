# Scoop on Linux

A user-scoped Linux package manager in Rust, installed as `scoop`. [`DESIGN.md`](DESIGN.md) describes the locked MVP contract.

## Linux MVP

All 24 scoped commands are implemented: discovery, verified native package lifecycle, maintenance/recovery, Scoopfiles and diagnostics. Package manifests are maintained separately in the canonical [`main` bucket](https://github.com/zhzy0077/scoop-linux-pkgs).

## Build and use

Build with Rust/Cargo and Git installed:

```sh
cargo build --release --locked
./target/release/scoop help
./target/release/scoop install main/jq
~/scoop/bin/jq --version
```

Run `scoop <command> --help` for syntax and options. Add the selected Scoop root's `bin` directory to PATH for direct command use.

### Command contract

| Command | Syntax |
| --- | --- |
| help | `help [command]` |
| config | `config [rm] [name] [value]` |
| bucket | `bucket add <name> [repo]`, `bucket list`, `bucket known`, `bucket rm <name>` |
| search | `search [regex]` |
| info | `info <app> [-v/--verbose]` |
| cat | `cat <app>` |
| install | `install <apps>... [-i/--independent] [-k/--no-cache] [-s/--skip-hash-check] [-u/--no-update] [-a/--arch <architecture>]` |
| uninstall | `uninstall <apps>...` |
| list | `list [query]` |
| prefix | `prefix <app>` |
| which | `which <command>` |
| depends | `depends <app> [-a/--arch <architecture>]` |
| download | `download <apps>... [-f/--force] [-s/--skip-hash-check] [-u/--no-update] [-a/--arch <architecture>]` |
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

Configuration lives at `$XDG_CONFIG_HOME/scoop/config.json`, with `~/.config` as the config-home default. The package root is selected by `SCOOP`, then `root_path`, then `~/scoop`; `cache_path` defaults to `<root>/cache`. Config values retain strings, with `true`/`false` converted to booleans. `config rm <name>` and the `null`/`$null` sentinels remove a value. Numeric, array-like and object-like arguments remain strings, matching upstream CLI conversion. Empty `SCOOP` and empty/relative `XDG_CONFIG_HOME` fall back to their defaults; an empty `HOME` uses the OS account home. Malformed configuration reports the exact file to repair and preserves its original bytes. Supported options include `default_architecture`, `use_git_history`, `force_update`, `show_update_log`, and `main_bucket_url`. Deferred operational integrations report unsupported config features.

Add a local Git repository with `scoop bucket add fixtures /absolute/repo`, or supply a remote repository URL. Manifests are scanned recursively in `bucket/`, or the repository root for legacy layouts. Bucket clones are staged before publication and mutations use the root lock. Removal requires a registered clean repository; unrelated directories, local modifications, and untracked files are preserved.

`known-buckets.json` is the Linux repository registry. The canonical `main` bucket is [`https://github.com/zhzy0077/scoop-linux-pkgs`](https://github.com/zhzy0077/scoop-linux-pkgs), with lookup priority. The main bucket is added automatically on first package use. `scoop bucket add main` also uses that default. An explicit repository argument takes precedence, followed by `SCOOP_MAIN_BUCKET`, `main_bucket_url`, and the registry default. `bucket known` displays the selected configured/default source; unknown names require an explicit repository argument.

### Manifest and source resolution

Publish/use [`schema.json`](schema.json) as the Linux JSON Schema. Runtime validation embeds this same schema and adds semantic checks: safe relative paths, exact basenames, duplicate executable names, canonical architecture keys, dependency names, resolved URL/hash pairing, and archive-only extraction mapping order. `checkver` and `autoupdate` remain inert authoring metadata. Architecture-less manifests assert architecture-independent artifacts; native package authors must declare architecture entries.

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
- `scoopfile` and `diagnostics`: source/config/hold round trips, per-entry import errors, installation checks and homepage opening.

## Installing and running packages

`install` plans dependencies in topological order, deduplicates package names and diagnoses cycles before artifact fetching. `--independent` selects only the requested app. Already installed unqualified packages are skipped; `app@version` or a qualified manifest path can install a different version while retaining the old one. Each completed package commits independently and later failures are reported together.

Downloads accept HTTPS, HTTP and local `file://` URLs. HTTPS is the release/bucket-authoring default; HTTP supports upstream-compatible endpoints and local HTTP fixtures, and `file://` supports explicit offline repositories. Manifests are trusted package inputs. Manifest responses have a 4 MiB limit; redirects are bounded and HTTPS-to-HTTP downgrade redirects are rejected. Artifacts stream to temporary files and are verified before atomic cache publication. Cache reuse revalidates SHA-256 and holds the verified inode open during extraction. `download --force` refreshes a cache entry; `install --no-cache` neither reads nor publishes cache entries. `--skip-hash-check` explicitly disables checksum verification. Automatic refresh is due after three hours (or an absent/invalid `last_update`) and runs before manifest planning; install/download support `--no-update` and warn when refresh is due. Each refresh records its attempt time, including failures, to bound retries. Bucket failures warn while package operations continue from local manifests; explicit operand-free `update` reports a failing refresh status after attempting all buckets.

ZIP, tar.gz and tar.xz archives, including uppercase extensions, retain executable file modes. Raw downloads retain their exact URL/fragment basename. Archive extraction rejects escaping paths, special files, escaping/cyclic links, and writes through symlinks. Archive entry names use forward-slash paths and reject backslashes; duplicate regular-file entries conflict and preserve the first entry. Hardlinks remain within the managed tree. Extraction mappings consume entries only for archives. Declared binaries gain execute permission, preserve their exact basenames/extensions, and receive ordinary relative symlinks in `<root>/bin`. Package-supplied executable wrappers run directly with inherited arguments, environment, working directory, streams, signals and status.

Installed versions save the exact manifest and a format-versioned `scoop-install.json` with source, Git commit/path/dirty status, architecture, hold state, timestamp and declared binaries. Current-link replacement and executable reconciliation are atomic per link. Before activation, files/directories and a prepared transaction are synced to disk. Prepared transactions roll back on ordinary errors or the next invocation; committed transactions finish backup/staging cleanup. Forced replacement retains the original directory in a journal-owned backup until commit. Recovery takes the root lock, ignores unpublished temporary files, isolates corrupt/conflicting journals per app, and continues recovering healthy transactions. Warnings name the journal and recovery steps. Mutations of an affected app wait for the root lock and remain blocked while its journal needs repair; read-only commands and unrelated apps remain usable. Conflicting user replacements are preserved for explicit resolution. Replacing a command owned by another installed package is reported. Unmanaged files/symlinks and unrelated root directories are preserved. Uninstall removes all owned versions and executable links; dependencies and external XDG application data remain owned by their users/applications.

`prefix` prints only the absolute active directory. `which` prints the resolved managed executable path and returns status 2 for missing/broken commands. `depends` prints the dependency installation order, including the requested package. `list` uses familiar Name/Version/Source/Updated/Info columns with RFC 3339 timestamps and per-entry warnings for stray or corrupt app entries. The CLI does not modify shell environment settings.

## Maintenance and recovery

`update` refreshes Scoop and clean Git buckets; bucket pulls require fast-forward and preserve dirty worktrees. Local authoring repositories without an origin are reported and skipped. `update app` selects its saved source, installs missing dependencies, preserves previous versions, and reconciles added/removed commands. `update '*'` and `--all` select installed apps. `--force` reinstalls the selected version; held apps stay held even with force. `force_update=true` also permits version downgrades. `--independent`, `--no-cache`, `--skip-hash-check` and `--quiet` retain their scoped meanings.

`status --local` uses local metadata. Ordinary status fetches bucket refs, warns about remote changes, and prints the local package table with holds, missing dependencies and unavailable manifests. It preserves bucket worktrees, Scoop activation and `last_update`; network failures warn and fall back to the local table. `hold`/`unhold` atomically update installation records. `hold scoop` sets the installed package hold until `unhold scoop`. `reset app@version` switches to a validated native retained version; `reset app` restores/reclaims its command links. `reset '*'` and `--all` cover installed apps. `cleanup` removes inactive recognized versions; `--cache` also removes those apps' non-current downloads. Cache show/rm selects managed cache filenames, preserving unrelated files. Cleanup/reset/recovery leave external XDG application data untouched.

## Installing and upgrading Scoop

Download the native `scoop` executable for your architecture, mark it executable, and initialize the installation:

```sh
chmod +x ./scoop
./scoop install scoop
export PATH="$HOME/scoop/bin:$PATH"
```

The main bucket is added automatically on first package use, and Scoop installs through the normal package lifecycle. An explicit repository argument to `bucket add main`, `SCOOP_MAIN_BUCKET`, or config `main_bucket_url` selects a custom main bucket. Its installed source is recorded in the package metadata.

Scoop is a regular package:

```sh
scoop update             # refresh bucket manifests
scoop update scoop       # upgrade Scoop
scoop update --all       # upgrade every installed package, including Scoop
scoop hold scoop
scoop unhold scoop
scoop reset scoop@0.0.1  # activate a retained version
scoop cleanup scoop
```

The running command finishes using its existing executable; the next invocation uses the newly activated version. Package holds, status, export/import, retained versions, and uninstall apply to Scoop. Automatic bucket refresh runs before package planning when due. `-u/--no-update` skips that refresh for install/download.

## Scoopfiles and diagnostics

`export [-c]` writes JSON only to stdout, with upstream `buckets`/`apps` and optional `config`. App fields are `Name`, `Version`, `Source`, RFC 3339 `Updated`, and comma-separated `Info` (native architecture and optional `Held package`). Machine-specific root/cache/alias/refresh keys and the source key `main_bucket_url` are omitted and refused on import. Export returns nonzero if an app/bucket cannot be represented, while stdout remains valid JSON. File/URL source paths remain explicit; a local source must be accessible on the destination machine.

`import <file-or-http(s)-url>` applies supported config values, adds buckets, installs apps and restores holds. Bucket apps use the bucket's current manifest; `<auto-generated>` entries request their recorded version through normal history resolution. Import validates native architecture and source/name consistency before activation. Unsupported global/Info/config entries and individual failures are reported while other entries continue; any failed entry gives a failing exit status. Existing bucket names remain available for import; differing remotes warn and keep the existing clone. Failed adds produce explicit per-app bucket errors. Import uses the shared due-refresh timing policy before app planning. Configuration is reloaded before package planning.

`checkup` checks Git, the absolute bin directory in PATH, root/cache/config write access, managed directories, current/retained installation records, executable permissions and links. It prints each issue and fails when repair is required. `reset app` repairs managed executable links. `home app` passes its HTTP(S) homepage as one argument to `xdg-open` and reports launcher failures.

## Development checks

Run development checks with Rust/Cargo, rustfmt, and Clippy:

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
```

### Tagged releases

Set the package version in `Cargo.toml` and `Cargo.lock`, commit it, then push a matching tag with an optional `v` prefix (for example, `git tag 0.0.2 && git push origin 0.0.2`). The release workflow checks and builds both native architectures on Ubuntu 24.04, then publishes a GitHub Release with `scoop-x86_64-unknown-linux-gnu`, `scoop-aarch64-unknown-linux-gnu`, `scoop.json`, and `SHA256SUMS`. Tags containing a prerelease suffix produce a GitHub prerelease. Each production binary is checked against the glibc 2.39 baseline and allowed runtime libraries. The generated `scoop.json` is a normal package manifest with architecture-specific URLs and checksums.

The main bucket's `scoop.json` uses its existing Excavator automation to track stable releases and verify the release's `SHA256SUMS`. Publish the main-bucket manifest alongside this package-lifecycle change. The release workflow uses the repository's automatic `GITHUB_TOKEN` with release write permission in the publishing job.

### Validated GNU runtime matrix

Supported validation targets are native x86-64 and arm64 on Fedora 45 and Ubuntu 26.04.1 LTS. X86-64 also runs on the Fedora 43 builder. Each native binary is reused unchanged on its other distributions. Release runtime requirements are glibc 2.39 or newer, `libc.so.6`, `libm.so.6`, the native GNU ELF loader, and `libgcc_s.so.1`. liblzma is statically linked. Fedora supplies these through `glibc`/`libgcc`; Ubuntu through `libc6`/`libgcc-s1`. Installing Scoop requires Git and a compatible native runtime; the selected main bucket supplies its package manifest and checksum. `xdg-open` is additionally required for `home`.

`export -c` includes user configuration such as proxy credentials or tokens, matching the reference's explicit config export. Store the output as a private file and review it before sharing.
