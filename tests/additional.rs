use scoop_linux::{config::Config, manifest::Manifest, resolve::Resolver};
use serde_json::{Value, json};
use std::{fs, process::Command};
fn value() -> Value {
    json!({"version":"1","homepage":"https://example.org","license":"MIT","url":"https://example.org/data","hash":"a".repeat(64)})
}
#[test]
fn metadata_uri_and_operational_shapes() {
    for (field, bad) in [
        ("homepage", json!("not a URL")),
        ("license", json!({"identifier":"MIT","unknown":true})),
        ("$schema", json!(42)),
        ("##", json!([true])),
        ("_comment", json!({})),
        ("architecture", json!([])),
        ("architecture", json!({})),
        ("architecture", json!({"aarch64":{}})),
        ("autoupdate", json!("script")),
        ("checkver", json!(true)),
        ("depends", json!("../escape")),
        ("url", json!("https://example.org/$version")),
        ("url", json!("https://example.org/a#/../escape")),
    ] {
        let mut v = value();
        v[field] = bad;
        assert!(
            Manifest::parse(v.to_string()).is_err(),
            "accepted {field}: {v}"
        );
    }
}
#[test]
fn xdg_default_config_and_data_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data/application");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("precious"), "preserve").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", temp.path())
            .env_remove("SCOOP")
            .env_remove("XDG_CONFIG_HOME")
            .env("XDG_DATA_HOME", temp.path().join("data"))
            .output()
            .unwrap()
    };
    assert!(
        run(&["config", "hold_update_until", "2030-01-01"])
            .status
            .success()
    );
    let config: Value =
        serde_json::from_slice(&fs::read(temp.path().join(".config/scoop/config.json")).unwrap())
            .unwrap();
    assert_eq!(config["hold_update_until"], "2030-01-01");
    for args in [
        &["uninstall", "tool"][..],
        &["cleanup", "--all"][..],
        &["reset", "--all"][..],
    ] {
        assert_eq!(run(args).status.success(), args[0] != "uninstall");
        assert_eq!(
            fs::read_to_string(data.join("precious")).unwrap(),
            "preserve"
        );
    }
    assert!(!temp.path().join("scoop/apps").exists());
}
#[test]
fn matching_history_works_when_head_requires_unsupported_features() {
    use scoop_linux::buckets::git;
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
    let mut v = value();
    fs::write(repo.join("tool.json"), v.to_string()).unwrap();
    git(&repo, &["add", "."]).unwrap();
    git(&repo, &["commit", "-qm", "old"]).unwrap();
    v["version"] = json!("2");
    v["persist"] = json!("data");
    fs::write(repo.join("tool.json"), v.to_string()).unwrap();
    git(&repo, &["add", "."]).unwrap();
    git(&repo, &["commit", "-qm", "new"]).unwrap();
    let mut config = Config {
        path: temp.path().join("config.json"),
        values: Default::default(),
        cache: root.join("cache"),
        root,
    };
    assert_eq!(
        Resolver::new(&config)
            .resolve("local/tool@1")
            .unwrap()
            .manifest
            .version(),
        "1"
    );
    assert!(Resolver::new(&config).resolve("local/tool").is_err());
    config.values.insert("use_git_history".into(), json!(false));
    assert!(Resolver::new(&config).resolve("local/tool@1").is_err());
}
