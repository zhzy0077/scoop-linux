use crate::{
    config::Config,
    manifest::{self, Manifest},
    resolve::{Resolved, Source},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub format_version: u32,
    pub architecture: String,
    pub bucket: Option<String>,
    pub url: Option<String>,
    pub bucket_commit: Option<String>,
    #[serde(default)]
    pub bucket_dirty: bool,
    pub manifest_path: Option<PathBuf>,
    pub hold: bool,
    pub installed_at: u64,
    pub bins: Vec<String>,
}
impl Record {
    pub fn new(resolved: &Resolved, architecture: &str, bins: Vec<String>) -> Self {
        let (bucket, url, commit, path) = match &resolved.source {
            Source::Bucket {
                name, commit, path, ..
            } => (
                Some(name.clone()),
                None,
                Some(commit.clone()),
                Some(path.clone()),
            ),
            Source::File(p) | Source::Installed(p) => {
                (None, Some(p.to_string_lossy().into_owned()), None, None)
            }
            Source::Url(url) => (None, Some(url.clone()), None, None),
        };
        Self {
            format_version: 1,
            architecture: architecture.into(),
            bucket,
            url,
            bucket_commit: commit,
            bucket_dirty: matches!(&resolved.source, Source::Bucket { dirty: true, .. }),
            manifest_path: path,
            hold: false,
            installed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            bins,
        }
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("scoop-install.json");
        let record: Self = serde_json::from_slice(
            &fs::read(&path)
                .with_context(|| format!("read installation record {}", path.display()))?,
        )
        .with_context(|| format!("invalid installation record {}", path.display()))?;
        ensure!(
            record.format_version == 1,
            "unsupported installation record format"
        );
        Ok(record)
    }
}
pub fn managed_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) => ensure!(
            m.file_type().is_dir(),
            "managed directory is an unrelated file or symlink: {}",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
pub fn operand_name(operand: &str) -> Result<&str> {
    let parts: Vec<_> = operand.split('/').collect();
    ensure!(
        parts.len() <= 2,
        "invalid installed package selector '{operand}'"
    );
    for part in &parts {
        manifest::name(part)?;
    }
    Ok(parts[parts.len() - 1])
}
pub fn app_dir(config: &Config, app: &str) -> Result<PathBuf> {
    manifest::name(app)?;

    Ok(config.root.join("apps").join(app))
}
pub fn current(config: &Config, app: &str) -> Result<Option<(PathBuf, Record)>> {
    let apps = config.root.join("apps");
    if fs::symlink_metadata(&apps).is_ok() {
        ensure!(
            fs::symlink_metadata(&apps)?.file_type().is_dir(),
            "apps is not a managed directory"
        );
    }
    let app_dir = app_dir(config, app)?;
    let link = app_dir.join("current");
    if !app_dir.exists() {
        return Ok(None);
    }
    ensure!(
        fs::symlink_metadata(&app_dir)?.file_type().is_dir(),
        "app directory is not managed"
    );
    let target = match fs::read_link(&link) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let version = target.to_str().context("invalid current version")?;
    version_name(version)?;
    ensure!(
        target.components().count() == 1,
        "current link escapes package"
    );
    let dir = app_dir.join(target);
    ensure!(
        fs::symlink_metadata(&dir)?.file_type().is_dir(),
        "version directory is not managed"
    );
    let record = Record::load(&dir)?;
    Ok(Some((dir, record)))
}
pub fn version_name(version: &str) -> Result<()> {
    ensure!(
        regex::Regex::new(r"^[\w.\-+_]+$")?.is_match(version)
            && ![".", "..", "current"].contains(&version),
        "unsafe or reserved version name '{version}'"
    );
    Ok(())
}
pub struct Inventory {
    pub apps: Vec<(String, PathBuf, Record)>,
    pub issues: Vec<String>,
}
pub fn installed(config: &Config) -> Result<Vec<(String, PathBuf, Record)>> {
    Ok(inventory(config)?.apps)
}
pub fn inventory(config: &Config) -> Result<Inventory> {
    let apps = config.root.join("apps");
    if !apps.exists() {
        return Ok(Inventory {
            apps: vec![],
            issues: vec![],
        });
    }
    ensure!(
        fs::symlink_metadata(&apps)?.file_type().is_dir(),
        "apps is not a managed directory"
    );
    let mut result = Vec::new();
    let mut issues = Vec::new();
    for entry in fs::read_dir(apps)? {
        let entry = entry?;
        let app = match entry.file_name().into_string() {
            Ok(app) => app,
            Err(_) => {
                let issue = format!("{}: invalid UTF-8 app name", entry.path().display());
                eprintln!("warning: {issue}");
                issues.push(issue);
                continue;
            }
        };
        let inspected = (|| -> Result<_> {
            manifest::name(&app)?;
            ensure!(
                entry.file_type()?.is_dir(),
                "entry is not a managed app directory"
            );
            current(config, &app)?.context("no current installation")
        })();
        match inspected {
            Ok((dir, record)) => result.push((app, dir, record)),
            Err(error) => {
                let issue = format!("{}: {error:#}", entry.path().display());
                eprintln!("warning: {issue}");
                issues.push(issue);
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Inventory {
        apps: result,
        issues,
    })
}
pub fn saved_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join("scoop-manifest.json");
    Manifest::parse(
        fs::read_to_string(&path)
            .with_context(|| format!("read installed manifest {}", path.display()))?,
    )
    .with_context(|| format!("invalid installed manifest {}", path.display()))
}
