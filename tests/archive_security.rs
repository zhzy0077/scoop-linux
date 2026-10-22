use scoop_linux::{
    archive,
    config::Config,
    download::{self, Options},
    manifest::Manifest,
};
use serde_json::json;
use std::{fs, io::Write};
fn check(
    bytes: &[u8],
    filename: &str,
    extract_dir: Option<&str>,
    extract_to: Option<&str>,
) -> anyhow::Result<tempfile::TempDir> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join(filename);
    fs::write(&source, bytes)?;
    let config = Config {
        root: temp.path().join("root"),
        cache: temp.path().join("cache"),
        path: temp.path().join("config"),
        values: Default::default(),
    };
    let mut value = json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&source).unwrap().as_str(),"hash":download::digest(&source)?});
    if let Some(dir) = extract_dir {
        value["extract_dir"] = json!(dir)
    }
    if let Some(to) = extract_to {
        value["extract_to"] = json!(to)
    }
    let manifest = Manifest::parse(value.to_string())?;
    let target = manifest.target("arm64", false)?;
    let downloaded = download::fetch(
        &config,
        "test",
        "1",
        &target.artifacts[0],
        &manifest,
        Options::default(),
    )?;
    let stage = temp.path().join("stage");
    fs::create_dir(&stage)?;
    archive::extract(&downloaded, &target.artifacts[0], &stage)?;
    Ok(temp)
}
fn zip(path: &str, link: Option<&str>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    if let Some(target) = link {
        writer
            .add_symlink(path, target, zip::write::SimpleFileOptions::default())
            .unwrap();
    } else {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"content").unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn tar(path: &str, target: Option<(&str, tar::EntryType)>) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_size(if target.is_some() { 0 } else { 7 });
    h.set_mode(0o755);
    if let Some((target, kind)) = target {
        h.set_entry_type(kind);
        h.set_link_name(target).unwrap();
    }
    let bytes = h.as_mut_bytes();
    bytes[..100].fill(0);
    bytes[..path.len()].copy_from_slice(path.as_bytes());
    h.set_cksum();
    builder
        .append(
            &h,
            if target.is_some() {
                &b""[..]
            } else {
                &b"content"[..]
            },
        )
        .unwrap();
    let raw = builder.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&raw).unwrap();
    gzip.finish().unwrap()
}
#[test]
fn reject_zip_traversal_absolute_links_and_cycles() {
    for path in ["../outside", "/outside", "a/../../outside", "a\\outside"] {
        assert!(
            check(&zip(path, None), "test.zip", None, None).is_err(),
            "{path}"
        );
    }
    for target in ["../../outside", "/etc/passwd", "loop"] {
        assert!(
            check(&zip("loop", Some(target)), "test.zip", None, None).is_err(),
            "{target}"
        );
    }
}
#[test]
fn reject_tar_traversal_special_files_and_escaping_hardlinks() {
    for path in ["../outside", "/outside", "a/../../outside"] {
        assert!(
            check(&tar(path, None), "test.tar.gz", None, None).is_err(),
            "{path}"
        );
    }
    for (target, kind) in [
        ("/etc/passwd", tar::EntryType::Link),
        ("../outside", tar::EntryType::Link),
        ("/etc/passwd", tar::EntryType::Symlink),
        ("../outside", tar::EntryType::Symlink),
        ("missing", tar::EntryType::Link),
        ("loop", tar::EntryType::Symlink),
    ] {
        assert!(
            check(
                &tar("loop", Some((target, kind))),
                "test.tar.gz",
                None,
                None
            )
            .is_err(),
            "{target}"
        );
    }
    assert!(
        check(
            &tar("device", Some(("ignored", tar::EntryType::Char))),
            "test.tar.gz",
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn remapping_cannot_turn_safe_source_link_into_escape() {
    let upper = check(
        &zip("dir/file", None),
        "TEST.ZIP",
        Some("dir"),
        Some("mapped"),
    )
    .unwrap();
    assert_eq!(
        fs::read(upper.path().join("stage/mapped/file")).unwrap(),
        b"content"
    );
    let upper = check(&tar("file", None), "TEST.TAR.GZ", None, None).unwrap();
    assert_eq!(
        fs::read(upper.path().join("stage/file")).unwrap(),
        b"content"
    );
    let bytes = zip("sub/link", Some("../safe"));
    assert!(check(&bytes, "test.zip", Some("sub"), None).is_err());
    assert!(check(&bytes, "test.zip", Some("sub"), Some("nested")).is_ok());
}
