use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde_json::{Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub values: Map<String, Value>,
    pub root: PathBuf,
    pub cache: PathBuf,
}
impl Config {
    pub fn load() -> Result<Self> {
        #[allow(deprecated)]
        let home = env::home_dir()
            .filter(|p| p.is_absolute())
            .context("an absolute home directory is required")?;
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let path = config_home.join("scoop/config.json");
        let values: Map<String, Value> = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).with_context(|| format!("invalid config JSON object at {}; repair this file before retrying (original content preserved)", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(e.into()),
        };
        for key in [
            "root_path",
            "cache_path",
            "default_architecture",
            "main_bucket_url",
        ] {
            ensure!(
                values
                    .get(key)
                    .is_none_or(|value| value.is_string() || value.is_null()),
                "config {key} must be a string"
            );
        }
        let root = env::var_os("SCOOP")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                values
                    .get("root_path")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| home.join("scoop"));
        ensure!(root.is_absolute(), "SCOOP/root_path must be absolute");
        let cache = values
            .get("cache_path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("cache"));
        ensure!(cache.is_absolute(), "cache_path must be absolute");
        Ok(Self {
            path,
            values,
            root,
            cache,
        })
    }
    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        let parent = self.path.parent().context("config parent")?;
        fs::create_dir_all(parent)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(parent.join(".config.lock"))?;
        lock.lock_exclusive()?;
        let mut values: Map<String, Value> = match fs::read(&self.path) {
            Ok(raw) => serde_json::from_slice(&raw)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(error) => return Err(error.into()),
        };
        if value.is_null() {
            values.remove(key);
        } else {
            values.insert(key.into(), value);
        }
        atomic_json(&self.path, &values)
    }
    pub fn architecture(&self) -> Option<&str> {
        self.values
            .get("default_architecture")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }
    pub fn command(&mut self, args: &[String]) -> Result<()> {
        if args.is_empty() {
            println!("{}", serde_json::to_string_pretty(&self.values)?);
            return Ok(());
        }
        let remove = args[0] == "rm";
        let key = args
            .get(usize::from(remove))
            .context("usage: scoop config rm <name>")?;
        if !remove && args.len() == 1 {
            if let Some(v) = self.values.get(key) {
                if let Some(s) = v.as_str() {
                    println!("{s}");
                } else {
                    println!("{v}");
                }
            } else {
                println!("'{key}' is not set");
            }
            return Ok(());
        }
        ensure!(args.len() == 2, "usage: scoop config [rm] <name> [value]");
        const UNSUPPORTED: &[&str] = &[
            "scoop_repo",
            "scoop_branch",
            "global_path",
            "use_sqlite_cache",
            "use_external_7zip",
            "aria2_enabled",
            "use_lessmsi",
            "no_junction",
            "shim",
            "path_env_var",
        ];
        if !remove && (UNSUPPORTED.contains(&key.as_str()) || key.starts_with("aria2_")) {
            bail!("unsupported config feature: {key}");
        }
        let parent = self.path.parent().context("config parent")?;
        fs::create_dir_all(parent)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(parent.join(".config.lock"))?;
        lock.lock_exclusive()?;
        // Reload under the lock to preserve unrelated concurrent configuration changes.
        let mut values: Map<String, Value> = match fs::read(&self.path) {
            Ok(raw) => serde_json::from_slice(&raw)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(e.into()),
        };
        if remove {
            values.remove(key);
        } else {
            let raw = &args[1];
            let value = match raw.to_ascii_lowercase().as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "$null" | "null" => Value::Null,
                _ => Value::String(raw.clone()),
            };
            match key.as_str() {
                _ if value.is_null() => {}
                "root_path" | "cache_path" => ensure!(
                    value.as_str().is_some_and(|s| Path::new(s).is_absolute()),
                    "{key} must be an absolute path"
                ),
                "default_architecture" => {
                    crate::platform::normalize(
                        value
                            .as_str()
                            .context("default_architecture must be a string")?,
                    )?;
                }
                "force_update" | "show_update_log" | "use_git_history" => {
                    ensure!(value.is_boolean(), "{key} must be a boolean")
                }
                _ => {}
            }
            if value.is_null() {
                values.remove(key);
            } else {
                values.insert(key.clone(), value);
            }
        }
        atomic_json(&self.path, &values)?;
        self.values = values;
        if remove {
            println!("'{key}' has been removed");
        } else {
            println!("'{key}' has been set to '{}'", args[1]);
        }
        Ok(())
    }
}
pub fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let parent = path.parent().context("metadata parent")?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    serde_json::to_writer_pretty(&mut tmp, value)?;
    tmp.write_all(b"\n")?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn mutation_lock(root: &Path) -> Result<fs::File> {
    fs::create_dir_all(root)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}
