use crate::{
    config::Config,
    manifest::{self, Artifact, Manifest},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub fn filename(url: &str) -> Result<String> {
    let url = reqwest::Url::parse(url)?;
    let raw = url
        .fragment()
        .map(|s| s.strip_prefix('/').unwrap_or(s))
        .unwrap_or_else(|| url.path().rsplit('/').next().unwrap_or(""));
    let name = percent_encoding::percent_decode_str(raw)
        .decode_utf8()?
        .into_owned();
    manifest::relative(&name)?;
    ensure!(
        !name.contains('/') && name != "." && name != "..",
        "download filename must be a basename: {name}"
    );
    Ok(name)
}
pub fn digest(path: &Path) -> Result<String> {
    digest_reader(&mut fs::File::open(path)?)
}
fn digest_reader(file: &mut impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn verify(path: &Path, expected: Option<&str>, skip: bool) -> Result<()> {
    verify_labeled(path, expected, skip, &path.display().to_string())
}
fn verify_labeled(
    path: &Path,
    expected: Option<&str>,
    skip: bool,
    description: &str,
) -> Result<()> {
    if !skip {
        let expected = expected.context("SHA-256 checksum required")?;
        ensure!(
            digest(path)? == expected.to_ascii_lowercase(),
            "SHA-256 checksum mismatch for {description}"
        );
    }
    Ok(())
}
pub struct Downloaded {
    pub path: PathBuf,
    pub filename: String,
    file: fs::File,
    _temporary: Option<tempfile::NamedTempFile>,
}
impl Downloaded {
    // Keep the verified inode open across atomic replacements in a shared cache.
    pub fn reader(&self) -> Result<fs::File> {
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }
}
#[derive(Default, Clone, Copy)]
pub struct Options {
    pub force: bool,
    pub no_cache: bool,
    pub skip_hash: bool,
}
pub fn fetch(
    config: &Config,
    app: &str,
    version: &str,
    artifact: &Artifact,
    manifest: &Manifest,
    options: Options,
) -> Result<Downloaded> {
    let filename = filename(&artifact.url)?;
    let key = format!("{:x}", Sha256::digest(artifact.url.as_bytes()));
    let cache = config
        .cache
        .join(format!("{app}#{version}#{key}#{filename}"));
    if !options.no_cache {
        fs::create_dir_all(config.cache.parent().context("cache parent")?)?;
        crate::state::managed_dir(&config.cache)?;
    }
    if !options.no_cache && !options.force && fs::symlink_metadata(&cache).is_ok() {
        ensure!(
            fs::symlink_metadata(&cache)?.file_type().is_file(),
            "cache entry is not a regular file"
        );
        let mut file = fs::File::open(&cache)?;
        let valid = options.skip_hash
            || artifact.hash.as_deref().is_some_and(|expected| {
                digest_reader(&mut file).is_ok_and(|actual| actual == expected.to_ascii_lowercase())
            });
        if valid {
            return Ok(Downloaded {
                path: cache,
                filename,
                file,
                _temporary: None,
            });
        }
        eprintln!("Invalid cached content for {app}; downloading again.");
        fs::remove_file(&cache)?;
    }
    let mut temporary = if options.no_cache {
        tempfile::NamedTempFile::new()?
    } else {
        tempfile::NamedTempFile::new_in(&config.cache)?
    };
    let mut url = reqwest::Url::parse(&artifact.url)?;
    url.set_fragment(None);
    match url.scheme() {
        "file" => {
            let source = url
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URL"))?;
            std::io::copy(&mut fs::File::open(source)?, &mut temporary)?;
        }
        "http" | "https" => {
            let client = crate::network::client(300)?;
            let mut request = client.get(url);
            if let Some(cookies) = manifest.value.get("cookie").and_then(|v| v.as_object()) {
                let mut values = Vec::new();
                for (key, value) in cookies {
                    let value = value.as_str().context("cookie value must be a string")?;
                    ensure!(
                        !key.contains([';', '\r', '\n', '=']) && !value.contains([';', '\r', '\n']),
                        "invalid cookie name/value"
                    );
                    values.push(format!("{key}={value}"));
                }
                request = request.header(reqwest::header::COOKIE, values.join("; "));
            }
            let mut response = request.send()?.error_for_status()?;
            ensure!(
                !response.status().is_redirection(),
                "download redirect policy refused the redirect"
            );
            std::io::copy(&mut response, &mut temporary)?;
        }
        scheme => anyhow::bail!("unsupported download scheme: {scheme}"),
    }
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    verify_labeled(
        temporary.path(),
        artifact.hash.as_deref(),
        options.skip_hash,
        &format!("{filename} ({})", artifact.url),
    )?;
    let file = temporary.as_file().try_clone()?;
    if options.no_cache {
        Ok(Downloaded {
            path: temporary.path().to_owned(),
            filename,
            file,
            _temporary: Some(temporary),
        })
    } else {
        temporary.persist(&cache)?;
        fs::File::open(&config.cache)?.sync_all()?;
        Ok(Downloaded {
            path: cache,
            filename,
            file,
            _temporary: None,
        })
    }
}
