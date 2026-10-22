use crate::{config::Config, manifest, state};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Component, Path, PathBuf},
};
pub fn target(app: &str, bin: &str) -> PathBuf {
    Path::new("../apps").join(app).join("current").join(bin)
}
pub fn owner(path: &Path) -> Result<Option<String>> {
    let target = match fs::read_link(path) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_)
            if fs::symlink_metadata(path)
                .is_ok_and(|metadata| !metadata.file_type().is_symlink()) =>
        {
            anyhow::bail!("not a managed executable: {}", path.display())
        }
        Err(e) => return Err(e).context("cannot inspect managed executable"),
    };
    let parts: Vec<_> = target.components().collect();
    if parts.len() >= 5
        && parts[0] == Component::ParentDir
        && parts[1].as_os_str() == "apps"
        && parts[3].as_os_str() == "current"
        && let Some(app) = parts[2].as_os_str().to_str()
    {
        manifest::name(app)?;
        let tail: PathBuf = parts[4..].iter().collect();
        manifest::relative(tail.to_str().context("invalid executable link")?)?;
        return Ok(Some(app.into()));
    }
    anyhow::bail!(
        "not a managed executable: {} (unmanaged symlink)",
        path.display()
    )
}
pub fn validate_basename(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && !name.contains(['/', '\0']) && ![".", ".."].contains(&name),
        "a single executable basename is required"
    );
    Ok(())
}
pub fn basename(bin: &str) -> Result<&str> {
    Path::new(bin)
        .file_name()
        .and_then(|s| s.to_str())
        .context("invalid binary basename")
}
pub fn preflight(config: &Config, bins: &[String]) -> Result<()> {
    let dir = config.root.join("bin");
    state::managed_dir(&dir)?;
    for bin in bins {
        let path = dir.join(basename(bin)?);
        if fs::symlink_metadata(&path).is_ok() {
            let app = owner(&path)?.context("unmanaged executable conflict")?;
            crate::journal::ensure_clear(config, &app)?;
            ensure!(
                state::current(config, &app)?.is_some(),
                "executable link belongs to an unregistered package: {app}"
            );
        }
    }
    Ok(())
}
pub fn atomic_symlink(target: &Path, path: &Path) -> Result<()> {
    let temp = tempfile::Builder::new()
        .prefix(".link-")
        .tempdir_in(path.parent().context("link parent")?)?;
    let link = temp.path().join("link");
    symlink(target, &link)?;
    fs::rename(link, path)?;
    Ok(())
}
pub fn reconcile(config: &Config, app: &str, old: &[String], bins: &[String]) -> Result<()> {
    preflight(config, bins)?;
    for bin in bins {
        let path = config.root.join("bin").join(basename(bin)?);
        if fs::symlink_metadata(&path).is_ok() {
            let previous = owner(&path)?.context("unmanaged executable")?;
            if previous != app {
                println!("Replacing '{}' from {previous} with {app}.", basename(bin)?);
            }
        }
        atomic_symlink(&target(app, bin), &path)?;
    }
    for bin in old {
        if !bins.iter().any(|b| basename(b).ok() == basename(bin).ok()) {
            remove_owned(config, app, bin)?;
        }
    }
    Ok(())
}
pub fn remove_owned(config: &Config, app: &str, bin: &str) -> Result<()> {
    crate::journal::ensure_clear(config, app)?;
    let dir = config.root.join("bin");
    if fs::symlink_metadata(&dir).is_ok() {
        ensure!(
            fs::symlink_metadata(&dir)?.file_type().is_dir(),
            "bin is not a managed directory"
        );
    }
    let path = config.root.join("bin").join(basename(bin)?);
    if fs::symlink_metadata(&path).is_ok() && owner(&path).ok().flatten().as_deref() == Some(app) {
        fs::remove_file(path)?;
    }
    Ok(())
}
