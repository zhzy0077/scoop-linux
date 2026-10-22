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
        git(&source, &["init", "-q"]).unwrap();
        git(&source, &["config", "user.name", "Fixture"]).unwrap();
        git(&source, &["config", "user.email", "fixture@example.org"]).unwrap();
        f.publish("tool", "1");
        f.ok(&["bucket", "add", "main", source.to_str().unwrap()]);
        f.ok(&["install", "tool", "-u"]);
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
        let o = self.cli().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }
    fn publish(&self, name: &str, version: &str) {
        let payload = self.temp.path().join(format!("{name}-{version}"));
        fs::write(&payload, "#!/bin/sh\nexit 0\n").unwrap();
        let source = self.temp.path().join("source");
        fs::write(source.join(format!("bucket/{name}.json")),json!({"version":version,"homepage":"https://example.org","license":"MIT","url":format!("{}#/{name}",reqwest::Url::from_file_path(&payload).unwrap()),"hash":download::digest(&payload).unwrap(),"bin":name}).to_string()).unwrap();
        git(&source, &["add", "."]).unwrap();
        git(&source, &["commit", "-qm", version]).unwrap();
    }
    fn stale(&self) {
        self.ok(&["config", "last_update", "2000-01-01T00:00:00Z"]);
    }
    fn current(&self) -> String {
        fs::read_link(self.temp.path().join("root/apps/tool/current"))
            .unwrap()
            .to_str()
            .unwrap()
            .into()
    }
}
#[test]
fn failed_refresh_continues_package_update_and_backs_off() {
    let f = Fixture::new();
    f.publish("tool", "2");
    fs::copy(
        f.temp.path().join("source/bucket/tool.json"),
        f.temp.path().join("root/buckets/main/bucket/tool.json"),
    )
    .unwrap();
    f.stale();
    let o = f.cli().args(["update", "tool"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("local changes"));
    assert_eq!(f.current(), "2");
    assert_ne!(f.ok(&["config", "last_update"]), "2000-01-01T00:00:00Z\n");
    let o = f.cli().args(["download", "tool"]).output().unwrap();
    assert!(o.status.success());
    assert!(!String::from_utf8_lossy(&o.stderr).contains("local changes"));
}
#[test]
fn incompatible_catalog_does_not_block_bucket_pull_and_same_version_needs_no_target() {
    let f = Fixture::new();
    f.publish("tool", "2");
    let path = f.temp.path().join("catalog.json");
    fs::write(
        &path,
        json!({"format_version":1,"version":"999.0.0","targets":{}}).to_string(),
    )
    .unwrap();
    f.ok(&["config", "release_catalog", path.to_str().unwrap()]);
    f.stale();
    let o = f.cli().arg("update").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("Updated bucket main"));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no compatible target"));
    assert_ne!(f.ok(&["config", "last_update"]), "2000-01-01T00:00:00Z\n");
    f.ok(&["update", "tool"]);
    assert_eq!(f.current(), "2");
    fs::write(
        &path,
        json!({"format_version":1,"version":env!("CARGO_PKG_VERSION"),"targets":{}}).to_string(),
    )
    .unwrap();
    f.ok(&["update"]);
}
#[test]
fn status_fetches_without_pull_config_write_or_activation_and_survives_offline() {
    let f = Fixture::new();
    f.publish("tool", "2");
    f.stale();
    let config = f.temp.path().join("config/scoop/config.json");
    let before = fs::read(&config).unwrap();
    let repo = f.temp.path().join("root/buckets/main");
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let o = f.cli().arg("status").output().unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("remote changes"));
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head);
    assert_eq!(fs::read(&config).unwrap(), before);
    assert_eq!(f.current(), "1");
    assert_ne!(git(&repo, &["rev-parse", "FETCH_HEAD"]).unwrap(), head);
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "/nonexistent-scoop-fixture-remote",
        ],
    )
    .unwrap();
    let o = f.cli().arg("status").output().unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("using local manifests"));
    assert!(String::from_utf8_lossy(&o.stdout).contains("Installed Version"));
    assert_eq!(fs::read(&config).unwrap(), before);
}
#[test]
fn refresh_resolves_packages_added_after_clone_for_install_and_download() {
    for command in ["install", "download"] {
        let f = Fixture::new();
        f.publish("newpkg", "1");
        f.stale();
        let output = f.ok(&[command, "newpkg"]);
        assert!(output.contains(if command == "install" {
            "Installed newpkg"
        } else {
            "newpkg#"
        }));
    }
}
#[test]
fn cache_names_are_literal_including_pluses_and_dots() {
    let f = Fixture::new();
    for name in ["g++", "a.b", "axb"] {
        f.publish(name, "1");
    }
    f.ok(&["update"]);
    for name in ["g++", "a.b", "axb"] {
        f.ok(&["download", name, "-u"]);
    }
    assert!(f.ok(&["cache", "rm", "g++"]).contains("Deleted: 1 files"));
    assert!(f.ok(&["cache", "show", "g++"]).contains("Total: 0 files"));
    assert!(f.ok(&["cache", "rm", "a.b"]).contains("Deleted: 1 files"));
    assert!(f.ok(&["cache", "show", "axb"]).contains("axb\t1\t"));
    let config: Value =
        serde_json::from_slice(&fs::read(f.temp.path().join("config/scoop/config.json")).unwrap())
            .unwrap();
    assert!(config.is_object());
}
