use crate::{
    config::Config,
    links, platform,
    resolve::Resolver,
    state::{self, Record},
};
use anyhow::{Context, Result, ensure};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
pub fn home(config: &Config, app: &str) -> Result<()> {
    let resolved = Resolver::new(config).resolve(app)?;
    let homepage = resolved.manifest.value["homepage"]
        .as_str()
        .context("homepage is missing")?;
    let url = reqwest::Url::parse(homepage)?;
    ensure!(
        ["https", "http"].contains(&url.scheme()),
        "homepage must use HTTP(S)"
    );
    let status = Command::new("xdg-open")
        .arg(url.as_str())
        .status()
        .context("xdg-open is required to open homepages")?;
    ensure!(status.success(), "xdg-open failed for {homepage}");
    Ok(())
}
fn writable(path: &Path) -> Result<()> {
    let mut ancestor = path;
    while !ancestor.exists() {
        ancestor = ancestor.parent().context("no existing parent")?;
    }
    ensure!(
        fs::metadata(ancestor)?.is_dir(),
        "path parent is not a directory: {}",
        ancestor.display()
    );
    let file = tempfile::NamedTempFile::new_in(ancestor)
        .with_context(|| format!("directory is not writable: {}", ancestor.display()))?;
    drop(file);
    Ok(())
}
pub fn checkup(config: &Config) -> Result<()> {
    let mut issues = Vec::new();
    let mut report = |name: &str, result: Result<()>| match result {
        Ok(()) => println!("OK: {name}"),
        Err(error) => {
            println!("WARN: {name}: {error:#}");
            issues.push(name.to_owned());
        }
    };
    report(
        "Git",
        (|| {
            ensure!(
                Command::new("git")
                    .arg("--version")
                    .output()?
                    .status
                    .success(),
                "Git is unavailable"
            );
            Ok(())
        })(),
    );
    report(
        "PATH",
        (|| {
            let path = std::env::var_os("PATH").context("PATH is missing")?;
            ensure!(
                std::env::split_paths(&path).any(|entry| fs::canonicalize(entry)
                    .ok()
                    .zip(fs::canonicalize(config.root.join("bin")).ok())
                    .is_some_and(|(entry, expected)| entry == expected)),
                "add {} to PATH",
                config.root.join("bin").display()
            );
            Ok(())
        })(),
    );
    for (name, path) in [
        ("root permissions", config.root.clone()),
        ("cache permissions", config.cache.clone()),
        (
            "config permissions",
            config.path.parent().context("config parent")?.to_owned(),
        ),
    ] {
        report(name, writable(&path));
    }
    for name in ["apps", "bin", ".staging", ".transactions", "buckets"] {
        let path = config.root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            report(
                &format!("managed {name}"),
                (|| {
                    ensure!(
                        fs::symlink_metadata(path)?.file_type().is_dir(),
                        "managed path is a file or symlink"
                    );
                    Ok(())
                })(),
            );
        }
    }
    let apps = config.root.join("apps");
    if fs::symlink_metadata(&apps).is_ok_and(|m| m.file_type().is_dir()) {
        for entry in fs::read_dir(&apps)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            report(
                &format!("installation {name}"),
                (|| {
                    ensure!(entry.file_type()?.is_dir(), "unmanaged app entry");
                    let (dir, record) = state::current(config, &name)?
                        .context("current installation missing or broken")?;
                    platform::select(Some(&record.architecture), None, &platform::host()?, true)?;
                    for version in fs::read_dir(entry.path())? {
                        let version = version?;
                        if version.file_name() == "current" {
                            continue;
                        }
                        ensure!(version.file_type()?.is_dir(), "unmanaged version entry");
                        state::version_name(
                            version.file_name().to_str().context("version UTF-8")?,
                        )?;
                        Record::load(&version.path())?;
                    }
                    for bin in &record.bins {
                        let declared = crate::archive::resolve_link(&dir, Path::new(bin))?;
                        let metadata = fs::metadata(&declared)
                            .with_context(|| format!("missing executable {bin}"))?;
                        ensure!(
                            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
                            "executable permission missing: {bin}"
                        );
                        let link = config.root.join("bin").join(links::basename(bin)?);
                        let owner = links::owner(&link)?.context("executable link missing")?;
                        if owner == name {
                            ensure!(
                                fs::read_link(&link)? == links::target(&name, bin),
                                "incorrect executable link: {}",
                                link.display()
                            );
                        } else {
                            eprintln!(
                                "Note: {} is currently owned by {owner}; reset {name} to reclaim it.",
                                links::basename(bin)?
                            );
                        }
                    }
                    if name != "scoop" {
                        let manifest = state::saved_manifest(&dir)?;
                        ensure!(
                            manifest.version()
                                == dir.file_name().unwrap_or_default().to_string_lossy(),
                            "manifest/current version mismatch"
                        );
                    }
                    Ok(())
                })(),
            );
        }
    }
    let bin = config.root.join("bin");
    if fs::symlink_metadata(&bin).is_ok_and(|m| m.file_type().is_dir()) {
        for entry in fs::read_dir(&bin)? {
            let entry = entry?;
            if !entry.file_type()?.is_symlink() {
                report(
                    &format!("link {}", entry.file_name().to_string_lossy()),
                    Err(anyhow::anyhow!("unmanaged executable entry")),
                );
            } else {
                report(
                    &format!("link {}", entry.file_name().to_string_lossy()),
                    (|| {
                        let owner = links::owner(&entry.path())?.context("unmanaged link")?;
                        ensure!(
                            state::current(config, &owner)?.is_some(),
                            "owner is not installed"
                        );
                        ensure!(
                            fs::canonicalize(entry.path())?.is_file(),
                            "link target is not a file"
                        );
                        Ok(())
                    })(),
                );
            }
        }
    }
    ensure!(
        issues.is_empty(),
        "checkup found {} issue(s): {}",
        issues.len(),
        issues.join(", ")
    );
    Ok(())
}
