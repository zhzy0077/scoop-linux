use scoop_linux::{
    archive,
    config::Config,
    download::{self, Options},
    manifest::Manifest,
};
use serde_json::json;
use std::{fs, io::Read};
#[test]
fn extraction_uses_verified_inode_across_atomic_cache_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data");
    let config = Config {
        path: temp.path().join("config"),
        root: temp.path().join("root"),
        cache: temp.path().join("cache"),
        values: Default::default(),
    };
    let make = |bytes: &[u8]| {
        fs::write(&source, bytes).unwrap();
        Manifest::parse(json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&source).unwrap().as_str(),"hash":download::digest(&source).unwrap()}).to_string()).unwrap()
    };
    let old = make(b"first verified bytes");
    let old_target = old.target("arm64", false).unwrap();
    let old_download = download::fetch(
        &config,
        "fixture",
        "1",
        &old_target.artifacts[0],
        &old,
        Options::default(),
    )
    .unwrap();
    let new = make(b"second verified bytes");
    let new_target = new.target("arm64", false).unwrap();
    let new_download = download::fetch(
        &config,
        "fixture",
        "1",
        &new_target.artifacts[0],
        &new,
        Options {
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(&old_download.path).unwrap(),
        b"second verified bytes"
    );
    let mut bytes = Vec::new();
    old_download
        .reader()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"first verified bytes");
    let stage = temp.path().join("stage");
    fs::create_dir(&stage).unwrap();
    archive::extract(&old_download, &old_target.artifacts[0], &stage).unwrap();
    assert_eq!(
        fs::read(stage.join("data")).unwrap(),
        b"first verified bytes"
    );
    drop(new_download);
}
