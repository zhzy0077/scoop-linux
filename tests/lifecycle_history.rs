use scoop_linux::{buckets::git, download};
use serde_json::{Value, json};
use std::{fs, process::Command};
#[test]
fn historical_install_snapshots_commit_and_reconciles_changed_binaries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let repo = root.join("buckets/local");
    fs::create_dir_all(repo.join("bucket")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "fixture@example.org"][..],
        &["config", "user.name", "Fixture"][..],
    ] {
        git(&repo, args).unwrap();
    }
    let mut commits = Vec::new();
    for version in ["1", "2"] {
        let filename = format!("tool-{version}.sh");
        let artifact = temp.path().join(&filename);
        fs::write(&artifact, format!("#!/bin/sh\nprintf version{version}")).unwrap();
        let manifest = json!({"version":version,"homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap(),"bin":filename});
        fs::write(repo.join("bucket/hist.json"), manifest.to_string()).unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-qm", version]).unwrap();
        commits.push(git(&repo, &["rev-parse", "HEAD"]).unwrap());
    }
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", temp.path())
            .env("SCOOP", &root)
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run(&["install", "local/hist@1"]);
    let record: Value =
        serde_json::from_slice(&fs::read(root.join("apps/hist/1/scoop-install.json")).unwrap())
            .unwrap();
    assert_eq!(record["bucket"], "local");
    assert_eq!(record["bucket_commit"], commits[0]);
    assert_eq!(record["bucket_dirty"], false);
    assert_eq!(record["manifest_path"], "bucket/hist.json");
    assert_eq!(
        Command::new(root.join("bin/tool-1.sh"))
            .output()
            .unwrap()
            .stdout,
        b"version1"
    );
    let current_manifest = repo.join("bucket/hist.json");
    let mut edited: Value = serde_json::from_slice(&fs::read(&current_manifest).unwrap()).unwrap();
    edited["notes"] = json!("local author edit");
    fs::write(&current_manifest, edited.to_string()).unwrap();
    run(&["install", "local/hist@2"]);
    let updated: Value =
        serde_json::from_slice(&fs::read(root.join("apps/hist/2/scoop-install.json")).unwrap())
            .unwrap();
    assert_eq!(updated["bucket_commit"], commits[1]);
    assert_eq!(updated["bucket_dirty"], true);
    assert_eq!(
        fs::read_to_string(root.join("apps/hist/2/scoop-manifest.json")).unwrap(),
        edited.to_string()
    );
    assert_eq!(
        Command::new(root.join("bin/tool-2.sh"))
            .output()
            .unwrap()
            .stdout,
        b"version2"
    );
    assert!(!root.join("bin/tool-1.sh").exists());
    assert!(root.join("apps/hist/1/scoop-manifest.json").exists());
    run(&["uninstall", "hist"]);
    assert!(!root.join("apps/hist").exists());
    assert!(!root.join("bin/tool-2.sh").exists());
}
