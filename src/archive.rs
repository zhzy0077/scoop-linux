use crate::{download::Downloaded, manifest::Artifact};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Component, Path, PathBuf},
};

pub fn clean(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(p) => result.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                ensure!(result.pop(), "escaping archive path: {}", path.display())
            }
            _ => bail!("absolute archive path: {}", path.display()),
        }
    }
    ensure!(
        !path.to_string_lossy().contains(['\\', '\0']),
        "unsafe archive path"
    );
    Ok(result)
}
fn destination(root: &Path, relative: &Path) -> Result<PathBuf> {
    let relative = clean(relative)?;
    let mut path = root.to_owned();
    for component in relative.components() {
        path.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "archive path traverses a symlink: {}",
                path.display()
            );
        }
    }
    Ok(path)
}
fn directory(root: &Path, relative: &Path) -> Result<PathBuf> {
    let path = destination(root, relative)?;
    fs::create_dir_all(&path)?;
    Ok(path)
}
fn regular(root: &Path, relative: &Path, reader: &mut impl Read, mode: u32) -> Result<()> {
    let path = destination(root, relative)?;
    fs::create_dir_all(path.parent().context("archive parent")?)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("archive file conflict: {}", path.display()))?;
    std::io::copy(reader, &mut file)?;
    file.flush()?;
    file.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
    Ok(())
}
// Resolve symlinks lexically, including dangling targets, without ever traversing outside root.
pub fn resolve_link(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut pending = clean(relative)?;
    for _ in 0..40 {
        let mut prefix = PathBuf::new();
        let components: Vec<_> = pending.components().collect();
        let mut expanded = false;
        for (index, component) in components.iter().enumerate() {
            prefix.push(component);
            if fs::symlink_metadata(root.join(&prefix)).is_ok_and(|m| m.file_type().is_symlink()) {
                let target = fs::read_link(root.join(&prefix))?;
                ensure!(!target.is_absolute(), "absolute archive symlink target");
                let mut next = prefix.parent().unwrap_or(Path::new("")).join(target);
                for rest in &components[index + 1..] {
                    next.push(rest);
                }
                pending = clean(&next)?;
                expanded = true;
                break;
            }
        }
        if !expanded {
            return Ok(root.join(pending));
        }
    }
    bail!("archive symlink cycle or excessive indirection")
}
pub fn validate_tree(root: &Path) -> Result<()> {
    fn walk(root: &Path, dir: &Path) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                resolve_link(root, entry.path().strip_prefix(root)?)?;
            } else if kind.is_dir() {
                walk(root, &entry.path())?;
            } else {
                ensure!(kind.is_file(), "unsupported special archive file");
            }
        }
        Ok(())
    }
    walk(root, root)
}
fn tar_extract(reader: impl Read, root: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let mut links = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = clean(&entry.path()?)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            directory(root, &path)?;
        } else if kind.is_file() {
            let mode = entry.header().mode()?;
            regular(root, &path, &mut entry, mode)?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()?
                .context("archive link lacks target")?
                .into_owned();
            links.push((path, target, kind.is_hard_link()));
        } else {
            bail!("unsupported archive entry type for {}", path.display());
        }
    }
    // Symlinks cannot become write destinations for regular entries or other links.
    for (path, target, hard) in &links {
        if *hard {
            continue;
        }
        ensure!(!target.is_absolute(), "absolute archive symlink target");
        clean(&path.parent().unwrap_or(Path::new("")).join(target))?;
        let output = destination(root, path)?;
        fs::create_dir_all(output.parent().context("link parent")?)?;
        symlink(target, output)?;
    }
    let mut hardlinks: Vec<_> = links.into_iter().filter(|(_, _, hard)| *hard).collect();
    while !hardlinks.is_empty() {
        let mut remaining = Vec::new();
        let before = hardlinks.len();
        for (path, target, hard) in hardlinks {
            let source = resolve_link(root, &clean(&target)?)?;
            if !source.exists() {
                remaining.push((path, target, hard));
                continue;
            }
            ensure!(
                fs::metadata(&source)?.is_file(),
                "hardlink must target a regular file"
            );
            let output = destination(root, &path)?;
            ensure!(!output.exists(), "duplicate archive link");
            fs::create_dir_all(output.parent().context("link parent")?)?;
            fs::hard_link(source, output)?;
        }
        ensure!(
            remaining.len() < before,
            "archive hardlink target missing or cyclic"
        );
        hardlinks = remaining;
    }
    validate_tree(root)
}
fn zip_extract(file: fs::File, root: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(file)?;
    let mut links = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let path = clean(Path::new(entry.name()))?;
        let mode = entry.unix_mode().unwrap_or(0o644);
        if entry.is_dir() {
            directory(root, &path)?;
        } else if mode & 0o170000 == 0o120000 {
            let mut target = String::new();
            entry.read_to_string(&mut target)?;
            links.push((path, PathBuf::from(target)));
        } else {
            ensure!(
                mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
                "unsupported ZIP special file"
            );
            regular(root, &path, &mut entry, mode)?;
        }
    }
    for (path, target) in links {
        ensure!(!target.is_absolute(), "absolute ZIP symlink target");
        clean(&path.parent().unwrap_or(Path::new("")).join(&target))?;
        let output = destination(root, &path)?;
        fs::create_dir_all(output.parent().context("link parent")?)?;
        symlink(target, output)?;
    }
    validate_tree(root)
}
fn copy_tree(
    source: &Path,
    output: &Path,
    root: &Path,
    inodes: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = output.join(entry.file_name());
        let relative = target.strip_prefix(root)?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            directory(root, relative)?;
            copy_tree(&entry.path(), &target, root, inodes)?;
        } else if kind.is_symlink() {
            let target_path = destination(root, relative)?;
            symlink(fs::read_link(entry.path())?, target_path)?;
        } else {
            let metadata = entry.metadata()?;
            let key = (metadata.dev(), metadata.ino());
            if let Some(previous) = inodes.get(&key) {
                fs::hard_link(previous, destination(root, relative)?)?;
            } else {
                regular(
                    root,
                    relative,
                    &mut fs::File::open(entry.path())?,
                    metadata.permissions().mode(),
                )?;
                if metadata.nlink() > 1 {
                    inodes.insert(key, target);
                }
            }
        }
    }
    Ok(())
}
pub fn extract(download: &Downloaded, artifact: &Artifact, stage: &Path) -> Result<()> {
    let filename = download.filename.to_ascii_lowercase();
    if [".zip", ".tar.gz", ".tar.xz", ".tgz"]
        .iter()
        .any(|s| filename.ends_with(s))
    {
        let temp = tempfile::tempdir_in(stage.parent().context("stage parent")?)?;
        if filename.ends_with(".zip") {
            zip_extract(download.reader()?, temp.path())?;
        } else if filename.ends_with(".tar.xz") {
            tar_extract(xz2::read::XzDecoder::new(download.reader()?), temp.path())?;
        } else {
            tar_extract(
                flate2::read::GzDecoder::new(download.reader()?),
                temp.path(),
            )?;
        }
        let subtree = if let Some(dir) = &artifact.extract_dir {
            destination(temp.path(), Path::new(dir))?
        } else {
            temp.path().to_owned()
        };
        ensure!(
            subtree.is_dir(),
            "extract_dir does not exist: {}",
            subtree.display()
        );
        let output = directory(
            stage,
            Path::new(artifact.extract_to.as_deref().unwrap_or(".")),
        )?;
        copy_tree(&subtree, &output, stage, &mut HashMap::new())?;
    } else {
        regular(
            stage,
            Path::new(&download.filename),
            &mut download.reader()?,
            0o644,
        )?;
    }
    validate_tree(stage)
}
