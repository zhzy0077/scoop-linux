use crate::{
    cli::{CacheCommand, Update},
    config::{self, Config},
    download, journal, lifecycle, manifest, platform,
    resolve::{self, Resolver},
    state::{self, Record},
    version,
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use std::{cmp::Ordering, fs, path::Path};
pub fn date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|v| v.with_timezone(&Utc))
        .or_else(|| {
            ["%Y-%m-%d", "%Y/%m/%d"]
                .iter()
                .find_map(|format| NaiveDate::parse_from_str(value, format).ok())
                .and_then(|v| Local.from_local_datetime(&v.and_hms_opt(0, 0, 0)?).single())
                .map(|v| v.with_timezone(&Utc))
        })
}
pub fn refresh_due(config: &Config) -> bool {
    config
        .values
        .get("last_update")
        .and_then(|v| v.as_str())
        .and_then(date)
        .is_none_or(|last| Utc::now().signed_duration_since(last).num_seconds() >= 3 * 3600)
}
pub fn automatic(config: &Config, disabled: bool) -> Result<()> {
    if refresh_due(config) {
        if disabled {
            eprintln!("Bucket refresh is due (--no-update).");
        } else if let Err(error) = refresh(config) {
            eprintln!("warning: automatic refresh failed: {error:#}");
        }
    }
    Ok(())
}
pub fn refresh(config: &Config) -> Result<()> {
    // Record the attempt before network I/O to bound retries after failures.
    config.set("last_update", Utc::now().to_rfc3339().into())?;
    let mut failures = Vec::new();
    let buckets = crate::buckets::Buckets::new(&config.root);
    for name in buckets.names()? {
        let result = (|| -> Result<()> {
            let _lock = config::mutation_lock(&config.root)?;
            journal::recover_locked(config)?;
            let repo = buckets.repo(&name)?;
            if crate::buckets::git(&repo, &["remote", "get-url", "origin"]).is_err() {
                eprintln!("Skipping local bucket {name}: no origin remote.");
                return Ok(());
            }
            ensure!(
                crate::buckets::git(&repo, &["status", "--porcelain", "--untracked-files=all"])?
                    .is_empty(),
                "bucket has local changes"
            );
            crate::buckets::git(&repo, &["pull", "--ff-only", "--no-rebase"])?;
            println!("Updated bucket {name}.");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("warning: {name}: {error:#}");
            failures.push(name);
        }
    }
    ensure!(
        failures.is_empty(),
        "refresh failed: {}",
        failures.join(", ")
    );
    Ok(())
}
fn selected(config: &Config, apps: &[String], all: bool) -> Result<Vec<String>> {
    if all || apps.iter().any(|a| a == "*") {
        Ok(state::installed(config)?
            .into_iter()
            .map(|(app, _, _)| app)
            .collect())
    } else {
        Ok(apps.to_vec())
    }
}
fn each(apps: Vec<String>, mut action: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let mut failed = Vec::new();
    for app in apps {
        if let Err(error) = action(&app) {
            eprintln!("{app}: {error:#}");
            failed.push(app);
        }
    }
    ensure!(failed.is_empty(), "operation failed: {}", failed.join(", "));
    Ok(())
}
pub fn hold(config: &Config, apps: &[String], held: bool) -> Result<()> {
    let _lock = config::mutation_lock(&config.root)?;
    journal::recover_locked(config)?;
    each(apps.to_vec(), |operand| {
        let app = state::operand_name(operand)?;
        journal::ensure_clear(config, app)?;
        let (dir, mut record) = state::current(config, app)?.context("package is not installed")?;
        record.hold = held;
        config::atomic_json(&dir.join("scoop-install.json"), &record)?;
        println!("{app} is {}.", if held { "held" } else { "unheld" });
        Ok(())
    })
}
pub fn activate_retained_locked(config: &Config, app: &str, version: &str) -> Result<()> {
    journal::ensure_clear(config, app)?;
    state::version_name(version)?;
    let previous = state::current(config, app)?.context("package is not installed")?;
    platform::select(
        Some(&previous.1.architecture),
        None,
        &platform::host()?,
        true,
    )?;
    let dir = state::app_dir(config, app)?.join(version);
    ensure!(
        fs::symlink_metadata(&dir)
            .with_context(|| format!("retained version {app}@{version} is not installed"))?
            .file_type()
            .is_dir(),
        "retained version directory is not managed"
    );
    let record = Record::load(&dir)?;
    platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
    let manifest = state::saved_manifest(&dir)?;
    ensure!(
        manifest.version() == version,
        "retained manifest/version mismatch"
    );
    let target = manifest.target(&record.architecture, true)?;
    ensure!(
        target.bins == record.bins,
        "retained manifest/bin record mismatch"
    );
    lifecycle::validate_binaries(&dir, &manifest, &target)?;
    journal::activate(
        config,
        app,
        version,
        None,
        false,
        &previous.1.bins,
        &record.bins,
    )?;
    println!("Reset {app} ({version}).");
    Ok(())
}
pub fn reset(config: &Config, apps: &[String], all: bool) -> Result<()> {
    let _lock = config::mutation_lock(&config.root)?;
    journal::recover_locked(config)?;
    each(selected(config, apps, all)?, |operand| {
        let (input, requested) = resolve::split_version(operand)?;
        let app = state::operand_name(input)?;
        journal::ensure_clear(config, app)?;
        let (current, _) = state::current(config, app)?.context("package is not installed")?;
        let version = requested.unwrap_or(
            current
                .file_name()
                .and_then(|s| s.to_str())
                .context("current version")?,
        );
        activate_retained_locked(config, app, version)
    })
}
fn outdated(config: &Config, old: &str, new: &str) -> bool {
    let order = version::compare(old, new);
    order == Ordering::Less
        || (config.values.get("force_update").and_then(|v| v.as_bool()) == Some(true)
            && order != Ordering::Equal)
}
pub fn status(config: &Config, local: bool) -> Result<()> {
    let mut needs_attention = false;
    if !local {
        let buckets = crate::buckets::Buckets::new(&config.root);
        for name in buckets.names()? {
            let result = (|| -> Result<()> {
                let _lock = config::mutation_lock(&config.root)?;
                let repo = buckets.repo(&name)?;
                if crate::buckets::git(&repo, &["remote", "get-url", "origin"]).is_err() {
                    return Ok(());
                }
                crate::buckets::git(&repo, &["fetch", "--quiet", "origin"])?;
                let current = crate::buckets::git(&repo, &["rev-parse", "HEAD"])?;
                let fetched = crate::buckets::git(&repo, &["rev-parse", "FETCH_HEAD"])?;
                if current != fetched {
                    needs_attention = true;
                    eprintln!("Bucket {name} has remote changes; run scoop update.");
                }
                Ok(())
            })();
            if let Err(error) = result {
                needs_attention = true;
                eprintln!("warning: bucket {name} check failed; using local manifests: {error:#}");
            }
        }
    }
    let inventory = state::inventory(config)?;
    needs_attention |= !inventory.issues.is_empty();
    let mut rows = Vec::new();
    for (app, dir, record) in inventory.apps {
        let old = dir
            .file_name()
            .and_then(|s| s.to_str())
            .context("version")?;
        match Resolver::new(config).resolve(&app) {
            Ok(resolved) => {
                let new = resolved.manifest.version();
                let missing: Vec<_> =
                    crate::manifest::strings(resolved.manifest.value.get("depends"))
                        .into_iter()
                        .filter(|dep| {
                            state::operand_name(dep).ok().is_none_or(|name| {
                                state::current(config, name).ok().flatten().is_none()
                            })
                        })
                        .collect();
                if outdated(config, old, new) || record.hold || !missing.is_empty() {
                    rows.push(format!(
                        "{app}\t{old}\t{new}\t{}{}",
                        if record.hold { "Held package" } else { "" },
                        if missing.is_empty() {
                            String::new()
                        } else {
                            format!(" Missing dependencies: {}", missing.join(", "))
                        }
                    ));
                }
            }
            Err(error) => rows.push(format!("{app}\t{old}\t?\tManifest unavailable: {error}")),
        }
    }
    if !rows.is_empty() {
        println!("Name\tInstalled Version\tLatest Version\tInfo");
        for row in rows {
            println!("{row}");
        }
    } else if !needs_attention {
        println!("Everything is ok!");
    }
    Ok(())
}
pub fn update(config: &Config, options: &Update) -> Result<()> {
    let refresh_only = options.apps.is_empty() && !options.all;
    if refresh_only {
        ensure!(!options.no_cache, "--no-cache requires an app or --all");
        return refresh(config);
    }
    if refresh_due(config)
        && let Err(error) = refresh(config)
    {
        eprintln!("warning: refresh failed; updating packages from local manifests: {error:#}");
    }
    let arch = platform::select(None, config.architecture(), &platform::host()?, true)?;
    let operands = selected(config, &options.apps, options.all)?;
    each(operands, |operand| {
        let (input, _) = resolve::split_version(operand)?;
        let app = state::operand_name(input)?;
        journal::check(config, app)?;
        let (current, record) = state::current(config, app)?.context("package is not installed")?;
        platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
        if record.hold {
            if !options.quiet {
                println!(
                    "{app} is held to {}.",
                    current.file_name().unwrap().to_string_lossy()
                );
            }
            return Ok(());
        }
        let resolver = Resolver::new(config);
        let planned = lifecycle::plan(
            &resolver,
            operand,
            &arch,
            options.independent,
            options.skip_hash_check,
        )?;
        let requested = planned.last().context("package plan")?;
        let old = current
            .file_name()
            .and_then(|s| s.to_str())
            .context("current version")?;
        if !options.force && !outdated(config, old, requested.resolved.manifest.version()) {
            if !options.quiet {
                println!("{app}: {old} (latest version)");
            }
            return Ok(());
        }
        for package in planned {
            let name = &package.resolved.name;
            if name != app && state::current(config, name)?.is_some() {
                continue;
            }
            lifecycle::install_version(
                config,
                &package,
                download::Options {
                    force: options.force,
                    no_cache: options.no_cache,
                    skip_hash: options.skip_hash_check,
                },
                name == app && options.force,
                options.quiet,
                true,
            )?;
        }
        Ok(())
    })
}
#[derive(Clone)]
struct CacheEntry {
    path: std::path::PathBuf,
    app: String,
    version: String,
    size: u64,
}
fn cache_entries(config: &Config) -> Result<Vec<CacheEntry>> {
    if !config.cache.exists() {
        return Ok(vec![]);
    }
    ensure!(
        fs::symlink_metadata(&config.cache)?.file_type().is_dir(),
        "cache is not a managed directory"
    );
    let mut entries = Vec::new();
    for entry in fs::read_dir(&config.cache)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let parts: Vec<_> = name.splitn(4, '#').collect();
        if parts.len() != 4
            || manifest::name(parts[0]).is_err()
            || state::version_name(parts[1]).is_err()
            || parts[2].len() != 64
            || !parts[2].bytes().all(|c| c.is_ascii_hexdigit())
            || manifest::relative(parts[3]).is_err()
            || parts[3].contains('/')
        {
            continue;
        }
        entries.push(CacheEntry {
            path: entry.path(),
            app: parts[0].into(),
            version: parts[1].into(),
            size: entry.metadata()?.len(),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
pub fn cache(config: &Config, command: Option<&CacheCommand>) -> Result<()> {
    let (remove, apps, all) = match command {
        Some(CacheCommand::Rm { apps, all }) => (true, apps.as_slice(), *all),
        Some(CacheCommand::Show { apps }) => (false, apps.as_slice(), false),
        None => (false, &[][..], false),
    };
    let _lock = if remove {
        Some(config::mutation_lock(&config.root)?)
    } else {
        None
    };
    if remove {
        journal::recover_locked(config)?;
    }
    let filter = !(all || apps.is_empty() || apps.iter().any(|s| s == "*"));
    if filter {
        for app in apps {
            manifest::name(app)?;
        }
    }
    let mut count = 0;
    let mut bytes = 0;
    println!("Name\tVersion\tLength");
    for entry in cache_entries(config)? {
        if filter && !apps.contains(&entry.app) {
            continue;
        }
        count += 1;
        bytes += entry.size;
        if remove {
            journal::ensure_clear(config, &entry.app)?;
            fs::remove_file(&entry.path)?;
        } else {
            println!("{}\t{}\t{}", entry.app, entry.version, entry.size);
        }
    }
    if remove && config.cache.exists() {
        journal::sync_dir(&config.cache)?;
    }
    println!(
        "{}: {count} files, {bytes} bytes",
        if remove { "Deleted" } else { "Total" }
    );
    Ok(())
}
pub fn cleanup(config: &Config, apps: &[String], all: bool, cache: bool) -> Result<()> {
    let _lock = config::mutation_lock(&config.root)?;
    journal::recover_locked(config)?;
    each(selected(config, apps, all)?, |operand| {
        let app = state::operand_name(operand)?;
        journal::ensure_clear(config, app)?;
        let (current, _) = state::current(config, app)?.context("package is not installed")?;
        let dir = current.parent().context("app directory")?;
        let mut old = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name() == "current" || entry.path() == current {
                continue;
            }
            ensure!(
                entry.file_type()?.is_dir(),
                "unmanaged entry in app directory"
            );
            state::version_name(entry.file_name().to_str().context("version")?)?;
            Record::load(&entry.path())?;
            old.push(entry.path());
        }
        for path in old {
            fs::remove_dir_all(path)?;
        }
        journal::sync_dir(dir)?;
        if cache {
            for entry in cache_entries(config)? {
                if entry.app == app && Path::new(&entry.version) != current.file_name().unwrap() {
                    fs::remove_file(entry.path)?;
                }
            }
            if config.cache.exists() {
                journal::sync_dir(&config.cache)?;
            }
        }
        println!("Cleaned {app}.");
        Ok(())
    })
}
