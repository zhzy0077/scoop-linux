use serde_json::json;
use std::{fs, process::Command};
#[test]
fn orphan_temporary_and_corrupt_journals_are_app_scoped_and_actionable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    for part in ["apps", "bin", ".staging", ".transactions"] {
        fs::create_dir_all(root.join(part)).unwrap();
    }
    fs::write(root.join(".transactions/.tmpAb12Cd"), b"").unwrap();
    fs::write(
        root.join(".transactions/broken.json"),
        b"{\"format_version\":",
    )
    .unwrap();
    let cli = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", temp.path())
            .env("SCOOP", &root)
            .env("XDG_CONFIG_HOME", temp.path().join("config"));
        c
    };
    for args in [
        vec!["--version"],
        vec!["help"],
        vec!["config", "show_update_log", "true"],
        vec!["export"],
        vec!["cache", "show"],
    ] {
        let o = cli().args(&args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(String::from_utf8_lossy(&o.stderr).contains("broken.json"));
        assert!(!String::from_utf8_lossy(&o.stderr).contains(".tmpAb12Cd"));
    }
    let o = cli().args(["uninstall", "broken"]).output().unwrap();
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("unresolved transaction for broken"));
    assert!(e.contains("restore this journal from backup"));
    let artifact = temp.path().join("payload");
    fs::write(&artifact, "#!/bin/sh\nexit 0\n").unwrap();
    let path = temp.path().join("healthy.json");
    fs::write(&path,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":format!("{}#/healthy",reqwest::Url::from_file_path(&artifact).unwrap()),"hash":scoop_linux::download::digest(&artifact).unwrap(),"bin":"healthy"}).to_string()).unwrap();
    assert!(
        cli()
            .args(["install", path.to_str().unwrap(), "-u"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(cli().args(["list"]).output().unwrap().status.success());
    assert!(
        cli()
            .args(["uninstall", "healthy"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read(root.join(".transactions/.tmpAb12Cd")).unwrap(),
        b""
    );
    assert!(root.join(".transactions/broken.json").is_file());
}
#[test]
fn release_transport_rejects_http_catalog_and_artifact_before_activation() {
    assert!(
        scoop_linux::self_update::read("http://127.0.0.1:1/catalog.json")
            .err()
            .unwrap()
            .to_string()
            .contains("HTTPS")
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalog.json");
    fs::write(&path,json!({"format_version":1,"version":"1","targets":{"x86_64-unknown-linux-gnu":{"architecture":"64bit","os":"linux","libc":"glibc","abi":"gnu","minimum_libc":"2.17","url":"http://127.0.0.1:1/scoop","sha256":"a".repeat(64)}}}).to_string()).unwrap();
    assert!(scoop_linux::self_update::read(path.to_str().unwrap()).is_err());
}
