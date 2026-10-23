use crate::{
    archive,
    config::{self, Config},
    download, links,
    manifest::{Manifest, Target},
    platform,
    resolve::{Resolved, Resolver},
    state::{self, Record},
};
use anyhow::{Context, Result, bail, ensure};
use std::{collections::HashSet, fs, os::unix::fs::PermissionsExt, path::Path};
#[derive(Clone)]
pub struct Planned {
    pub resolved: Resolved,
    pub target: Target,
}
pub fn plan(
    resolver: &Resolver<'_>,
    operand: &str,
    arch: &str,
    independent: bool,
    skip_hash: bool,
) -> Result<Vec<Planned>> {
    #[derive(Default)]
    struct Traversal {
        active: Vec<String>,
        done: HashSet<String>,
        out: Vec<Planned>,
    }
    fn visit(
        resolver: &Resolver<'_>,
        operand: &str,
        arch: &str,
        independent: bool,
        skip: bool,
        traversal: &mut Traversal,
    ) -> Result<()> {
        let resolved = resolver.resolve(operand)?;
        let name = resolved.name.clone();
        ensure!(
            !traversal.active.contains(&name),
            "dependency cycle: {} -> {name}",
            traversal.active.join(" -> ")
        );
        if traversal.done.contains(&name) {
            return Ok(());
        }
        state::version_name(resolved.manifest.version())?;
        let target = resolved.manifest.target(arch, skip)?;
        // Validate download names and format support before any mutation or network artifact fetch.
        for artifact in &target.artifacts {
            let filename = download::filename(&artifact.url)?;
            ensure!(
                ![".7z", ".rar", ".msi", ".tar.bz2", ".tar.zst"]
                    .iter()
                    .any(|ext| filename.ends_with(ext)),
                "unsupported artifact format: {filename}"
            );
        }
        traversal.active.push(name.clone());
        if !independent {
            for dependency in &target.dependencies {
                visit(resolver, dependency, arch, false, skip, traversal)?;
            }
        }
        traversal.active.pop();
        traversal.done.insert(name);
        traversal.out.push(Planned { resolved, target });
        Ok(())
    }
    let mut traversal = Traversal::default();
    visit(
        resolver,
        operand,
        arch,
        independent,
        skip_hash,
        &mut traversal,
    )?;
    Ok(traversal.out)
}
pub(crate) fn prepare_layout(config: &Config) -> Result<()> {
    fs::create_dir_all(&config.root)?;
    for part in ["apps", "bin", ".staging"] {
        state::managed_dir(&config.root.join(part))?;
    }
    Ok(())
}
pub(crate) fn validate_binaries(stage: &Path, manifest: &Manifest, target: &Target) -> Result<()> {
    archive::validate_tree(stage)?;
    // Native ELF content must be explicitly architecture-indexed, including undeclared executables.
    fn inspect(dir: &Path, has_arch: bool, arch: &str) -> Result<()> {
        use std::io::Read;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                inspect(&entry.path(), has_arch, arch)?;
            } else if ty.is_file() {
                let mut header = Vec::new();
                fs::File::open(entry.path())?
                    .take(20)
                    .read_to_end(&mut header)?;
                let count = header.len();
                ensure!(
                    !(count >= 2 && &header[..2] == b"MZ"),
                    "Windows artifacts are unsupported"
                );
                if count >= 4 && &header[..4] == b"\x7fELF" {
                    ensure!(
                        count >= 20 && [1, 2].contains(&header[4]) && [1, 2].contains(&header[5]),
                        "invalid ELF header"
                    );
                    let class = match arch {
                        "64bit" | "arm64" | "riscv64" => Some(2),
                        "32bit" | "arm" => Some(1),
                        _ => None,
                    };
                    if let Some(class) = class {
                        ensure!(
                            header[4] == class,
                            "artifact ELF class does not match selected architecture {arch}"
                        );
                    }
                    ensure!(
                        has_arch,
                        "native ELF artifacts require an architecture entry"
                    );
                    let machine = if header[5] == 2 {
                        u16::from_be_bytes([header[18], header[19]])
                    } else {
                        u16::from_le_bytes([header[18], header[19]])
                    };
                    let expected = match arch {
                        "64bit" => Some(62),
                        "32bit" => Some(3),
                        "arm64" => Some(183),
                        "arm" => Some(40),
                        "riscv64" => Some(243),
                        _ => None,
                    };
                    if let Some(expected) = expected {
                        ensure!(
                            machine == expected,
                            "artifact CPU does not match selected architecture {arch}"
                        );
                    }
                }
            }
        }
        Ok(())
    }
    inspect(
        stage,
        manifest.value.get("architecture").is_some(),
        &target.architecture,
    )?;
    for bin in &target.bins {
        let path = archive::resolve_link(stage, Path::new(&bin.path))?;
        let metadata = fs::metadata(&path)
            .with_context(|| format!("declared binary missing: {}", bin.path))?;
        ensure!(
            metadata.is_file(),
            "declared binary must be a file: {}",
            bin.path
        );
        fs::set_permissions(
            path,
            fs::Permissions::from_mode((metadata.permissions().mode() & 0o777) | 0o111),
        )?;
    }
    Ok(())
}
pub fn install_one(config: &Config, planned: &Planned, options: download::Options) -> Result<()> {
    install_version(config, planned, options, false, false, false)
}
pub fn install_version(
    config: &Config,
    planned: &Planned,
    options: download::Options,
    force: bool,
    quiet: bool,
    respect_hold: bool,
) -> Result<()> {
    platform::select(
        Some(&planned.target.architecture),
        None,
        &platform::host()?,
        true,
    )?;
    let _lock = config::mutation_lock(&config.root)?;
    prepare_layout(config)?;
    crate::journal::recover_locked(config)?;
    let app = &planned.resolved.name;
    crate::journal::ensure_clear(config, app)?;
    let version = planned.resolved.manifest.version();
    state::version_name(version)?;
    let app_dir = state::app_dir(config, app)?;
    state::managed_dir(&app_dir)?;
    let previous = state::current(config, app)?;
    if let Some((dir, record)) = &previous {
        platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
        if respect_hold {
            if record.hold {
                if !quiet {
                    println!("{app} is held.");
                }
                return Ok(());
            }
            let old = dir
                .file_name()
                .and_then(|name| name.to_str())
                .context("installed version")?;
            if !force
                && crate::version::compare(old, version) != std::cmp::Ordering::Less
                && config
                    .values
                    .get("force_update")
                    .and_then(|value| value.as_bool())
                    != Some(true)
            {
                return Ok(());
            }
        }
    }
    let final_dir = app_dir.join(version);
    let exists = fs::symlink_metadata(&final_dir).is_ok();
    if exists {
        ensure!(
            fs::symlink_metadata(&final_dir)?.file_type().is_dir(),
            "unmanaged version conflict"
        );
        Record::load(&final_dir)?;
        if !force {
            if previous.as_ref().is_some_and(|(dir, _)| dir == &final_dir) {
                if !quiet {
                    println!("{app} is already installed; use scoop update.");
                }
                return Ok(());
            }
            return crate::maintenance::activate_retained_locked(config, app, version);
        }
    }
    links::preflight(config, &planned.target.bins)?;
    let stage = tempfile::Builder::new()
        .prefix(&format!("{app}-"))
        .tempdir_in(config.root.join(".staging"))?;
    for artifact in &planned.target.artifacts {
        let file = download::fetch(
            config,
            app,
            version,
            artifact,
            &planned.resolved.manifest,
            options,
        )?;
        archive::extract(&file, artifact, stage.path())?;
    }
    validate_binaries(stage.path(), &planned.resolved.manifest, &planned.target)?;
    for reserved in ["scoop-manifest.json", "scoop-install.json"] {
        ensure!(
            fs::symlink_metadata(stage.path().join(reserved))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "artifact attempts to supply reserved metadata: {reserved}"
        );
    }
    let mut record = Record::new(
        &planned.resolved,
        &planned.target.architecture,
        planned.target.bins.clone(),
    );
    record.hold = previous.as_ref().is_some_and(|(_, record)| record.hold);
    fs::write(
        stage.path().join("scoop-manifest.json"),
        &planned.resolved.manifest.raw,
    )?;
    config::atomic_json(&stage.path().join("scoop-install.json"), &record)?;
    let old_bins = previous
        .as_ref()
        .map(|(_, r)| r.bins.clone())
        .unwrap_or_default();
    crate::journal::activate(
        config,
        app,
        version,
        Some(stage.path()),
        exists,
        &old_bins,
        &record.bins,
    )?;
    if !quiet {
        println!(
            "Installed {app} ({version}) [{}] from {}",
            planned.target.architecture, planned.resolved.source
        );
    }
    for note in planned
        .resolved
        .manifest
        .notes(&app_dir.join("current"), &final_dir)
    {
        println!("{note}");
    }
    if let Some(suggestions) = planned
        .resolved
        .manifest
        .value
        .get("suggest")
        .and_then(|v| v.as_object())
    {
        for (label, packages) in suggestions {
            println!(
                "Suggested ({label}): {}",
                crate::manifest::strings(Some(packages)).join(", ")
            );
        }
    }
    Ok(())
}
pub fn install(
    config: &Config,
    apps: &[String],
    arch: &str,
    independent: bool,
    options: download::Options,
) -> Result<()> {
    let resolver = Resolver::new(config);
    let mut completed = HashSet::new();
    let mut failures = Vec::new();
    for operand in apps {
        let result: Result<()> = (|| {
            let (input, version) = crate::resolve::split_version(operand)?;
            if version.is_none()
                && !input.contains("://")
                && !input.ends_with(".json")
                && !input.starts_with(['.', '/'])
            {
                let name = state::operand_name(input)?;
                crate::journal::check(config, name)?;
                if let Some((_, record)) = state::current(config, name)? {
                    platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
                    println!("{name} is already installed; use scoop update.");
                    return Ok(());
                }
            }
            let planned = plan(&resolver, operand, arch, independent, options.skip_hash)?;
            let requested = planned
                .last()
                .context("package missing")?
                .resolved
                .name
                .clone();
            for package in planned {
                let name = &package.resolved.name;
                crate::journal::check(config, name)?;
                if completed.contains(name) {
                    continue;
                }
                if let Some((_, record)) = state::current(config, name)? {
                    platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
                    let selected =
                        state::app_dir(config, name)?.join(package.resolved.manifest.version());
                    if name != &requested || version.is_none() || selected.exists() {
                        if selected.exists() {
                            let saved = Record::load(&selected)?;
                            platform::select(
                                Some(&saved.architecture),
                                None,
                                &platform::host()?,
                                true,
                            )?;
                        }
                        println!("{name} is already installed; use scoop update.");
                        completed.insert(name.clone());
                        continue;
                    }
                }
                install_one(config, &package, options)?;
                completed.insert(name.clone());
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{operand}: {error:#}");
            failures.push(operand.clone());
        }
    }
    ensure!(
        failures.is_empty(),
        "installation failed: {}",
        failures.join(", ")
    );
    Ok(())
}
pub fn uninstall(config: &Config, apps: &[String]) -> Result<()> {
    let _lock = config::mutation_lock(&config.root)?;
    crate::journal::recover_locked(config)?;
    let mut failures = Vec::new();
    for operand in apps {
        let result = (|| {
            let app = state::operand_name(operand)?;
            crate::journal::ensure_clear(config, app)?;
            let app_dir = state::app_dir(config, app)?;
            let (_, active) = state::current(config, app)?.context("package is not installed")?;
            let mut versions = Vec::new();
            for entry in fs::read_dir(&app_dir)? {
                let entry = entry?;
                if entry.file_name() == "current" {
                    continue;
                }
                ensure!(
                    entry.file_type()?.is_dir(),
                    "unrelated file in app directory: {}",
                    entry.path().display()
                );
                state::version_name(entry.file_name().to_str().context("version name")?)?;
                Record::load(&entry.path())?;
                versions.push(entry.path());
            }
            for bin in &active.bins {
                links::remove_owned(config, app, bin)?;
            }
            for dir in versions {
                let record = Record::load(&dir)?;
                for bin in record.bins {
                    links::remove_owned(config, app, &bin)?;
                }
                fs::remove_dir_all(dir)?;
            }
            fs::remove_file(app_dir.join("current"))?;
            fs::remove_dir(app_dir)?;
            println!("Uninstalled {app}.");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{operand}: {error:#}");
            failures.push(operand.clone());
        }
    }
    if !failures.is_empty() {
        bail!("uninstall failed: {}", failures.join(", "))
    }
    Ok(())
}
pub fn download(
    config: &Config,
    apps: &[String],
    arch: &str,
    options: download::Options,
) -> Result<()> {
    let resolver = Resolver::new(config);
    let mut failures = Vec::new();
    for operand in apps {
        let result: Result<()> = (|| {
            let mut planned = plan(&resolver, operand, arch, true, options.skip_hash)?;
            let package = planned.pop().context("package missing")?;
            let _lock = config::mutation_lock(&config.root)?;
            crate::journal::recover_locked(config)?;
            crate::journal::ensure_clear(config, &package.resolved.name)?;
            for artifact in &package.target.artifacts {
                let file = download::fetch(
                    config,
                    &package.resolved.name,
                    package.resolved.manifest.version(),
                    artifact,
                    &package.resolved.manifest,
                    options,
                )?;
                println!("{}", file.path.display());
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{operand}: {error:#}");
            failures.push(operand.clone());
        }
    }
    ensure!(
        failures.is_empty(),
        "download failed: {}",
        failures.join(", ")
    );
    Ok(())
}
