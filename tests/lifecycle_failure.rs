use scoop_linux::{buckets::git, download, platform};
use serde_json::{Value, json};
use std::{fs, process::Command};
#[test]
fn operational_rejection_precedes_artifact_fetch_and_root_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bad.json");
    for extra in [
        json!({"persist":"data"}),
        json!({"bin":[["tool","alias"]]}),
        json!({"bin":["a/tool","b/tool"]}),
        json!({"bin":"scoop"}),
        json!({"bin":"bin/."}),
        json!({"env_set":{"X":"Y"}}),
    ] {
        let mut value = json!({"version":"1","homepage":"https://example.org","license":"MIT","url":"http://127.0.0.1:1/unreachable","hash":"a".repeat(64)});
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        fs::write(&path, value.to_string()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
            .arg("install")
            .arg(&path)
            .env("HOME", temp.path())
            .env("SCOOP", temp.path().join("root"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("error sending request"));
        assert!(!temp.path().join("root").exists());
    }
}
#[test]
fn completed_dependencies_survive_parent_failure_and_next_operand_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let repo = root.join("buckets/local");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "fixture@example.org"][..],
        &["config", "user.name", "Fixture"][..],
    ] {
        git(&repo, args).unwrap();
    }
    for name in ["dependency", "broken", "next"] {
        let artifact = temp.path().join(format!("{name}.sh"));
        fs::write(&artifact, format!("#!/bin/sh\nprintf {name}")).unwrap();
        let mut value = json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap(),"bin":format!("{name}.sh")});
        if name == "broken" {
            value["depends"] = json!("dependency");
            value["hash"] = json!("b".repeat(64));
        }
        fs::write(repo.join(format!("{name}.json")), value.to_string()).unwrap();
    }
    git(&repo, &["add", "."]).unwrap();
    git(&repo, &["commit", "-qm", "fixtures"]).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
        .args(["install", "broken", "next"])
        .env("HOME", temp.path())
        .env("SCOOP", &root)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    for name in ["dependency", "next"] {
        assert!(root.join(format!("apps/{name}/current")).exists());
        assert_eq!(
            Command::new(root.join(format!("bin/{name}.sh")))
                .output()
                .unwrap()
                .stdout,
            name.as_bytes()
        );
    }
    assert!(!root.join("apps/broken/current").exists());
}
#[test]
fn reject_mismatched_native_cpu_and_windows_content_before_activation() {
    let temp = tempfile::tempdir().unwrap();
    let host = platform::host().unwrap();
    let path = temp.path().join("native.json");
    for (index, content) in [
        (0, fs::read("/bin/echo").unwrap()),
        (1, b"MZWindows content".to_vec()),
    ] {
        let mut content = content;
        if index == 0 {
            let machine: u16 = if host == "arm64" { 62 } else { 183 };
            let encoded = if content[5] == 2 {
                machine.to_be_bytes()
            } else {
                machine.to_le_bytes()
            };
            content[18..20].copy_from_slice(&encoded);
        }
        let artifact = temp.path().join("native");
        fs::write(&artifact, content).unwrap();
        let value: Value = json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap(),"bin":"native","architecture":{host.clone():{}}});
        fs::write(&path, value.to_string()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
            .arg("install")
            .arg(&path)
            .env("HOME", temp.path())
            .env("SCOOP", temp.path().join("root"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!temp.path().join("root/apps/native/current").exists());
        assert!(!temp.path().join("root/bin/native").exists());
    }
}
