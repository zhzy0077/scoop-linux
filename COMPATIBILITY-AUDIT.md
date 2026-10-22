# Linux MVP compatibility audit

Contract: the unchanged [`DESIGN.md`](DESIGN.md). Upstream reference: Scoop snapshot `e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f`, as specified in that contract. This audit covers the supported Linux subset. Acceptance is recorded in [`MVP-CHECKLIST.md`](MVP-CHECKLIST.md); reproducible validation is in [`VERIFICATION.md`](VERIFICATION.md).

## Command and option coverage

[`tests/cli.rs`](tests/cli.rs) enumerates all 24 commands and every required short/long option. The following suites verify behavior through the same command handlers and shared services used by the CLI.

| Commands / required options | Direct behavioral coverage |
|---|---|
| help; config get/set/rm | `cli`, `config_architecture`, `additional`, `locking`, `review_regressions`; XDG/root/cache precedence, typed CLI values, corruption preservation and concurrent config writers |
| bucket add/list/known/rm; search; info `-v`; cat | `discovery`, `additional`, `review_regressions`, `main_bucket::main_default`; both bucket layouts, precedence/history/provenance, exact cat bytes, canonical default, overrides and safe removal |
| install `-i -k -s -u -a` | `lifecycle`, `lifecycle_edges`, `lifecycle_failure`, `refresh`, `refresh_review`, `config_architecture`, `download_http`; dependency planning, partial completion, native activation, cache/hash/refresh/arch policies |
| uninstall; list; prefix; which; depends `-a` | `lifecycle`, `lifecycle_history`, `review_phase2`, `review_regressions`, `config_architecture`; ownership, XDG preservation, resilient listing, executable basenames and foreign inspection |
| download `-f -s -u -a` | `download_http`, `cache_inode`, `refresh`, `cli`; refresh suppression, forced download, explicit skip verification, foreign download and verified-inode reuse |
| update `-f -i -k -s -q -a`; status `-l`; hold/unhold | `maintenance`, `refresh`, `refresh_review`, `self_update`, `recovery`; version/hold/force behavior, dependencies, options, refresh resilience, fetch-only/offline status and executable handoff |
| reset `-a`; cleanup `-a -k`; cache show/rm `-a` | `maintenance`, `refresh_review`, `recovery`, real-package gate; retained versions, command reclaiming, literal names including g++/dots and managed-only removal |
| export `-c`; import file/HTTP(S) | `scoopfile`, `scoopfile_http`, real-package gate; field names, config/source/hold/native semantics, current bucket versions, generated/history selectors, BOM/case handling, existing/failed buckets and individual errors |
| checkup; home | `scoopfile`; Git/PATH/write/record/executable/link diagnostics, canonical PATH, path-rich errors, one-argument HTTP(S) homepage opening and launcher exit status |

## Shared safety and state invariants

| Requirement | Evidence |
|---|---|
| Exact architecture entry and inheritance; top-level architecture independence | `manifest`, `config_architecture`, `cli`, `main_bucket`; native ELF CPU/class checks in lifecycle/self-update |
| Validated manifest subset; unsupported aliases/hooks/persist/global/purge | `manifest`, `cli`, `review_regressions`; flat bins, safe paths, duplicate/reserved basenames and explicit operational-field errors before package artifact fetching/activation |
| SHA-256, streaming/cache integrity and URL behavior | `download_http`, `cache_inode`, `lifecycle_failure`; atomic cache publication/revalidation, cookies, filename fragments and bounded manifest transport |
| ZIP/tar.gz/tar.xz modes, mappings and containment | `archive_security`, `lifecycle_edges`, `additional`, `review_regressions`; traversal/special-file/link/hardlink and duplicate-entry cases |
| Ordinary relative symlinks and execution semantics | `lifecycle`, `lifecycle_edges`, `lifecycle_history`, `review_phase2`; direct/Bash/Zsh/Fish execution, arguments, environment, working directory, streams, signals and collisions |
| Dependency order/shared names/cycles; version history | `lifecycle`, `lifecycle_history`, `discovery`, `maintenance` |
| Per-package recovery and safe concurrent mutations | `recovery`, `recovery_isolation`, `review_phase3`, `locking`; interrupted prepared/backup/published/current/links/committed state, force backup, corrupt/conflicting journal isolation, retained evidence and unrelated-app progress |
| Refresh timing and error behavior | `refresh`, `refresh_review`, `scoopfile`; refresh precedes planning, records attempt time and preserves local progress on failure. Import evidence directly asserts newer-origin selection and an advanced timestamp; network-attempt count is not asserted. |
| Bootstrap trust, release compatibility and secure transport | `bootstrap_trust`, `review_phase3`, `self_update`; native target/CPU/libc/ABI/version/hash requirements, trusted seed binding, HTTP/downgrade rejection, swapped-catalog negatives and safe activation |
| Independent Rust self-update | `tests/build-real-release.py`, `tests/support/self_update_real.rs`; actual second Rust executable, HTTPS/private CA, native/foreign/musl metadata selection, status without activation, holds/concurrent updaters and updated-process Git ancestry |
| Usable recovered stable Scoop | `self_update::real::independent_rust_self_activation_recovers_every_applicable_boundary`; all five applicable boundaries assert current/journal/staging state, run recovered `bin/scoop --version`, and use it to read/validate a real bucket manifest. Package force-backup has its own recovery coverage. |
| Truthful/reproducible native release packaging | `tests/packaging-gate.py`; both actual CPU binaries, computed GLIBC/NEEDED metadata, ELF/source/URL/input rejection, deterministic tar/checksum inventory and correct native selection |
| Ten real Linux packages and clean reconstruction | `main_bucket`, `tests/release-lifecycle.py`; HTTPS/hash/target/bin/license coherence, exact current/saved-manifest/record identities, reset/cleanup, holds, restored-version equality, executable invocation, XDG preservation and uninstall |
| Clean runtime and maintained source trust | `tests/clean-bootstrap.sh`, `tests/verify-main.py`, `main/VERIFICATION.json`; documented prerequisites, trusted installed bytes, native runtime compatibility and byte-identical fresh-cache regeneration with publisher checksums/provenance |
| Canonical main source/publication layout | `main_bucket::main_default`; known/add/discovery and bootstrap fallback, explicit/environment/config/catalog priority via isolated Git URL rewrites to the reviewed ten-manifest tree. [`main/`](main/README.md) is the dedicated repository payload. |

The distribution gate requires the independently built Rust artifact and runs exact feature-qualified executables from Cargo's recorded manifest. The test-tool preflight checks its explicit OpenSSL CLI/Python/compiler/shell prerequisites. Independent review also exercised production-feature two-release self-update on both native CPUs.

## Intentional compatibility choices

- Package activation is native-only. Foreign `download`/`depends` inspection remains explicit; architecture-less artifacts assert independence.
- Executables use exact Linux basenames and ordinary symlinks. Alias/argument tuples, shims, persistence, global installs and shell/environment integration remain outside this MVP.
- Archives use canonical forward-slash paths; duplicate regular-file entries conflict. Application-owned XDG data remains outside Scoop removal/recovery.
- Scoopfiles are bounded to 4 MiB and support optional `Architecture`/`Hold` fields. Import refuses machine/release-channel settings and conflicting installed-app sources; incomplete export returns nonzero with valid JSON.
- Ordinary bucket import selects its current manifest and then restores holds, including when an exported historical version differs. Existing bucket names remain usable with a warning on differing remotes.
- Status keeps missing dependencies in its Info column. Required-operand parser errors use Clap's usage exit status; homepage opening uses HTTP(S) `xdg-open`. Human-readable messages are familiar Linux equivalents, while cat/prefix/export preserve their documented machine-readable formats.
- `checkver` and `autoupdate` remain inert authoring metadata. The reviewed pinned maintenance script updates the main bucket.

## Public configuration

The canonical main source is `https://github.com/zhzy0077/scoop-linux-pkgs`; publish the contents of `main/` at that Git repository's root. Hosted release artifact/catalog URLs are separate configuration. Test catalogs containing file/container paths are labelled local verification metadata. Repository documentation and tests use current source paths and require no pre-consolidation commit history or maintainer-local report files.
