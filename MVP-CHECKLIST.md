# Linux MVP acceptance checklist

Contract: [`DESIGN.md`](DESIGN.md), preserved unchanged. Its SHA-256 is `a19fca5efe92e2a2fc9b19b196c7d35acefef4910f408c4436502118137bb3aa`.

The full locked MVP, test-tooling preflight, recovered-executable evidence and canonical main-bucket default/publication payload have passed independent Claude Sonnet 5.5 review, with no required findings remaining. Implementation used Pi with `github-copilot/gpt-6.1-sol`.

## Phase 1 — foundation and discovery

- [x] All 24 command syntaxes/options and explicit unsupported-feature diagnostics.
- [x] Configuration/root/cache precedence, typed CLI values and XDG configuration.
- [x] Git bucket management, standard/legacy layouts, source precedence and historical manifests.
- [x] Linux schema, required metadata, supported checksums and operational-field validation.
- [x] Exact architecture entries/inheritance, native activation and explicit foreign inspection.
- [x] Help, search, info and byte-preserving cat.

## Phase 2 — installation and execution

- [x] Verified streaming downloads, cache revalidation/atomic publication and cookies.
- [x] Standalone, ZIP, tar.gz and tar.xz artifacts; extraction mappings/modes/containment.
- [x] Dependency ordering, deduplication/cycles and independently completed package operations.
- [x] Versioned manifests/records, native ELF validation and ordinary basename symlinks.
- [x] Command ownership/collisions, uninstall/list/prefix/which/depends/download.
- [x] Direct/Bash/Zsh/Fish execution, arguments/streams/signals and XDG-data preservation.

## Phase 3 — maintenance and reliability

- [x] Refresh/update options, failed-attempt backoff and continued local package progress.
- [x] Fetch/check-only status with offline fallback; holds and force behavior.
- [x] Retained reset, command repair/reclaiming, managed cleanup and literal cache operands.
- [x] Refresh timing before planning, no-update suppression and explicit foreign downloads.
- [x] Durable journal recovery, app-scoped conflict isolation, force backup and mutation locks.
- [x] Trusted seed bootstrap, target-indexed releases and independent Rust self-update/handoff.
- [x] Crash/concurrency tests, including executing recovered stable Scoop after every applicable self-activation boundary.

## Phase 4 — reproducibility and distribution

- [x] Scoopfile config/source/hold/native-architecture round trips and per-entry failures.
- [x] Channel-key exclusion, existing-bucket handling and valid-JSON incomplete-export errors.
- [x] Checkup/home, canonical PATH diagnostics and quiet broken-pipe export.
- [x] Ten reviewed Linux manifests, both native architectures and recorded source trust.
- [x] Deterministic two-target packaging, checksums, bootstrap and runtime metadata validation.
- [x] Native install/run/update/reset/export/import/uninstall and clean-runtime gates.
- [x] Requirement/command compatibility audit against the pinned upstream reference.

## Final verification and configuration

- [x] Independent review of the locked MVP and all required implementation corrections.
- [x] Independent review of explicit test prerequisites and recovered-executable assertions.
- [x] Independent review of the canonical `main` default and publication payload follow-up.
- [x] Formatting, warnings-denied Clippy, native tests and real-package gates.
- [x] Exact native CPU/libc requirements; release artifact/catalog hosting kept separate.
- [x] Dedicated main-bucket payload: the contents of [`main/`](main/README.md) form the `scoop-linux-pkgs` repository root.
- [x] Current reproducible commands and evidence coverage: [`VERIFICATION.md`](VERIFICATION.md) and [`COMPATIBILITY-AUDIT.md`](COMPATIBILITY-AUDIT.md).

## Current release scope

The canonical main Git source is `https://github.com/zhzy0077/scoop-linux-pkgs`. Source/bucket publication and release artifact/catalog hosting are separate operations. Local test catalogs are explicitly labelled verification metadata.

Validated targets are native arm64 and x86-64 GNU Linux on Fedora 45 and Ubuntu 26.04.1 LTS, with additional x86 Fedora 43 builder coverage. The full gate covers the implemented command/option contract, package safety/lifecycle, independent self-update, recovered execution, dual-target packaging and pristine-runtime bootstrap.

This document records current reviewed delivery. Exact review snapshot provenance, command transcripts and artifact hashes are retained in the maintainer verification archive; repository usage and verification require only the current source tree.
