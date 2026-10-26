use crate::{
    buckets::{self, Buckets},
    config::{self, Config},
    download, lifecycle, maintenance, manifest, platform,
    resolve::Resolver,
    state,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value, json};
use std::{fs, path::Path};
const MACHINE_CONFIG: &[&str] = &[
    "last_update",
    "root_path",
    "global_path",
    "cache_path",
    "alias",
    "main_bucket_url",
];
pub struct Export {
    pub document: Value,
    pub issues: Vec<String>,
}
fn normalized_source(source: &str) -> String {
    let path = if source.starts_with("file://") {
        reqwest::Url::parse(source)
            .ok()
            .and_then(|url| url.to_file_path().ok())
    } else if Path::new(source).is_absolute() {
        Some(source.into())
    } else {
        None
    };
    let source = path
        .and_then(|path| fs::canonicalize(&path).ok().or(Some(path)))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_owned());
    source
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned()
}
fn field<'a>(entry: &'a Value, name: &str) -> Option<&'a Value> {
    entry
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}
fn text<'a>(entry: &'a Value, name: &str) -> Result<&'a str> {
    field(entry, name)
        .and_then(Value::as_str)
        .with_context(|| format!("{name} must be a string"))
}
pub fn export(config: &Config, include_config: bool) -> Result<Export> {
    let _lock = config::mutation_lock(&config.root)?;
    crate::journal::recover_locked(config)?;
    let buckets = Buckets::new(&config.root);
    let mut repositories = Vec::new();
    let mut issues = Vec::new();
    let names = match buckets.names() {
        Ok(names) => names,
        Err(error) => {
            issues.push(format!("bucket inventory: {error:#}"));
            vec![]
        }
    };
    for name in names {
        let source = (|| -> Result<String> {
            buckets::git(&buckets.repo(&name)?, &["remote", "get-url", "origin"])
        })();
        match source {
            Ok(source) if !source.is_empty() => {
                repositories.push(json!({"Name":name,"Source":source}))
            }
            result => {
                let error = result
                    .err()
                    .map(|error| format!("{error:#}"))
                    .unwrap_or_else(|| "origin is empty".into());
                let issue = format!("bucket {name}: {error}");
                eprintln!("warning: {issue}");
                issues.push(issue);
            }
        }
    }
    let inventory = match state::inventory(config) {
        Ok(inventory) => inventory,
        Err(error) => {
            issues.push(format!("app inventory: {error:#}"));
            state::Inventory {
                apps: vec![],
                issues: vec![],
            }
        }
    };
    issues.extend(inventory.issues);
    let mut apps = Vec::new();
    for (name, dir, record) in inventory.apps {
        let result = (|| -> Result<Value> {
            ensure!(
                record
                    .bucket
                    .as_deref()
                    .or(record.url.as_deref())
                    .is_some_and(|source| !source.is_empty()),
                "installation source is missing"
            );
            ensure!(
                platform::normalize(&record.architecture)? == record.architecture,
                "invalid recorded architecture"
            );
            let version = dir
                .file_name()
                .and_then(|s| s.to_str())
                .context("version name")?;
            let updated = chrono::DateTime::<chrono::Utc>::from_timestamp(
                i64::try_from(record.installed_at)?,
                0,
            )
            .context("invalid installation timestamp")?
            .to_rfc3339();
            let mut info = vec![record.architecture.clone()];
            if record.hold {
                info.push("Held package".into())
            }
            Ok(
                json!({"Name":name,"Version":version,"Source":record.bucket.as_deref().or(record.url.as_deref()).unwrap_or(""),"Updated":updated,"Info":info.join(", ")}),
            )
        })();
        match result {
            Ok(app) => apps.push(app),
            Err(error) => {
                let issue = format!("app {name}: {error:#}");
                eprintln!("warning: {issue}");
                issues.push(issue);
            }
        }
    }
    let mut document = Map::new();
    if include_config {
        let mut values = config.values.clone();
        for key in MACHINE_CONFIG {
            values.remove(*key);
        }
        document.insert("config".into(), values.into());
    }
    document.insert("buckets".into(), repositories.into());
    document.insert("apps".into(), apps.into());
    Ok(Export {
        document: document.into(),
        issues,
    })
}
pub fn import(config: &Config, location: &str) -> Result<()> {
    let raw = if location.starts_with("https://") || location.starts_with("http://") {
        buckets::http_text(location)?
    } else {
        ensure!(
            fs::metadata(location)
                .with_context(|| format!("read Scoopfile {location}"))?
                .len()
                <= 4 * 1024 * 1024,
            "Scoopfile exceeds 4 MiB"
        );
        fs::read_to_string(location).with_context(|| format!("read Scoopfile {location}"))?
    };
    let document: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .context("invalid Scoopfile JSON")?;
    ensure!(document.is_object(), "Scoopfile must be an object");
    let mut failures = Vec::new();
    if let Some(values) = field(&document, "config") {
        if let Some(values) = values.as_object() {
            for (key, value) in values {
                let result = (|| -> Result<()> {
                    ensure!(
                        !MACHINE_CONFIG.contains(&key.as_str()),
                        "machine-specific config is not imported: {key}"
                    );
                    let raw = match value {
                        Value::String(v) => v.clone(),
                        Value::Bool(v) => v.to_string(),
                        Value::Null => "$null".into(),
                        _ => anyhow::bail!("unsupported config value type: {key}"),
                    };
                    let mut current = Config::load()?;
                    current.command(&[key.clone(), raw])
                })();
                if let Err(error) = result {
                    eprintln!("config {key}: {error:#}");
                    failures.push(format!("config {key}"));
                }
            }
        } else {
            eprintln!("config: expected an object");
            failures.push("config".into());
        }
    }
    let current = Config::load()?;
    ensure!(
        current.root == config.root,
        "import cannot move the installation root"
    );
    let config = &current;
    let buckets = Buckets::new(&config.root);
    let mut names = std::collections::HashSet::new();
    let mut unavailable = std::collections::HashSet::new();
    if let Some(entries) = field(&document, "buckets") {
        if let Some(entries) = entries.as_array() {
            for (index, entry) in entries.iter().enumerate() {
                let result = (|| -> Result<()> {
                    let name = text(entry, "Name")?;
                    manifest::name(name)?;
                    names.insert(name.to_owned());
                    let source = text(entry, "Source")?;
                    let repo = buckets.repo(name)?;
                    if repo.exists() {
                        ensure!(
                            fs::symlink_metadata(&repo)?.file_type().is_dir(),
                            "bucket {name} is not a managed directory"
                        );
                        buckets::git(&repo, &["rev-parse", "--is-inside-work-tree"])?;
                        let existing = buckets::git(&repo, &["remote", "get-url", "origin"])
                            .unwrap_or_default();
                        if normalized_source(&existing) != normalized_source(source) {
                            eprintln!(
                                "warning: bucket {name} already exists with source '{existing}'; keeping it (Scoopfile requested '{source}')."
                            );
                        }
                    } else {
                        buckets.add(name, Some(source))?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    if let Some(name) = field(entry, "Name").and_then(Value::as_str) {
                        unavailable.insert(name.to_owned());
                    }
                    eprintln!("bucket entry {index}: {error:#}");
                    failures.push(format!("bucket entry {index}"));
                }
            }
        } else {
            eprintln!("buckets: expected an array");
            failures.push("buckets".into());
        }
    }
    // Import shares install's due-refresh policy, once, before any app planning.
    maintenance::automatic(config, false)?;
    if let Some(entries) = field(&document, "apps") {
        if let Some(entries) = entries.as_array() {
            for (index, entry) in entries.iter().enumerate() {
                let result = (|| -> Result<()> {
                    let name = text(entry, "Name")?;
                    manifest::name(name)?;
                    let source = text(entry, "Source")?;
                    let info = field(entry, "Info").and_then(Value::as_str).unwrap_or("");
                    let tokens: Vec<_> = info
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect();
                    ensure!(
                        !tokens.contains(&"Global install"),
                        "unsupported feature: global Scoopfile entry"
                    );
                    let explicit = field(entry, "Architecture").and_then(Value::as_str);
                    let info_arch = tokens
                        .iter()
                        .find(|token| {
                            matches!(**token, "32bit" | "64bit" | "arm64" | "arm" | "riscv64")
                                || (!token.contains(' ')
                                    && platform::normalize(token).is_ok_and(|s| s == **token))
                        })
                        .copied();
                    if let (Some(explicit), Some(info_arch)) = (explicit, info_arch) {
                        ensure!(
                            platform::normalize(explicit)? == platform::normalize(info_arch)?,
                            "conflicting Scoopfile architecture fields"
                        );
                    }
                    let selected = platform::select(
                        explicit.or(info_arch),
                        config.architecture(),
                        &platform::host()?,
                        true,
                    )?;
                    for token in &tokens {
                        ensure!(
                            Some(*token) == info_arch
                                || ["Held package", "Deprecated package"].contains(token),
                            "unsupported Scoopfile Info entry: {token}"
                        );
                    }
                    let operand = if names.contains(source) {
                        ensure!(
                            !unavailable.contains(source),
                            "bucket {source} could not be added; repair its bucket entry before importing {name}"
                        );
                        format!("{source}/{name}")
                    } else if source == "<auto-generated>" {
                        let version = text(entry, "Version")?;
                        state::version_name(version)?;
                        format!("{name}@{version}")
                    } else {
                        ensure!(!source.is_empty(), "app Source is missing");
                        source.to_owned()
                    };
                    let planned =
                        lifecycle::plan(&Resolver::new(config), &operand, &selected, false)?;
                    ensure!(
                        planned.last().is_some_and(|p| p.resolved.name == name),
                        "app name/source mismatch for {name}"
                    );
                    if let Some((_, record)) = state::current(config, name)? {
                        platform::select(
                            Some(&record.architecture),
                            None,
                            &platform::host()?,
                            true,
                        )?;
                        ensure!(
                            record.bucket.as_deref().or(record.url.as_deref()) == Some(source)
                                || source == "<auto-generated>",
                            "installed source conflict for {name}"
                        );
                    }
                    lifecycle::install(
                        config,
                        &[operand],
                        &selected,
                        false,
                        download::Options {
                            progress: true,
                            ..Default::default()
                        },
                    )?;
                    let held = tokens.contains(&"Held package")
                        || field(entry, "Hold").and_then(Value::as_bool) == Some(true);
                    if held {
                        maintenance::hold(config, &[name.into()], true)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    eprintln!("app entry {index}: {error:#}");
                    failures.push(format!("app entry {index}"));
                }
            }
        } else {
            eprintln!("apps: expected an array");
            failures.push("apps".into());
        }
    }
    ensure!(
        failures.is_empty(),
        "Scoopfile entries failed: {}",
        failures.join(", ")
    );
    Ok(())
}
