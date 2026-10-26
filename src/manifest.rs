use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{collections::HashSet, path::Path, sync::OnceLock};

#[derive(Debug, Clone)]
pub struct Manifest {
    pub value: Value,
    pub raw: String,
}
#[derive(Debug, Clone)]
pub struct Artifact {
    pub url: String,
    pub hash: Option<String>,
    pub extract_dir: Option<String>,
    pub extract_to: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Target {
    pub architecture: String,
    pub artifacts: Vec<Artifact>,
    pub bins: Vec<Binary>,
    pub dependencies: Vec<String>,
}
pub fn name(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.+@".contains(&c)),
        "invalid package or bucket name '{value}'"
    );
    Ok(())
}
pub fn relative(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && !value.starts_with('/')
            && !value.contains(['\\', '\0'])
            && !(value.as_bytes().get(1) == Some(&b':')
                && value.as_bytes()[0].is_ascii_alphabetic())
            && !value.split('/').any(|c| matches!(c, "" | "." | "..")),
        "unsafe relative path '{value}'"
    );
    Ok(())
}
pub fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => vec![],
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Binary {
    pub path: String,
    pub name: String,
}
impl Binary {
    pub fn validate(&self) -> Result<()> {
        relative(&self.path)?;
        relative(&self.name)?;
        ensure!(
            !self.name.contains('/') && self.name == self.name.to_lowercase(),
            "invalid bin name"
        );
        Ok(())
    }
}
/// Parse Scoop `bin` definitions: a target, or `[target, name, arguments]`.
pub fn binaries(value: Option<&Value>) -> Result<Vec<Binary>> {
    let entries = match value {
        None => return Ok(Vec::new()),
        Some(value @ Value::String(_)) => vec![value],
        Some(Value::Array(entries)) => entries.iter().collect(),
        _ => bail!("bin must be a path or a list of bin definitions"),
    };
    let mut names = HashSet::new();
    let mut result = Vec::new();
    for entry in entries {
        let fields = if let Some(path) = entry.as_str() {
            vec![path]
        } else {
            entry
                .as_array()
                .context("bin entries must be paths or bin definitions")?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .context("bin definition fields must be strings")
                })
                .collect::<Result<Vec<_>>>()?
        };
        let path = fields
            .first()
            .context("bin definitions require a target")?
            .replace('\\', "/");
        relative(&path)?;
        ensure!(
            fields.iter().skip(2).all(|arg| arg.trim().is_empty()),
            "unsupported manifest feature: bin fixed arguments for '{path}'"
        );
        let basename = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .context("bin needs an executable basename")?;
        let default_name = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
        let name = fields
            .get(1)
            .filter(|name| !name.is_empty())
            .copied()
            .unwrap_or(default_name)
            .to_lowercase();
        ensure!(names.insert(name.clone()), "duplicate bin name '{name}'");
        let bin = Binary { path, name };
        bin.validate()?;
        result.push(bin);
    }
    Ok(result)
}
impl Manifest {
    pub fn parse(raw: String) -> Result<Self> {
        let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
            .context("invalid manifest JSON")?;
        let object = value.as_object().context("manifest must be an object")?;
        const UNSUPPORTED: &[&str] = &[
            "persist",
            "env_add_path",
            "env_set",
            "installer",
            "uninstaller",
            "pre_install",
            "post_install",
            "pre_uninstall",
            "post_uninstall",
            "innosetup",
            "psmodule",
            "shortcuts",
        ];
        for field in UNSUPPORTED {
            if object.contains_key(*field) {
                bail!("unsupported manifest feature: {field}");
            }
            if let Some(arches) = value.get("architecture").and_then(Value::as_object) {
                for (arch, entry) in arches {
                    if entry.get(*field).is_some() {
                        bail!("unsupported manifest feature: architecture.{arch}.{field}");
                    }
                }
            }
        }
        static SCHEMA: OnceLock<jsonschema::Validator> = OnceLock::new();
        let validator = SCHEMA.get_or_init(|| {
            jsonschema::validator_for(
                &serde_json::from_str::<Value>(include_str!("../schema.json"))
                    .expect("embedded schema JSON"),
            )
            .expect("embedded schema is valid")
        });
        if let Some(error) = validator.iter_errors(&value).next() {
            bail!("manifest {}: {error}", error.instance_path);
        }
        let manifest = Self { value, raw };
        manifest.validate_fields(&manifest.value)?;
        if let Some(arches) = manifest
            .value
            .get("architecture")
            .and_then(Value::as_object)
        {
            for (arch, entry) in arches {
                ensure!(
                    crate::platform::normalize(arch)? == *arch,
                    "architecture key '{arch}' must be canonical"
                );
                manifest.validate_fields(entry)?;
                binaries(entry.get("bin").or(manifest.value.get("bin")))?;
            }
        }
        for dep in strings(manifest.value.get("depends")) {
            validate_dependency(&dep)?;
        }
        Ok(manifest)
    }
    fn validate_fields(&self, value: &Value) -> Result<()> {
        for field in ["extract_dir", "extract_to"] {
            for path in strings(value.get(field)) {
                relative(&path)?;
            }
        }
        binaries(value.get("bin"))?;
        for url in strings(value.get("url")) {
            let parsed = reqwest::Url::parse(&url).context("invalid artifact URL")?;
            ensure!(
                ["http", "https", "file"].contains(&parsed.scheme()),
                "unsupported artifact URL scheme '{}': {url}",
                parsed.scheme()
            );
            ensure!(
                !url.contains('$'),
                "artifact URL contains an unresolved variable"
            );
            if let Some(fragment) = parsed.fragment() {
                ensure!(
                    fragment.starts_with('/'),
                    "URL fragment must use #/filename"
                );
                relative(&fragment[1..])?;
            }
        }
        Ok(())
    }
    pub fn version(&self) -> &str {
        self.value["version"].as_str().expect("validated version")
    }
    pub fn target(&self, arch: &str) -> Result<Target> {
        ensure!(
            self.version() != "nightly",
            "unsupported manifest feature: nightly execution"
        );
        let entry =
            if let Some(arches) = self.value.get("architecture") {
                Some(arches.get(arch).with_context(|| {
                    format!("manifest has no exact architecture entry '{arch}'")
                })?)
            } else {
                None
            };
        let field = |key: &str| entry.and_then(|v| v.get(key)).or(self.value.get(key));
        let urls = strings(field("url"));
        let hashes = strings(field("hash"));
        ensure!(
            !urls.is_empty(),
            "installable manifest requires resolved URLs"
        );
        ensure!(
            hashes.is_empty() || hashes.len() == urls.len(),
            "URL/hash count mismatch"
        );
        let dirs = strings(field("extract_dir"));
        let destinations = strings(self.value.get("extract_to"));
        let mut archive_index = 0;
        let mut artifacts = Vec::new();
        for (i, url) in urls.into_iter().enumerate() {
            let archive = is_archive(&url)?;
            let (extract_dir, extract_to) = if archive {
                let result = (
                    dirs.get(archive_index).cloned(),
                    destinations.get(archive_index).cloned(),
                );
                archive_index += 1;
                result
            } else {
                (None, None)
            };
            artifacts.push(Artifact {
                url,
                hash: hashes
                    .get(i)
                    .map(|h| h.strip_prefix("sha256:").unwrap_or(h).to_ascii_lowercase()),
                extract_dir,
                extract_to,
            });
        }
        ensure!(
            dirs.len() <= archive_index && destinations.len() <= archive_index,
            "extraction entries exceed extracted archive count"
        );
        Ok(Target {
            architecture: arch.to_owned(),
            artifacts,
            bins: binaries(field("bin"))?,
            dependencies: strings(self.value.get("depends")),
        })
    }
    pub fn notes(&self, active: &Path, original: &Path) -> Vec<String> {
        strings(self.value.get("notes"))
            .into_iter()
            .map(|s| {
                s.replace("$original_dir", &original.to_string_lossy())
                    .replace("$dir", &active.to_string_lossy())
            })
            .collect()
    }
}
pub fn validate_dependency(value: &str) -> Result<()> {
    ensure!(
        !value.contains('@'),
        "dependency names do not support version qualifiers: {value}"
    );
    let parts: Vec<_> = value.split('/').collect();
    ensure!(parts.len() <= 2, "invalid dependency '{value}'");
    for part in parts {
        name(part)?;
    }
    Ok(())
}
pub fn is_archive(url: &str) -> Result<bool> {
    let filename = crate::download::filename(url)?;
    Ok([".zip", ".tar.gz", ".tar.xz", ".tgz"]
        .iter()
        .any(|ext| filename.to_ascii_lowercase().ends_with(ext)))
}
