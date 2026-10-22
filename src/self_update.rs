use crate::{
    config::{self, Config},
    download, journal, lifecycle,
    manifest::{Manifest, Target},
    platform,
    resolve::{Resolved, Source},
    state::{self, Record},
    version,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTarget {
    pub architecture: String,
    pub os: String,
    pub libc: String,
    pub abi: String,
    pub minimum_libc: Option<String>,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub required_libraries: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format_version: u32,
    pub version: String,
    pub targets: BTreeMap<String, ReleaseTarget>,
    pub main_bucket_url: Option<String>,
}
pub const BUILD_TARGET: &str = env!("SCOOP_BUILD_TARGET");
pub fn location(config: &Config) -> Option<String> {
    std::env::var("SCOOP_RELEASE_CATALOG")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            config
                .values
                .get("release_catalog")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
}
pub fn read(location: &str) -> Result<Catalog> {
    ensure!(
        !location.contains("://")
            || location.starts_with("https://")
            || location.starts_with("file://"),
        "remote release catalogs require HTTPS (local paths/file URLs are allowed)"
    );
    let remote = location.starts_with("https://");
    let raw = if remote {
        crate::buckets::http_text(location)?
    } else {
        let path = if location.starts_with("file://") {
            reqwest::Url::parse(location)?
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid catalog file URL"))?
        } else {
            PathBuf::from(location)
        };
        ensure!(
            fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
            "release catalog exceeds 4 MiB"
        );
        fs::read_to_string(path)?
    };
    let value: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))?;
    let validator = jsonschema::validator_for(&serde_json::from_str::<serde_json::Value>(
        include_str!("../release-catalog.schema.json"),
    )?)?;
    if let Some(error) = validator.iter_errors(&value).next() {
        anyhow::bail!("release catalog {}: {error}", error.instance_path);
    }
    let mut catalog: Catalog = serde_json::from_value(value)?;
    ensure!(
        catalog.format_version == 1,
        "unsupported release catalog format"
    );
    state::version_name(&catalog.version)?;
    for target in catalog.targets.values_mut() {
        if target.url.starts_with("https://") {
            let url = reqwest::Url::parse(&target.url)?;
            ensure!(
                url.host_str().is_some(),
                "release artifact HTTPS URL requires a host"
            );
        } else {
            ensure!(!remote, "remote release catalog artifacts require HTTPS");
            if target.url.starts_with("file://") {
                reqwest::Url::parse(&target.url)?
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("invalid release artifact file URL"))?;
            } else {
                ensure!(
                    Path::new(&target.url).is_absolute(),
                    "remote release artifacts require HTTPS (local absolute paths/file URLs are allowed)"
                );
                target.url = reqwest::Url::from_file_path(&target.url)
                    .map_err(|_| anyhow::anyhow!("invalid release artifact path"))?
                    .to_string();
            }
        }
    }
    Ok(catalog)
}
pub fn runtime_libc() -> (&'static str, String) {
    #[cfg(target_env = "gnu")]
    {
        // glibc provides a process-lifetime, NUL-terminated version string.
        let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) }
            .to_string_lossy()
            .into_owned();
        ("glibc", version)
    }
    #[cfg(not(target_env = "gnu"))]
    {
        ("musl-static", String::new())
    }
}
pub fn compatible(catalog: &Catalog) -> Result<&ReleaseTarget> {
    let target = catalog
        .targets
        .get(BUILD_TARGET)
        .with_context(|| format!("release has no compatible target {BUILD_TARGET}"))?;
    platform::select(Some(&target.architecture), None, &platform::host()?, true)?;
    let (libc, libc_version) = runtime_libc();
    let abi = if cfg!(target_env = "gnu") {
        "gnu"
    } else {
        "musl"
    };
    ensure!(
        target.os == "linux" && target.libc == libc && target.abi == abi,
        "release libc/ABI is incompatible with {BUILD_TARGET}"
    );
    let loader = match std::env::consts::ARCH {
        "x86_64" => Some("ld-linux-x86-64.so.2"),
        "aarch64" => Some("ld-linux-aarch64.so.1"),
        _ => None,
    };
    ensure!(
        target
            .required_libraries
            .iter()
            .all(
                |name| ["libc.so.6", "libm.so.6", "libgcc_s.so.1"].contains(&name.as_str())
                    || Some(name.as_str()) == loader
            ),
        "release requires libraries outside the validated GNU runtime baseline"
    );
    if libc == "glibc" {
        let minimum = target
            .minimum_libc
            .as_deref()
            .context("glibc release requires minimum_libc")?;
        ensure!(
            minimum
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit())),
            "invalid minimum_libc"
        );
        ensure!(
            version::compare(&libc_version, minimum) != std::cmp::Ordering::Less,
            "release requires glibc {minimum}; host has {libc_version}"
        );
    }
    ensure!(
        target.sha256.len() == 64 && target.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
        "release requires a SHA-256 checksum"
    );
    Ok(target)
}
fn install(
    config: &Config,
    catalog: &Catalog,
    location: &str,
    respect_hold: bool,
) -> Result<Option<PathBuf>> {
    let target = compatible(catalog)?;
    let arch = platform::host()?;
    let homepage = if location.contains("://") {
        location.to_owned()
    } else {
        reqwest::Url::from_file_path(fs::canonicalize(location)?)
            .map_err(|_| anyhow::anyhow!("invalid catalog location"))?
            .to_string()
    };
    let value = serde_json::json!({"version":catalog.version,"homepage":homepage,"license":"Unknown","architecture":{arch.clone():{"url":target.url,"hash":target.sha256}}});
    // Package metadata is internal; release libc/ABI requirements stay exclusively in the catalog.
    let manifest = Manifest::parse(value.to_string())?;
    let resolved = Resolved {
        name: "scoop".into(),
        manifest,
        source: Source::Url(location.into()),
        alternatives: vec![],
    };
    let _lock = config::mutation_lock(&config.root)?;
    if respect_hold && config.path.exists() {
        let latest: serde_json::Value = serde_json::from_slice(&fs::read(&config.path)?)?;
        if latest
            .get("hold_update_until")
            .and_then(|value| value.as_str())
            .and_then(crate::maintenance::date)
            .is_some_and(|until| until > chrono::Utc::now())
        {
            return Ok(None);
        }
    }
    lifecycle::prepare_layout(config)?;
    journal::recover_locked(config)?;
    journal::ensure_clear(config, "scoop")?;
    let app = config.root.join("apps/scoop");
    state::managed_dir(&app)?;
    let previous = state::current(config, "scoop")?;
    if let Some((_, record)) = &previous {
        platform::select(Some(&record.architecture), None, &arch, true)?;
    }
    let version_dir = app.join(&catalog.version);
    if version_dir.exists() {
        ensure!(
            previous
                .as_ref()
                .is_some_and(|(dir, _)| dir == &version_dir),
            "retained Scoop version already exists; refusing unrelated replacement"
        );
        download::verify(&version_dir.join("scoop"), Some(&target.sha256), false)?;
        let record = Record::load(&version_dir)?;
        ensure!(record.bins == ["scoop"], "invalid Scoop binary record");
        journal::activate(
            config,
            "scoop",
            &catalog.version,
            None,
            false,
            &record.bins,
            &["scoop".into()],
        )?;
        return Ok(Some(version_dir.join("scoop")));
    }
    let stage = tempfile::Builder::new()
        .prefix("scoop-")
        .tempdir_in(config.root.join(".staging"))?;
    let artifact = resolved.manifest.target(&arch, false)?.artifacts.remove(0);
    let downloaded = download::fetch(
        config,
        "scoop",
        &catalog.version,
        &artifact,
        &resolved.manifest,
        download::Options {
            no_cache: true,
            ..Default::default()
        },
    )?;
    let binary = stage.path().join("scoop");
    let mut file = fs::File::create(&binary)?;
    std::io::copy(&mut downloaded.reader()?, &mut file)?;
    file.flush()?;
    file.set_permissions(fs::Permissions::from_mode(0o755))?;
    file.sync_all()?;
    drop(file);
    use std::io::Read;
    let mut magic = [0; 4];
    fs::File::open(&binary)?.read_exact(&mut magic)?;
    ensure!(
        &magic == b"\x7fELF",
        "release artifact must be a native Linux ELF binary"
    );
    let selected = Target {
        architecture: arch.clone(),
        artifacts: vec![artifact],
        bins: vec!["scoop".into()],
        dependencies: vec![],
    };
    lifecycle::validate_binaries(stage.path(), &resolved.manifest, &selected)?;
    let output = Command::new(&binary)
        .arg("--version")
        .output()
        .context("release binary is not executable on this CPU/libc")?;
    ensure!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|word| word == catalog.version),
        "release binary version does not match catalog"
    );
    fs::write(
        stage.path().join("scoop-manifest.json"),
        &resolved.manifest.raw,
    )?;
    config::atomic_json(
        &stage.path().join("scoop-install.json"),
        &Record::new(&resolved, &arch, vec!["scoop".into()]),
    )?;
    journal::activate(
        config,
        "scoop",
        &catalog.version,
        Some(stage.path()),
        false,
        &previous.map(|(_, record)| record.bins).unwrap_or_default(),
        &["scoop".into()],
    )?;
    Ok(Some(version_dir.join("scoop")))
}
pub fn update(config: &Config) -> Result<Option<PathBuf>> {
    let Some(location) = location(config) else {
        return Ok(None);
    };
    let catalog = read(&location)?;
    let current = state::current(config, "scoop")?
        .map(|(dir, _)| dir.file_name().unwrap().to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    if version::compare(&current, &catalog.version) != std::cmp::Ordering::Less {
        return Ok(None);
    }
    let binary = install(config, &catalog, &location, true)?;
    if binary.is_some() {
        println!("Updated Scoop to {} ({BUILD_TARGET}).", catalog.version);
    }
    Ok(binary)
}
/// Inspect release metadata without activation, configuration writes or handoff.
pub fn check(config: &Config) -> Result<()> {
    let Some(location) = location(config) else {
        return Ok(());
    };
    let catalog = read(&location)?;
    let current = state::current(config, "scoop")?
        .map(|(dir, _)| dir.file_name().unwrap().to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    if version::compare(&current, &catalog.version) == std::cmp::Ordering::Less {
        compatible(&catalog)?;
        eprintln!(
            "Scoop is out of date ({current} -> {}); run scoop update.",
            catalog.version
        );
    } else {
        eprintln!("Scoop is up to date ({current}).");
    }
    Ok(())
}
pub fn handoff(binary: &Path) -> Result<()> {
    let status = Command::new(binary)
        .args(std::env::args_os().skip(1))
        .env("SCOOP_INTERNAL_REFRESH_CONTINUE", "1")
        .status()?;
    use std::os::unix::process::ExitStatusExt;
    std::process::exit(
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
    );
}
pub fn bootstrap(config: &Config, location: &str) -> Result<()> {
    let location = if location.contains("://") {
        location.to_owned()
    } else {
        fs::canonicalize(location)?.to_string_lossy().into_owned()
    };
    let mut catalog = read(&location)?;
    let target = compatible(&catalog)?;
    if let Ok(expected) = std::env::var("SCOOP_BOOTSTRAP_SHA256") {
        ensure!(
            target.sha256.eq_ignore_ascii_case(&expected),
            "bootstrap catalog does not match the trusted seed SHA-256"
        );
        let seed = std::env::current_exe().context("locate verified bootstrap seed")?;
        download::verify(&seed, Some(&expected), false)?;
        // Reverify and install the seed bytes through the shared download service.
        catalog
            .targets
            .get_mut(BUILD_TARGET)
            .context("native bootstrap target")?
            .url = reqwest::Url::from_file_path(&seed)
            .map_err(|_| anyhow::anyhow!("invalid bootstrap seed path"))?
            .to_string();
    }
    let source = std::env::var("SCOOP_MAIN_BUCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            config
                .values
                .get("main_bucket_url")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .or(catalog.main_bucket_url.clone())
        .or_else(|| {
            crate::buckets::Buckets::known()
                .get("main")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .context("bootstrap requires a configured Linux main bucket")?;
    let binary = install(config, &catalog, &location, false)?
        .context("bootstrap installation was not activated")?;
    let buckets = crate::buckets::Buckets::new(&config.root);
    if !buckets.repo("main")?.exists() {
        buckets.add("main", Some(&source))?;
    }
    config.set("release_catalog", location.into())?;
    config.set("main_bucket_url", source.into())?;
    config.set("last_update", chrono::Utc::now().to_rfc3339().into())?;
    println!(
        "Installed Scoop: {}\nAdd this absolute directory to PATH:\n{}",
        binary.display(),
        config.root.join("bin").display()
    );
    Ok(())
}
