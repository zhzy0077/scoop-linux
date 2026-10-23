use crate::{archive, manifest::Binary};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub const DIRECTORY: &str = ".scoop-shims";

/// Match Scoop's target lookup: package file, explicit file, then a command on PATH.
pub fn resolve(package: &Path, bin: &Binary) -> Result<(PathBuf, bool)> {
    if let Some(path) = &bin.external {
        ensure!(
            path.is_file(),
            "external shim target is missing: {}",
            path.display()
        );
        return Ok((path.clone(), false));
    }
    let path = Path::new(&bin.path);
    if !path.is_absolute() {
        let local = archive::resolve_link(package, path)?;
        if local.is_file() {
            return Ok((local, true));
        }
    }
    if path.is_file() {
        return Ok((fs::canonicalize(path)?, false));
    }
    if path.components().count() == 1 {
        for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let candidate = directory.join(path);
            if fs::metadata(&candidate)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            {
                let resolved = fs::canonicalize(candidate)?;
                // Reinstalling a PATH shim must resolve its original target, not itself.
                if let Some(directory) = resolved.parent()
                    && directory.file_name().is_some_and(|name| name == DIRECTORY)
                    && let Some(version) = directory.parent()
                    && let Ok(record) = crate::state::Record::load(version)
                    && let Some(saved) = record
                        .bins
                        .iter()
                        .find(|saved| saved.path == bin.path && saved.name == bin.name)
                {
                    if let Some(external) = &saved.external {
                        ensure!(
                            external.is_file(),
                            "external shim target is missing: {}",
                            external.display()
                        );
                        return Ok((external.clone(), false));
                    }
                    continue;
                }
                return Ok((resolved, false));
            }
        }
    }
    anyhow::bail!("shim target does not exist: {}", bin.path)
}

fn launcher(
    package: &Path,
    original: &Path,
    current: &Path,
    persist: &Path,
    bin: &Binary,
) -> Result<Option<String>> {
    let (resolved, packaged) = resolve(package, bin)?;
    if packaged && bin.args.is_empty() {
        return Ok(None);
    }
    let executable = if packaged {
        original.join(&bin.path)
    } else {
        resolved
    };
    let mut arguments = vec![
        executable
            .to_str()
            .context("shim target must be UTF-8")?
            .to_owned(),
    ];
    arguments.extend(bin.args.iter().map(|arg| {
        arg.replace("$original_dir", &original.to_string_lossy())
            .replace("$persist_dir", &persist.to_string_lossy())
            .replace("$dir", &current.to_string_lossy())
    }));
    let command = shlex::try_join(arguments.iter().map(String::as_str))?;
    Ok(Some(format!("#!/bin/sh\nexec {command} \"$@\"\n")))
}

pub fn prepare(
    package: &Path,
    original: &Path,
    current: &Path,
    persist: &Path,
    bins: &mut [Binary],
) -> Result<()> {
    let directory = package.join(DIRECTORY);
    fs::create_dir(&directory)?;
    for bin in bins {
        let (resolved, packaged) = resolve(package, bin)?;
        if !packaged {
            bin.external = Some(resolved);
        }
        let output = directory.join(&bin.name);
        if let Some(script) = launcher(package, original, current, persist, bin)? {
            fs::write(&output, script)?;
            fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
        } else {
            std::os::unix::fs::symlink(Path::new("..").join(&bin.path), output)?;
        }
    }
    Ok(())
}

pub fn verify(package: &Path, current: &Path, persist: &Path, bins: &[Binary]) -> Result<()> {
    ensure!(
        fs::symlink_metadata(package.join(DIRECTORY))?
            .file_type()
            .is_dir(),
        "shim directory is not managed"
    );
    for bin in bins {
        let output = package.join(DIRECTORY).join(&bin.name);
        let metadata = fs::symlink_metadata(&output)?;
        if let Some(expected) = launcher(package, package, current, persist, bin)? {
            ensure!(
                metadata.is_file()
                    && metadata.permissions().mode() & 0o111 != 0
                    && fs::read_to_string(output)? == expected,
                "invalid shim for {}",
                bin.name
            );
        } else {
            ensure!(
                metadata.file_type().is_symlink()
                    && fs::read_link(output)? == Path::new("..").join(&bin.path),
                "invalid shim for {}",
                bin.name
            );
        }
    }
    Ok(())
}
