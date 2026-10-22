#![cfg(feature = "test-hooks")]
use serde_json::json;
use std::{fs, process::Command};
#[test]
fn corrupt_journal_does_not_prevent_other_transaction_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join(".transactions")).unwrap();
    fs::write(root.join(".transactions/broken.json"), b"{").unwrap();
    let payload = temp.path().join("payload");
    fs::write(&payload, "#!/bin/sh\nexit 0\n").unwrap();
    let manifest = temp.path().join("healthy.json");
    fs::write(&manifest,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":format!("{}#/healthy",reqwest::Url::from_file_path(&payload).unwrap()),"hash":scoop_linux::download::digest(&payload).unwrap(),"bin":"healthy"}).to_string()).unwrap();
    let cli = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", temp.path())
            .env("SCOOP", &root)
            .env("XDG_CONFIG_HOME", temp.path().join("config"));
        c
    };
    let o = cli()
        .arg("install")
        .arg(&manifest)
        .arg("-u")
        .env("SCOOP_TEST_FAILPOINT", "prepared")
        .env("SCOOP_TEST_CRASH", "1")
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(root.join(".transactions/healthy.json").is_file());
    let o = cli().arg("--version").output().unwrap();
    assert!(o.status.success());
    assert!(root.join(".transactions/broken.json").is_file());
    assert!(!root.join(".transactions/healthy.json").exists());
    assert_eq!(fs::read_dir(root.join(".staging")).unwrap().count(), 0);
}
