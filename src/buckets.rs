use crate::{config, manifest};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct Buckets {
    pub root: PathBuf,
}
pub fn git(repo: &Path, args: &[&str]) -> Result<String> {
    Ok(git_raw(repo, args)?.trim_end().to_owned())
}
pub fn git_raw(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("Git is required")?;
    ensure!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?)
}
impl Buckets {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.join("buckets"),
        }
    }
    pub fn known() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(include_str!("../known-buckets.json")).expect("known bucket registry")
    }
    pub fn names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        if !self.root.exists() {
            return Ok(names);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && !entry.file_name().to_string_lossy().starts_with(".bucket-")
            {
                names.push(
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("non-UTF8 bucket name"))?,
                );
            }
        }
        names.sort();
        let known = Self::known();
        names.sort_by_key(|n| {
            (
                known.keys().position(|key| key == n).unwrap_or(usize::MAX),
                n.clone(),
            )
        });
        Ok(names)
    }
    pub fn repo(&self, bucket: &str) -> Result<PathBuf> {
        manifest::name(bucket)?;
        Ok(self.root.join(bucket))
    }
    pub fn directory(&self, bucket: &str) -> Result<PathBuf> {
        let repo = self.repo(bucket)?;
        Ok(if repo.join("bucket").is_dir() {
            repo.join("bucket")
        } else {
            repo
        })
    }
    pub fn manifests(&self, bucket: &str) -> Result<Vec<PathBuf>> {
        fn walk(dir: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
            if !dir.exists() {
                return Ok(());
            }
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let ty = entry.file_type()?;
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if ty.is_dir() {
                    walk(&entry.path(), result)?;
                } else if ty.is_file() && entry.path().extension().is_some_and(|v| v == "json") {
                    result.push(entry.path());
                }
            }
            Ok(())
        }
        let mut result = Vec::new();
        walk(&self.directory(bucket)?, &mut result)?;
        result.sort();
        Ok(result)
    }
    pub fn find(&self, bucket: &str, app: &str) -> Result<Option<PathBuf>> {
        manifest::name(app)?;
        let matches: Vec<_> = self
            .manifests(bucket)?
            .into_iter()
            .filter(|p| p.file_stem().is_some_and(|s| s == app))
            .collect();
        ensure!(matches.len() <= 1, "ambiguous manifest '{bucket}/{app}'");
        Ok(matches.into_iter().next())
    }
    pub fn add(&self, name: &str, source: Option<&str>) -> Result<()> {
        ensure!(
            !name.starts_with('.'),
            "bucket names must not start with '.'"
        );
        let destination = self.repo(name)?;
        let known = Self::known();
        let source = source
            .or_else(|| known.get(name).and_then(|v| v.as_str()))
            .with_context(|| {
                format!("no Linux repository configured for bucket '{name}'; supply <repo>")
            })?;
        ensure!(!source.starts_with('-'), "invalid bucket repository");
        let root = self.root.parent().context("bucket root")?;
        let _lock = config::mutation_lock(root)?;
        ensure!(
            fs::symlink_metadata(&destination)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "bucket destination already exists: {}",
            destination.display()
        );
        fs::create_dir_all(&self.root)?;
        ensure!(
            fs::symlink_metadata(&self.root)?.file_type().is_dir(),
            "buckets is not a managed directory"
        );
        let temp = tempfile::Builder::new()
            .prefix(".bucket-")
            .tempdir_in(&self.root)?;
        let clone = temp.path().join("repo");
        let status = Command::new("git")
            .args(["clone", "--", source])
            .arg(&clone)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        ensure!(
            status.status.success(),
            "git clone: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        git(&clone, &["rev-parse", "--verify", "HEAD"])?;
        let registry_path = root.join(".bucket-sources.json");
        let mut registry: serde_json::Map<String, serde_json::Value> = if registry_path.exists() {
            serde_json::from_slice(&fs::read(&registry_path)?)?
        } else {
            Default::default()
        };
        let original = registry.clone();
        registry.insert(
            name.to_owned(),
            git(&clone, &["remote", "get-url", "origin"])?.into(),
        );
        config::atomic_json(&registry_path, &registry)?;
        if let Err(error) = fs::rename(&clone, &destination) {
            config::atomic_json(&registry_path, &original)?;
            return Err(error.into());
        }
        println!("Bucket '{name}' added ({source}).");
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        let repo = self.repo(name)?;
        if fs::symlink_metadata(&self.root).is_ok() {
            ensure!(
                fs::symlink_metadata(&self.root)?.file_type().is_dir(),
                "buckets is not a managed directory"
            );
        }
        ensure!(
            fs::symlink_metadata(&repo).is_ok(),
            "bucket '{name}' not found"
        );
        let _lock = config::mutation_lock(self.root.parent().context("bucket root")?)?;
        let registry_path = self
            .root
            .parent()
            .context("bucket root")?
            .join(".bucket-sources.json");
        let mut registry: serde_json::Map<String, serde_json::Value> = if registry_path.exists() {
            serde_json::from_slice(&fs::read(&registry_path)?)?
        } else {
            Default::default()
        };
        ensure!(
            registry.contains_key(name),
            "refusing to remove unregistered bucket directory '{name}'"
        );
        ensure!(
            fs::symlink_metadata(&repo)?.file_type().is_dir(),
            "bucket must be a managed directory"
        );
        ensure!(
            repo.join(".git").is_dir(),
            "refusing to remove a directory without a Git repository"
        );
        ensure!(
            registry.get(name).and_then(|v| v.as_str())
                == Some(git(&repo, &["remote", "get-url", "origin"])?.as_str()),
            "bucket source changed; refusing to remove an unrelated repository"
        );
        ensure!(
            git(&repo, &["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
            "bucket has local changes; preserve them before removing it"
        );
        // Remove only a clean registered repository, never an unrelated root entry.
        fs::remove_dir_all(repo)?;
        registry.remove(name);
        config::atomic_json(&registry_path, &registry)?;
        println!("Bucket '{name}' removed.");
        Ok(())
    }
    pub fn command(&self, command: &crate::cli::BucketCommand) -> Result<()> {
        use crate::cli::BucketCommand::*;
        match command {
            Add { name, repo } => self.add(name, repo.as_deref()),
            Rm { name } => self.remove(name),
            Known => {
                for (name, repo) in Self::known() {
                    println!("{name}\t{}", repo.as_str().unwrap_or_default());
                }
                Ok(())
            }
            List => {
                println!("Name\tSource\tUpdated\tManifests");
                for name in self.names()? {
                    let repo = self.repo(&name)?;
                    let source =
                        git(&repo, &["remote", "get-url", "origin"]).unwrap_or_else(|error| {
                            eprintln!("warning: {name}: {error}");
                            "<unavailable>".into()
                        });
                    let updated =
                        git(&repo, &["log", "-1", "--format=%cI"]).unwrap_or_else(|error| {
                            eprintln!("warning: {name}: {error}");
                            "<unavailable>".into()
                        });
                    println!(
                        "{name}\t{source}\t{updated}\t{}",
                        self.manifests(&name)
                            .map(|paths| paths.len())
                            .unwrap_or_else(|error| {
                                eprintln!("warning: {name}: {error}");
                                0
                            })
                    );
                }
                Ok(())
            }
        }
    }
}
pub fn http_text(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    if !["http", "https"].contains(&parsed.scheme()) {
        bail!("unsupported manifest URL scheme");
    }
    let response = crate::network::client(30)?
        .get(parsed)
        .send()?
        .error_for_status()?;
    ensure!(
        !response.status().is_redirection(),
        "manifest redirect policy refused the redirect"
    );
    const LIMIT: u64 = 4 * 1024 * 1024;
    use std::io::Read;
    let mut bytes = Vec::new();
    response.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "manifest response exceeds 4 MiB"
    );
    String::from_utf8(bytes).context("manifest must be UTF-8")
}
