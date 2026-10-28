use crate::{
    config::Config,
    manifest::{self, Binary, Manifest},
    resolve::{Resolved, Source},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub architecture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
    #[serde(skip)]
    pub bins: Vec<Binary>,
}
impl Record {
    pub fn new(resolved: &Resolved, architecture: &str, bins: Vec<Binary>) -> Self {
        let (bucket, url) = match &resolved.source {
            Source::Bucket { name, .. } => (Some(name.clone()), None),
            Source::File(p) | Source::Installed(p) => {
                (None, Some(p.to_string_lossy().into_owned()))
            }
            Source::Url(url) => (None, Some(url.clone())),
        };
        Self {
            architecture: architecture.into(),
            bucket,
            url,
            hold: false,
            bins,
        }
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("scoop-install.json");
        let mut record: Self = serde_json::from_slice(
            &fs::read(&path)
                .with_context(|| format!("read installation record {}", path.display()))?,
        )
        .with_context(|| format!("invalid installation record {}", path.display()))?;
        record.bins = saved_manifest(dir)?.target(&record.architecture)?.bins;
        let mut commands = std::collections::HashSet::new();
        for bin in &record.bins {
            bin.validate()?;
            ensure!(
                commands.insert(&bin.name),
                "duplicate shim name in installation record"
            );
        }
        Ok(record)
    }
}
pub fn updated(dir: &Path) -> Result<chrono::DateTime<chrono::Utc>> {
    Ok(fs::metadata(dir.join("scoop-install.json"))?
        .modified()?
        .into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_bins_from_saved_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("scoop-manifest.json"), r#"{"version":"1.0","homepage":"https://example.com","license":"MIT","url":"https://example.com/app.tar.gz","bin":[["bin/tool","alias"]]}"#).unwrap();
        fs::write(
            dir.path().join("scoop-install.json"),
            r#"{"architecture":"64bit","bucket":"main"}"#,
        )
        .unwrap();
        let record = Record::load(dir.path()).unwrap();
        assert_eq!(
            record.bins,
            vec![Binary {
                path: "bin/tool".into(),
                name: "alias".into()
            }]
        );
        assert_eq!(
            serde_json::to_value(record).unwrap(),
            serde_json::json!({"architecture":"64bit","bucket":"main"})
        );
    }
}
