use scoop_linux::download;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(f.temp.path().join("config/scoop")).unwrap();
        fs::write(
            f.temp.path().join("config/scoop/config.json"),
            r#"{"last_update":"2099-01-01T00:00:00Z"}"#,
        )
        .unwrap();
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
        let output = self.cli().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn publish(&self, app: &str, version: &str, body: &str, bin: &str) -> String {
        let artifact = self.temp.path().join(format!("{app}-{version}-{body}"));
        fs::write(&artifact, format!("#!/bin/sh\nprintf '{body}'")).unwrap();
        let mut url = reqwest::Url::from_file_path(&artifact).unwrap();
        url.set_fragment(Some(&format!("/{bin}")));
        let path = self.temp.path().join(format!("{app}.json"));
        fs::write(&path,json!({"version":version,"homepage":"https://example.org","license":"MIT","url":url.as_str(),"hash":download::digest(&artifact).unwrap(),"bin":bin}).to_string()).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn current(&self, app: &str) -> String {
        fs::read_link(self.temp.path().join(format!("root/apps/{app}/current")))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
    fn execute(&self, bin: &str) -> Vec<u8> {
        let output = Command::new(self.temp.path().join("root/bin").join(bin))
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }
}
#[test]
fn held_force_updates_retained_reset_collisions_cleanup_and_cache() {
    let f = Fixture::new();
    let path = f.publish("app", "1", "first", "tool");
    f.ok(&["install", &path]);
    f.publish("app", "2", "second", "tool");
    assert!(f.ok(&["status", "-l"]).contains("app\t1\t2"));
    f.ok(&["hold", "app"]);
    assert!(f.ok(&["update", "app", "-f"]).contains("held"));
    assert_eq!(f.current("app"), "1");
    f.ok(&["update", "--all", "-f"]);
    assert_eq!(f.current("app"), "1");
    f.ok(&["unhold", "app"]);
    f.ok(&["update", "app"]);
    assert_eq!(f.current("app"), "2");
    assert_eq!(f.execute("tool"), b"second");
    f.publish("app", "2", "forced", "tool");
    f.ok(&["update", "app", "-f", "-q"]);
    assert_eq!(f.execute("tool"), b"forced");
    assert!(f.temp.path().join("root/apps/app/1").is_dir());
    assert_eq!(
        fs::read_dir(f.temp.path().join("root/.transactions"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_dir(f.temp.path().join("root/.staging"))
            .unwrap()
            .count(),
        0
    );
    f.ok(&["reset", "app@1"]);
    assert_eq!(f.execute("tool"), b"first");
    let b = f.publish("other", "1", "other", "tool");
    f.ok(&["install", &b]);
    assert_eq!(f.execute("tool"), b"other");
    f.ok(&["reset", "app"]);
    assert_eq!(f.execute("tool"), b"first");
    f.ok(&["reset", "app@2"]);
    f.ok(&["cleanup", "app", "-k"]);
    assert!(!f.temp.path().join("root/apps/app/1").exists());
    assert!(f.temp.path().join("root/apps/app/2").is_dir());
    assert!(!f.ok(&["cache", "show", "app"]).contains("app\t1\t"));
    let note = f.temp.path().join("root/cache/unrelated.txt");
    fs::write(&note, "keep").unwrap();
    assert!(f.ok(&["cache"]).contains("Total:"));
    f.ok(&["cache", "rm", "app"]);
    assert!(!f.ok(&["cache", "show"]).contains("app\t2\t"));
    f.ok(&["cache", "rm", "--all"]);
    assert_eq!(fs::read_to_string(note).unwrap(), "keep");
    f.ok(&["cleanup", "--all"]);
    f.ok(&["reset", "--all"]);
}
#[test]
fn missing_commands_removed_bins_foreign_records_and_downgrades() {
    let f = Fixture::new();
    let path = f.publish("app", "1", "one", "old");
    f.ok(&["install", &path]);
    let old_command = f.temp.path().join("root/bin/old");
    fs::remove_file(&old_command).unwrap();
    fs::write(&old_command, "user replacement").unwrap();
    f.publish("app", "2", "two", "new");
    f.ok(&["update", "app"]);
    assert_eq!(
        fs::read_to_string(&old_command).unwrap(),
        "user replacement"
    );
    fs::remove_file(old_command).unwrap();
    assert!(f.temp.path().join("root/bin/new").exists());
    f.ok(&["reset", "app@1"]);
    assert!(!f.temp.path().join("root/bin/new").exists());
    f.ok(&["reset", "app@2"]);
    // Unrelated files also survive downgrade and failed native activation.
    fs::write(f.temp.path().join("root/bin/user-file"), "keep").unwrap();
    f.publish("app", "1", "one", "old");
    f.ok(&["update", "app"]);
    assert_eq!(f.current("app"), "2");
    f.ok(&["config", "force_update", "true"]);
    f.ok(&["update", "app"]);
    assert_eq!(f.current("app"), "1");
    let record = f.temp.path().join("root/apps/app/2/scoop-install.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    value["architecture"] = json!(if scoop_linux::platform::host().unwrap() == "arm64" {
        "64bit"
    } else {
        "arm64"
    });
    fs::write(&record, value.to_string()).unwrap();
    let output = f.cli().args(["reset", "app@2"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(f.current("app"), "1");
    assert_eq!(
        fs::read_to_string(f.temp.path().join("root/bin/user-file")).unwrap(),
        "keep"
    );
    assert_eq!(
        f.cli().args(["which", "missing"]).status().unwrap().code(),
        Some(2)
    );
    assert_eq!(
        fs::read_link(f.temp.path().join("root/apps/app/current")).unwrap(),
        Path::new("1")
    );
}
#[test]
fn update_options_dependencies_no_cache_hash_and_partial_errors() {
    let f = Fixture::new();
    let dep = f.publish("dep", "1", "dependency", "dep");
    let app = f.publish("app", "1", "first", "app");
    f.ok(&["install", &app]);
    f.publish("app", "2", "second", "app");
    let mut value: Value = serde_json::from_slice(&fs::read(&app).unwrap()).unwrap();
    value["depends"] = json!("missing");
    fs::write(&app, value.to_string()).unwrap();
    assert!(
        !f.cli()
            .args(["update", "app"])
            .output()
            .unwrap()
            .status
            .success()
    );
    f.ok(&["update", "app", "-i", "-k"]);
    assert_eq!(f.current("app"), "2");
    assert!(!f.ok(&["cache", "show", "app"]).contains("app\t2\t"));
    let mut value: Value = serde_json::from_slice(&fs::read(&app).unwrap()).unwrap();
    value["version"] = json!("3");
    value["hash"] = json!("b".repeat(64));
    value.as_object_mut().unwrap().remove("depends");
    fs::write(&app, value.to_string()).unwrap();
    assert!(
        !f.cli()
            .args(["update", "app"])
            .output()
            .unwrap()
            .status
            .success()
    );
    f.ok(&["update", "app", "-s"]);
    assert_eq!(f.current("app"), "3");
    let repo = f.temp.path().join("root/buckets/local");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "fixture@example.org"][..],
        &["config", "user.name", "Fixture"][..],
    ] {
        scoop_linux::buckets::git(&repo, args).unwrap();
    }
    fs::copy(&dep, repo.join("dep.json")).unwrap();
    scoop_linux::buckets::git(&repo, &["add", "."]).unwrap();
    scoop_linux::buckets::git(&repo, &["commit", "-qm", "dependency"]).unwrap();
    f.publish("app", "4", "fourth", "app");
    let mut value: Value = serde_json::from_slice(&fs::read(&app).unwrap()).unwrap();
    value["depends"] = json!("local/dep");
    fs::write(&app, value.to_string()).unwrap();
    f.ok(&["update", "app"]);
    assert_eq!(f.current("app"), "4");
    assert_eq!(f.current("dep"), "1");
    assert_eq!(f.execute("dep"), b"dependency");
    let out = f
        .cli()
        .args(["update", "not-installed", "dep"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("dep"));
}
#[test]
fn cache_symlink_replacement_is_preserved() {
    let f = Fixture::new();
    let path = f.publish("app", "1", "one", "tool");
    f.ok(&["install", &path]);
    let cache = f.temp.path().join("root/cache");
    let external = f.temp.path().join("external-cache");
    fs::rename(&cache, &external).unwrap();
    std::os::unix::fs::symlink(&external, &cache).unwrap();
    let before: Vec<_> = fs::read_dir(&external)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        !f.cli()
            .args(["cache", "rm", "--all"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !f.cli()
            .args(["download", &path, "--force"])
            .output()
            .unwrap()
            .status
            .success()
    );
    for path in before {
        assert!(path.is_file());
    }
    assert!(
        fs::symlink_metadata(&cache)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    f.publish("app", "2", "two", "tool");
    f.ok(&["update", "app", "-k"]);
    assert_eq!(f.execute("tool"), b"two");
}
#[test]
fn scoop_hold_has_upstream_date_name() {
    let f = Fixture::new();
    f.ok(&["hold", "scoop"]);
    let raw = f.ok(&["config", "hold_update_until"]);
    assert!(scoop_linux::maintenance::date(raw.trim()).unwrap() > chrono::Utc::now());
    f.ok(&["unhold", "scoop"]);
    assert!(f.ok(&["config", "hold_update_until"]).contains("not set"));
}
