use crate::{
    buckets::{Buckets, git, git_raw, http_text},
    config::Config,
    manifest::{self, Manifest},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub enum Source {
    Bucket {
        name: String,
        commit: String,
        path: PathBuf,
        dirty: bool,
    },
    File(PathBuf),
    Url(String),
    Installed(PathBuf),
}
impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bucket { name, dirty, .. } => write!(
                f,
                "{name}{}",
                if *dirty { " (dirty working tree)" } else { "" }
            ),
            Self::File(p) | Self::Installed(p) => write!(f, "{}", p.display()),
            Self::Url(url) => write!(f, "{url}"),
        }
    }
}
impl Source {
    pub fn provenance(&self) -> String {
        match self {
            Self::Bucket {
                name,
                commit,
                path,
                dirty,
            } => format!(
                "bucket={name}; commit={commit}; path={}; working_tree={}",
                path.display(),
                if *dirty { "dirty" } else { "clean" }
            ),
            Self::File(path) => format!("file={}", path.display()),
            Self::Installed(path) => format!("installed_manifest={}", path.display()),
            Self::Url(url) => format!("url={url}"),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub manifest: Manifest,
    pub source: Source,
    pub alternatives: Vec<String>,
}
pub struct Resolver<'a> {
    pub config: &'a Config,
    pub buckets: Buckets,
    validate: bool,
}
impl<'a> Resolver<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            buckets: Buckets::new(&config.root),
            validate: true,
        }
    }
    pub fn manifest_text(&self, operand: &str) -> Result<String> {
        let display = Self {
            config: self.config,
            buckets: self.buckets.clone(),
            validate: false,
        };
        Ok(display.resolve(operand)?.manifest.raw)
    }
    fn parse(&self, raw: String) -> Result<Manifest> {
        if self.validate {
            Manifest::parse(raw)
        } else {
            let value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                .context("invalid manifest JSON")?;
            Ok(Manifest { value, raw })
        }
    }
    pub fn resolve(&self, operand: &str) -> Result<Resolved> {
        let literal = Path::new(operand);
        let explicit_file = operand.ends_with(".json") || operand.starts_with(['.', '/']);
        let (input, version) = if explicit_file && literal.is_file() {
            (operand, None)
        } else {
            split_version(operand)?
        };
        if input.starts_with("https://") || input.starts_with("http://") {
            let url = reqwest::Url::parse(input)?;
            let name = Path::new(url.path())
                .file_stem()
                .and_then(|s| s.to_str())
                .context("manifest URL needs a package filename")?
                .to_owned();
            return self.external(
                name,
                http_text(input)?,
                Source::Url(input.to_owned()),
                version,
            );
        }
        let path = Path::new(input);
        if input.ends_with(".json") || input.starts_with(['.', '/']) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("manifest file needs a package filename")?
                .to_owned();
            let path =
                fs::canonicalize(path).with_context(|| format!("manifest file '{input}'"))?;
            return self.external(
                name,
                fs::read_to_string(&path)?,
                Source::File(path),
                version,
            );
        }
        let parts: Vec<_> = input.split('/').collect();
        ensure!(parts.len() <= 2, "invalid package selector '{input}'");
        for part in &parts {
            manifest::name(part)?;
        }
        if parts.len() == 2 {
            return self.bucket(parts[0], parts[1], version);
        }
        let app = parts[0];
        // Installed source wins for bare names; explicit bucket operands always win above.
        let current = self.config.root.join("apps").join(app).join("current");
        let record_path = current.join("scoop-install.json");
        if record_path.is_file() {
            let record: serde_json::Value = serde_json::from_slice(&fs::read(&record_path)?)?;
            if let Some(bucket) = record.get("bucket").and_then(|v| v.as_str()) {
                return self.bucket(bucket, app, version);
            }
            if let Some(url) = record.get("url").and_then(|v| v.as_str()) {
                let raw = if url.starts_with("http://") || url.starts_with("https://") {
                    http_text(url)
                } else {
                    fs::read_to_string(url).map_err(Into::into)
                };
                if let Ok(raw) = raw {
                    return self.external(
                        app.into(),
                        raw,
                        if url.starts_with("http") {
                            Source::Url(url.into())
                        } else {
                            Source::File(url.into())
                        },
                        version,
                    );
                }
            }
            let path = current.join("scoop-manifest.json");
            return self.external(
                app.into(),
                fs::read_to_string(&path)?,
                Source::Installed(path),
                version,
            );
        }
        let mut matches = Vec::new();
        for bucket in self.buckets.names()? {
            if self.buckets.find(&bucket, app)?.is_some() {
                matches.push(bucket);
            }
        }
        // Bucket precedence is determined before searching a selected bucket's history.
        if let Some(first) = matches.first() {
            let mut result = self.bucket(first, app, version)?;
            result.alternatives = matches.into_iter().skip(1).collect();
            return Ok(result);
        }
        if version.is_some() {
            for bucket in self.buckets.names()? {
                if let Ok(found) = self.bucket(&bucket, app, version) {
                    return Ok(found);
                }
            }
        }
        bail!("manifest '{operand}' was not found in local buckets")
    }
    fn external(
        &self,
        name: String,
        raw: String,
        source: Source,
        version: Option<&str>,
    ) -> Result<Resolved> {
        manifest::name(&name)?;
        let manifest = self.parse(raw)?;
        if let Some(version) = version {
            let declared = manifest
                .value
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("<missing>");
            ensure!(
                declared == version,
                "matching manifest required: requested {version}, manifest declares {declared}"
            );
        }
        Ok(Resolved {
            name,
            manifest,
            source,
            alternatives: Vec::new(),
        })
    }
    fn bucket(&self, bucket: &str, app: &str, version: Option<&str>) -> Result<Resolved> {
        manifest::name(bucket)?;
        manifest::name(app)?;
        let repo = self.buckets.repo(bucket)?;
        let path = self.buckets.find(bucket, app)?;
        if let Some(path) = path {
            let raw = fs::read_to_string(&path)?;
            let value: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                .unwrap_or(serde_json::Value::Null);
            if version.is_none_or(|v| value.get("version").and_then(|v| v.as_str()) == Some(v)) {
                let manifest = self.parse(raw)?;
                return Ok(Resolved {
                    name: app.into(),
                    manifest,
                    source: Source::Bucket {
                        name: bucket.into(),
                        commit: git(&repo, &["rev-parse", "HEAD"])?,
                        path: path.strip_prefix(&repo)?.to_owned(),
                        dirty: !git(
                            &repo,
                            &[
                                "status",
                                "--porcelain",
                                "--",
                                path.strip_prefix(&repo)?
                                    .to_str()
                                    .context("non-UTF8 manifest path")?,
                            ],
                        )?
                        .is_empty(),
                    },
                    alternatives: Vec::new(),
                });
            }
        }
        if let Some(version) = version {
            ensure!(
                self.config
                    .values
                    .get("use_git_history")
                    .and_then(|v| v.as_bool())
                    != Some(false),
                "historical manifest resolution is disabled by use_git_history"
            );
            // Walk only commits changing this filename. NUL-separated names preserve Unicode paths.
            let history = git_raw(
                &repo,
                &[
                    "log",
                    "--format=COMMIT:%H",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    "HEAD",
                    "--",
                    &format!(":(literal){app}.json"),
                    &format!(":(glob)**/{app}.json"),
                ],
            )?;
            let mut commit = "";
            for part in history.split('\0') {
                let part = part.trim_start_matches('\n');
                if let Some(hash) = part.strip_prefix("COMMIT:") {
                    commit = hash;
                    continue;
                }
                if part.is_empty() || commit.is_empty() {
                    continue;
                }
                let path = part;
                let Ok(raw) = git_raw(&repo, &["show", &format!("{commit}:{path}")]) else {
                    continue;
                };
                let Ok(value) =
                    serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}'))
                else {
                    continue;
                };
                if value.get("version").and_then(|v| v.as_str()) == Some(version) {
                    if !path.starts_with("bucket/")
                        && git(&repo, &["cat-file", "-t", &format!("{commit}:bucket")])
                            .is_ok_and(|kind| kind == "tree")
                    {
                        continue;
                    }
                    return self.external(
                        app.into(),
                        raw,
                        Source::Bucket {
                            name: bucket.into(),
                            commit: commit.into(),
                            path: path.into(),
                            dirty: false,
                        },
                        Some(version),
                    );
                }
            }
            bail!(
                "no historical manifest for '{bucket}/{app}@{version}'; a matching manifest is required"
            );
        }
        bail!("manifest '{bucket}/{app}' was not found")
    }
}
pub fn split_version(input: &str) -> Result<(&str, Option<&str>)> {
    if let Some((base, version)) = input.rsplit_once('@') {
        // An @ in URL authority or a directory is part of the source.
        if !version.contains('/') {
            ensure!(
                !version.is_empty()
                    && version
                        .chars()
                        .all(|c| c.is_alphanumeric() || ".-+_".contains(c)),
                "invalid version qualifier '{version}'"
            );
            return Ok((base, Some(version)));
        }
    }
    Ok((input, None))
}
