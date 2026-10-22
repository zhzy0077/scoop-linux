use scoop_linux::{buckets::git, download};
use serde_json::{Value, json};
use std::{fs, process::Command};
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        let source = f.temp.path().join("source");
        fs::create_dir_all(source.join("bucket")).unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "fixture@example.org"][..],
            &["config", "user.name", "Fixture"][..],
        ] {
            git(&source, args).unwrap();
        }
        f.publish("1");
        let source = source.to_str().unwrap();
        f.ok(&["bucket", "add", "local", source]);
        f
    }
    fn cli(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", self.temp.path())
            .env("SCOOP", self.temp.path().join("root"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"));
        c
    }
    fn ok(&self, args: &[&str]) -> String {
        let out = self.cli().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn publish(&self, version: &str) {
        let artifact = self.temp.path().join(format!("source-{version}"));
        fs::write(&artifact, format!("#!/bin/sh\nprintf {version}")).unwrap();
        let mut url = reqwest::Url::from_file_path(&artifact).unwrap();
        url.set_fragment(Some("/tool"));
        let source = self.temp.path().join("source");
        fs::write(source.join("bucket/tool.json"),json!({"version":version,"homepage":"https://example.org","license":"MIT","url":url.as_str(),"hash":download::digest(&artifact).unwrap(),"bin":"tool"}).to_string()).unwrap();
        git(&source, &["add", "."]).unwrap();
        git(&source, &["commit", "-qm", version]).unwrap();
    }
    fn stale(&self) {
        self.ok(&["config", "last_update", "2000-01-01T00:00:00Z"]);
    }
    fn current(&self) -> String {
        fs::read_link(self.temp.path().join("root/apps/tool/current"))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
    fn bucket_version(&self) -> String {
        let value: Value = serde_json::from_slice(
            &fs::read(self.temp.path().join("root/buckets/local/bucket/tool.json")).unwrap(),
        )
        .unwrap();
        value["version"].as_str().unwrap().to_owned()
    }
}
#[test]
fn automatic_refresh_three_hours_no_update_flags_local_status_and_manual_update() {
    let f = Fixture::new();
    f.publish("2");
    f.stale();
    f.ok(&["download", "tool", "-u"]);
    assert_eq!(f.bucket_version(), "1");
    f.ok(&["install", "tool", "--no-update-scoop"]);
    assert_eq!(f.current(), "1");
    assert_eq!(f.bucket_version(), "1");
    f.ok(&["update", "tool"]);
    assert_eq!(f.current(), "2");
    assert_eq!(f.bucket_version(), "2");
    let config: Value =
        serde_json::from_slice(&fs::read(f.temp.path().join("config/scoop/config.json")).unwrap())
            .unwrap();
    let last = scoop_linux::maintenance::date(config["last_update"].as_str().unwrap()).unwrap();
    assert!(chrono::Utc::now().signed_duration_since(last).num_seconds() < 60);
    f.publish("3");
    f.ok(&["download", "tool"]);
    assert_eq!(f.bucket_version(), "2");
    f.stale();
    assert!(!f.ok(&["status", "--local"]).contains("tool\t2\t3"));
    assert_eq!(f.bucket_version(), "2");
    let before = f.ok(&["config", "last_update"]);
    let out = f.cli().arg("status").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("remote changes"));
    assert_eq!(f.bucket_version(), "2");
    assert_eq!(f.ok(&["config", "last_update"]), before);
    f.publish("4");
    f.ok(&["update"]);
    assert_eq!(f.bucket_version(), "4");
    assert_eq!(f.current(), "2");
    f.ok(&["update", "*"]);
    assert_eq!(f.current(), "4");
}
#[test]
fn dirty_buckets_are_preserved_and_refresh_attempt_advances_timestamp() {
    let f = Fixture::new();
    f.stale();
    let path = f.temp.path().join("root/buckets/local/bucket/tool.json");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\n ")).unwrap();
    f.publish("2");
    let out = f.cli().arg("update").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("local changes"));
    assert_eq!(fs::read_to_string(path).unwrap(), format!("{original}\n "));
    assert_ne!(f.ok(&["config", "last_update"]), "2000-01-01T00:00:00Z\n");
}
#[test]
fn refresh_due_boundaries_and_reference_version_order() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = scoop_linux::config::Config {
        path: temp.path().join("config"),
        root: temp.path().join("root"),
        cache: temp.path().join("cache"),
        values: Default::default(),
    };
    assert!(scoop_linux::maintenance::refresh_due(&config));
    config.values.insert(
        "last_update".into(),
        (chrono::Utc::now() - chrono::Duration::hours(2))
            .to_rfc3339()
            .into(),
    );
    assert!(!scoop_linux::maintenance::refresh_due(&config));
    config.values.insert(
        "last_update".into(),
        (chrono::Utc::now() - chrono::Duration::hours(3))
            .to_rfc3339()
            .into(),
    );
    assert!(scoop_linux::maintenance::refresh_due(&config));
    for (old, new) in [
        ("0.1.0", "0.1.1"),
        ("0.5.0-alpha.2", "0.5.0-alpha.10"),
        ("0.5.0-alpha.10", "0.5.0-beta"),
        ("0.5.0-rc.1", "0.5.0"),
        ("1", "1.0"),
        ("1.1.1_8", "1.1.1_9"),
        ("7.0.4-9", "7.0.4-10"),
        ("1", "1+hotfix.0"),
        ("0.5alpha", "0.5"),
        ("0.5Beta", "0.5"),
    ] {
        assert_eq!(
            scoop_linux::version::compare(old, new),
            std::cmp::Ordering::Less,
            "{old} -> {new}"
        );
        assert_eq!(
            scoop_linux::version::compare(new, old),
            std::cmp::Ordering::Greater
        );
    }
}
