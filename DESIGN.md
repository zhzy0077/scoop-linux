Scoop on Linux — proposed MVP and high-level design
=================================================

Build a user-scoped Linux package manager in Rust, installed as `scoop`, with Scoop's command names, package lifecycle, Git buckets, and JSON manifests. The existing Cargo project is the implementation starting point.

The compatibility reference is `../Scoop` at commit `e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f`, particularly `libexec/`, `schema.json`, and the installation, dependency, bucket, and manifest libraries. This is a proposed scope; implementation has not started.

**Compatibility contract**

Match command syntax, option meanings, package selection, and observable state changes for supported features. Use familiar table columns and messages; preserve machine-readable formats for `export`, `prefix`, and `cat`. Shell examples quote wildcards: `scoop update '*'`.

The reference contains 28 top-level commands. This design includes 24 in the MVP and three deferred commands. The upstream `shim` command is excluded from the design. Deferred commands and options must return a specific unsupported-feature error until implemented.

Linux packages live in Linux-specific buckets using Scoop's JSON structure. Existing Windows buckets contain Windows artifacts and PowerShell installation logic; package authors must supply Linux artifacts and adapt installation instructions. Bucket names such as `main` resolve through this project's known-bucket registry.

**MVP command scope, in delivery order**

| Priority | Commands | Required behavior |
| --- | --- | --- |
| 1: Foundation and discovery | `help`, `config`, `bucket`, `search`, `info`, `cat` | Command help; config get/set/rm; bucket add/list/known/rm; search local manifests by package and binary name; display metadata or manifest content. |
| 2: Package lifecycle | `install`, `uninstall`, `list`, `prefix`, `which`, `depends` | Resolve packages and dependencies, install verified artifacts, expose executables, list installed state, locate installations and executable targets, remove managed installations. |
| 3: Maintenance | `update`, `status`, `hold`, `unhold`, `reset`, `cleanup`, `cache`, `download` | Refresh Scoop and buckets, update packages, report outdated packages, control holds, switch retained versions and restore executable symlinks, remove old versions, manage verified downloads. |
| 4: Reproducibility and diagnostics | `export`, `import`, `checkup`, `home` | Scoopfile round trips; diagnose Git/PATH/permissions/broken installations; open the package homepage with `xdg-open`. |
| After MVP | `alias`, `create`, `virustotal` | Custom subcommands; manifest creation; VirusTotal integration. |

Support the following options in the MVP, including their upstream short forms. Other commands follow their reference positional syntax.

| Commands | MVP options |
| --- | --- |
| `install` | `-i/--independent`, `-k/--no-cache`, `-s/--skip-hash-check`, `-u/--no-update`, `-a/--arch <architecture>` |
| `update` | `-f/--force`, `-i/--independent`, `-k/--no-cache`, `-s/--skip-hash-check`, `-q/--quiet`, `-a/--all` |
| `download` | `-f/--force`, `-s/--skip-hash-check`, `-u/--no-update`, `-a/--arch <architecture>` |
| `depends` | `-a/--arch <architecture>` |
| `cleanup` | `-a/--all`, `-k/--cache` |
| `reset`, `cache rm` | `-a/--all` |
| `status` | `-l/--local` |
| `info` | `-v/--verbose` |
| `export` | `-c/--config` |

The MVP has an architecture-agnostic core and a shell-agnostic command interface, with portable CLI packages delivered as standalone binaries, ZIP, tar.gz, or tar.xz archives. Git is required to manage buckets.

Detect the native host architecture; resolve the requested architecture in order of explicit `--arch`, `default_architecture`, then host architecture. Keep Scoop's `64bit`, `32bit`, and `arm64` names, normalizing unambiguous Linux aliases such as `x86_64`, `i686`, and `aarch64`. Treat 32-bit ARM as distinct from `arm64`. Permit additional canonical architecture keys, such as `riscv64`, without changing lifecycle code; these keys extend the upstream schema.

Architecture resolution follows these rules:

1. If `architecture` is present, require an entry matching the requested architecture exactly. An absent entry is an error, including when top-level download fields exist. For example, an arm64 host with only `architecture.64bit` fails resolution.
2. Within the matching entry, use its field value when present; otherwise inherit the corresponding top-level value. Validate the resulting URL/hash pairs together. Fields from another architecture entry are never selected.
3. If `architecture` is absent, top-level artifacts must be architecture-independent. Native binaries must declare an architecture entry, even for single-architecture packages.
4. Installation, update, reset, and import require the selected architecture to match the host. Reject foreign-architecture overrides or installed records before activation. `download --arch` and `depends --arch` may inspect an explicitly requested foreign target without installing it.

The MVP assumes native execution. CPU translation and emulation are outside its execution model.

Rust executables and native package artifacts are built for particular targets. Publish Scoop as a regular architecture-indexed package in the main bucket. Release binaries must satisfy the documented glibc baseline and runtime library requirements. Adding a target requires a compatible build and validation; package availability depends on its published artifacts. Running a selected artifact requires a compatible CPU and libc/ABI. Initial release validation covers x86-64 and arm64 Linux, with Ubuntu LTS and current Fedora coverage; the resolver accepts additional canonical architecture keys.

The next compatibility milestone adds global installs and `--global`, shell hooks and custom installers, shell environment integration (`env_add_path` and `env_set`), desktop shortcuts, the three deferred commands, additional archive/hash formats, nightly packages, autoupdate-generated historical manifests, and optional config integrations such as aria2, SQLite indexing, and custom Scoop source branches. PowerShell modules, Windows installers, and registry operations require an explicitly designed platform equivalent. The MVP rejects manifests that require unsupported execution features before downloading or modifying an installation.

Application data is owned by applications, using their normal Linux locations and inherited `XDG_*` settings. Scoop-managed `persist` mappings and `uninstall -p/--purge` are outside the MVP. Reject manifests requiring `persist` and reject `--purge` explicitly. Uninstall, cleanup, reset, and recovery operate only on Scoop-managed package files and metadata; XDG application data remains application-owned.

**Behavior that must stay familiar**

- Accept package names, `bucket/package`, local JSON files, and manifest URLs. Support multiple packages where the reference does. Match upstream bucket lookup precedence and expose the selected source in output.
- Resolve `package@version` and `bucket/package@version` from the current manifest or bucket Git history. A version-qualified file or URL succeeds when its manifest declares that version; otherwise report that a matching manifest is required.
- Install dependencies in topological order, deduplicate shared dependencies, and report cycles. Preserve Scoop's simple dependency-name model. Package names identify installations within a scope; two buckets supplying the same name compete for that installation.
- `update` without operands updates Scoop itself and refreshes buckets. `update <app>` updates the selected installation; `update '*'` or `--all` updates installed packages according to upstream hold and force behavior. Preserve old versions until cleanup.
- `reset app@version` activates an already installed version. `reset app` restores its current version's executable symlinks, including reclaiming command names from another package.
- `uninstall` removes the package's installed versions and owned executable symlinks. `cleanup` removes inactive versions. Dependencies remain installed until explicitly removed.
- Import/export preserve Scoopfile `buckets`, `apps`, and optional `config`. Match upstream fields including `Name`, `Version`, `Source`, `Updated`, and `Info` for apps, and `Name`/`Source` for buckets. Import follows upstream source selection and hold restoration; ordinary bucket entries install the bucket's current version. Report unsupported entries individually.

**Manifest contract**

Publish a Linux-specific JSON schema describing the supported subset and the differences below. Use existing Scoop field names and filename conventions. This MVP adds no new top-level manifest fields. Paths are case-sensitive, forward-slash paths relative to the package directory.

Require `version`, `homepage`, and `license`, matching the upstream schema. An installable manifest also needs resolved download URLs and, unless `--skip-hash-check` is used, a supported checksum for each download. Keep upstream's version-string validation; `nightly` execution remains deferred.

| Fields | MVP interpretation |
| --- | --- |
| `version`, `description`, `homepage`, `license` | Scoop metadata types; `license` accepts an identifier string or an object containing `identifier` and optional `url`. |
| `notes` | String or array of strings. Support literal `$dir` and `$original_dir` substitution with active and version-directory paths. Persistence-related substitution is outside the MVP. |
| `url`, `hash` | Strings or arrays paired in download order. Accept bare 64-hex SHA-256 or `sha256:<64-hex>`. Support Scoop's `#/filename` URL fragment convention. Other upstream checksum algorithms remain deferred. |
| `cookie` | Scoop's name/value object for download cookies. |
| `architecture.<architecture>` | Exact target selection and field inheritance as specified above. Supported per-architecture fields are `url`, `hash`, `extract_dir`, `bin`, and retained `checkver` metadata. |
| `extract_dir`, `extract_to` | String or array selecting an archive subtree and destination within the version directory. Pair entries in extracted-archive order, as upstream does; raw-file downloads do not consume extraction entries. `extract_to` stays top-level, matching upstream's schema. |
| `bin` | Scoop shim definitions: a target string or an array of strings and nested `[target, name, arguments]` entries. Resolve targets from the package, an explicit file, or PATH. Default names lowercase the filename and remove its final extension. Expose the resulting command through `~/scoop/bin`. |
| `depends`, `suggest` | `depends` is a package string or array; `suggest` maps suggestion labels to package strings or arrays. Dependencies remain top-level, as in upstream's schema. |
| `checkver`, `autoupdate` | Retain authoring metadata; execution is outside the MVP. |
| `$schema`, `##`, `_comment` | Accept schema identification and Scoop comment fields as metadata; validate against the local Linux schema. |
| `persist`, `env_add_path`, `env_set`, `installer`, `uninstaller`, `pre_install`, `post_install`, `pre_uninstall`, `post_uninstall`, `innosetup`, `psmodule`, `shortcuts` | Unsupported MVP installation features. Reject their use with a field-specific diagnostic. |

Reject unknown operational fields instead of silently skipping installation behavior. Validate architecture entries against the supported field set; extending architecture names does not permit arbitrary per-architecture properties. Keep authoring metadata inert.

This contract was reviewed against `../Scoop/schema.json`, `arch_specific` and `Get-SupportedArchitecture` in `lib/manifest.ps1`, `shim_def` and `show_notes` in `lib/install.ps1`, and `Invoke-Extraction` in `lib/decompress.ps1`. Upstream's runtime accepts per-architecture `extract_to` through its generic accessor, while its schema omits that property; the MVP follows the schema here. Scoop uses the same package-manifest schema as other native packages.

Preserve archive executable modes; add execute permission for manifest-declared binaries when needed. Reject absolute or escaping extraction paths and escaping archive links. Validate package names and manifest-relative paths before filesystem operations.

Shim definitions accept a target plus optional name and fixed arguments, matching upstream `shim_def` and `create_shims`. Fixed argument strings use POSIX quoting on Linux, are substituted for `$dir`, `$original_dir`, and `$persist_dir`, and precede user arguments. Generate per-version `.scoop-shims` metadata; keep extracted files unchanged. Use symlinks for packaged targets without fixed arguments and executable `exec` launchers for arguments or external targets. Reject duplicate exposed command names.

**Architecture**

Use a single Rust executable with small modules. Command handlers parse and render; shared services own package behavior so `install`, `update`, `download`, and `import` use the same resolution and verification rules.

```text
CLI parsing and output
        |
Command handlers
        |
Package lifecycle / dependency planning
        |
        +-- Bucket and manifest resolution ---- Git repositories / JSON
        +-- Download and extraction ---------- HTTPS / cache / archives
        +-- Installed state ------------------ manifests / install records
        +-- Linux integration ---------------- executable symlinks
        +-- Self-update ---------------------- release metadata / binaries
```

Use `clap` for command parsing, `serde`/`serde_json` for data, `reqwest` with Rustls for HTTP, `sha2` for checksums, and established archive libraries. Invoke system Git with argument arrays for bucket operations. Keep installed records and manifests as the source of truth; scan local bucket JSON for searches at MVP scale.

Suggested source modules: `cli`, `commands`, `config`, `platform`, `buckets`, `manifest`, `resolve`, `download`, `archive`, `state`, `lifecycle`, and `links`. The `platform` module owns host detection and architecture normalization.

**Filesystem and state**

Default to `~/scoop`, honoring `SCOOP` and `root_path` overrides with upstream-compatible precedence. Store configuration at `$XDG_CONFIG_HOME/scoop/config.json`, defaulting to `~/.config/scoop/config.json`. Support `cache_path` for a separate download cache.

```text
~/scoop/
  apps/
    scoop/<version>/scoop
    scoop/current -> <version>
    <app>/<version>/
      ...package files...
      scoop-manifest.json
      scoop-install.json
    <app>/current -> <version>
  buckets/<bucket>/bucket/<app>.json
  cache/
  bin/
    scoop -> ../apps/scoop/current/scoop
    rg -> ../apps/ripgrep/current/bin/rg
  .staging/
  .transactions/
  .lock
```

Also accept the reference's legacy bucket layout with manifests at the repository root. Save the exact installed manifest plus source bucket/URL, bucket commit, architecture, hold state, and installation timestamp. Internal record extensions carry a format version.

Serialize mutations with a per-root lock. Write metadata using temporary files and rename. Journal package activation so an interrupted operation can restore or complete its `current` link and executable symlink ownership on the next invocation. A multi-package command commits each package independently and reports failures at the end; completed dependencies remain installed.

**Installation and update flow**

1. Resolve configuration, source, version, architecture, and dependency order. Validate all required manifest features.
2. Acquire the mutation lock and recheck installed state. Download missing artifacts to temporary cache files, verify hashes, then publish complete cache entries. Revalidate cached content when reused.
3. Extract into a staging directory on the installation filesystem. Validate declared binary paths and executable permissions.
4. Save the manifest and installation record, move the staged version into `apps/<app>/<version>`, and journal activation.
5. Replace the `current` symlink atomically and reconcile executable symlinks for added, changed, and removed `bin` entries. Finalize the journal and report success.

The previous active version remains usable if download, verification, or extraction fails. Recovery restores managed links and metadata after interrupted activation. Force-reinstalling the active version requires a temporary backup until the replacement commits.

For command-name collisions between packages, match Scoop's installation/reset ownership behavior and report replacements. Track ownership through managed symlink targets so uninstall and update only remove links still owned by that package. An unmanaged file at the destination is a conflict requiring resolution before activation.

**Linux integration**

Expose each declared executable through an ordinary relative symlink, for example `~/scoop/bin/rg -> ../apps/ripgrep/current/bin/rg`. The operating system executes the package's binary or wrapper directly. Arguments, environment, working directory, streams, signals, and exit status follow normal Linux execution semantics. The MVP implementation consists of symlink management; package execution requires no Scoop launcher or per-command runtime metadata.

Installing the Scoop package creates a stable `scoop` symlink in `bin`. Add the absolute directory to `PATH`. Users configure that directory through their shell, session manager, or existing environment configuration. Commands work from any shell or direct process invocation; absolute paths also work. Changes to `current` immediately affect existing executable symlinks. A child process cannot change its parent shell's environment, so future shell-wide environment integration requires an explicit adapter.

The main bucket is added automatically on first package use. `scoop install scoop` installs Scoop through the standard package lifecycle. `scoop update` refreshes bucket manifests; `scoop update scoop` and `scoop update --all` upgrade Scoop as a normal package. Holds, inventory, status, reset, cleanup, Scoopfiles and uninstall apply uniformly. An in-flight command continues using its open executable; the next invocation uses the activated version. Automatic bucket refresh retains the three-hour timing policy and `--no-update` option.

**Implementation milestones and acceptance**

| Milestone | Deliverable | Acceptance |
| --- | --- | --- |
| 1 | CLI contract, config, buckets, manifest parsing, discovery | Every MVP command has documented syntax; local Git fixtures verify source resolution, architecture aliases/overrides, architecture-independent artifacts, exact-entry field inheritance, search, and errors. Reject arm64-to-64bit fallback even with top-level URLs; reject foreign installation overrides; permit explicit foreign-target downloads. Cover required metadata, checksum forms, supported field shapes, and mixed archive/raw-file extraction ordering. |
| 2 | Cache, downloads, extraction, dependencies, install/uninstall, executable symlinks | Install and execute fixture packages; validate dependency order, shim names and fixed arguments, executable modes, package-supplied wrappers, uninstall, checksum failures, and archive containment. Verify target/name/argument definitions, default names, external target lookup, duplicate names, and invalid definitions. Invoke the same executables directly and through Bash, Zsh, and Fish. Verify that externally stored XDG data survives uninstall. |
| 3 | Update/reset/hold, cleanup, locking, recovery, Scoop package upgrades | Exercise upgrades and retained-version switching; verify held-package behavior against upstream; inject failures around activation; validate concurrent mutations and a two-release Scoop package upgrade. |
| 4 | Scoopfile operations, diagnostics, Linux main bucket, release packaging | Recreate an installation from export, pass clean-machine lifecycle runs covering both validation architectures and distributions, and verify all scoped commands/options. |

Maintain a compatibility checklist keyed to each reference command and option. Derive behavioral fixtures from `../Scoop/test` and command implementations; use them for Linux state transitions and data formats. Where platform-neutral behavior is ambiguous, compare with the pinned reference on Windows before declaring parity.

Release the MVP with roughly ten maintained portable CLI manifests, for example ripgrep, fd, jq, bat, fzf, delta, zoxide, eza, yq, and just, subject to their available Linux artifacts. Include synthetic fixture packages for dependencies, architecture selection, XDG application data, and command-name collisions. The release gate is a complete install → run → update → reset → export/import → uninstall workflow, plus correct failure recovery.
