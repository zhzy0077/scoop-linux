use scoop_linux::{config, download};
use serde_json::{Value, json};
use std::{fs, process::Command};
#[test]
fn config_concurrent_writers_preserve_all_keys() {
    let temp = tempfile::tempdir().unwrap();
    let mut children = Vec::new();
    for index in 0..12 {
        children.push(
            Command::new(env!("CARGO_BIN_EXE_scoop"))
                .args(["config", &format!("key{index}"), &format!("value{index}")])
                .env("HOME", temp.path())
                .env("XDG_CONFIG_HOME", temp.path().join("config"))
                .env("SCOOP", temp.path().join("root"))
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let value: Value =
        serde_json::from_slice(&fs::read(temp.path().join("config/scoop/config.json")).unwrap())
            .unwrap();
    for index in 0..12 {
        assert_eq!(value[format!("key{index}")], format!("value{index}"));
    }
}
#[test]
fn root_lock_serializes_install_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let artifact = temp.path().join("data");
    fs::write(&artifact, b"architecture-independent").unwrap();
    let manifest = temp.path().join("tool.json");
    fs::write(&manifest,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap()}).to_string()).unwrap();
    let lock = config::mutation_lock(&root).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scoop"))
        .arg("install")
        .arg(manifest)
        .env("HOME", temp.path())
        .env("SCOOP", &root)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(child.try_wait().unwrap().is_none());
    assert!(!root.join("apps/tool/current").exists());
    drop(lock);
    assert!(child.wait().unwrap().success());
    assert!(root.join("apps/tool/current").exists());
}
