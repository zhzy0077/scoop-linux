#![cfg(feature = "test-hooks")]
use scoop_linux::download;
use serde_json::json;
use std::{fs, process::Command};
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_scoop"));
        command
            .env("HOME", self.temp.path())
            .env("SCOOP", self.temp.path().join("root"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"));
        command
    }
    fn publish(&self, version: &str, body: &str, bin: &str) -> String {
        let artifact = self.temp.path().join(format!("source-{version}-{body}"));
        fs::write(&artifact, format!("#!/bin/sh\nprintf '{body}'")).unwrap();
        let mut url = reqwest::Url::from_file_path(&artifact).unwrap();
        url.set_fragment(Some(&format!("/{bin}")));
        let path = self.temp.path().join("app.json");
        fs::write(&path,json!({"version":version,"homepage":"https://example.org","license":"MIT","url":url.as_str(),"hash":download::digest(&artifact).unwrap(),"bin":bin}).to_string()).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn ok(&self, args: &[&str]) {
        let out = self.cli().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn current(&self) -> String {
        fs::read_link(self.temp.path().join("root/apps/app/current"))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
    fn output(&self, bin: &str) -> Vec<u8> {
        let out = Command::new(self.temp.path().join("root/bin").join(bin))
            .output()
            .unwrap();
        assert!(out.status.success());
        out.stdout
    }
    fn empty_journal(&self) {
        assert_eq!(
            fs::read_dir(self.temp.path().join("root/.transactions"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(self.temp.path().join("root/.staging"))
                .unwrap()
                .count(),
            0
        );
    }
}
#[test]
fn crashes_at_activation_boundaries_recover_on_next_command() {
    for point in ["prepared", "published", "current", "links", "committed"] {
        let f = Fixture::new();
        let path = f.publish("1", "old", "old");
        f.ok(&["install", &path]);
        f.publish("2", "new", "new");
        let out = f
            .cli()
            .args(["update", "app"])
            .env("SCOOP_TEST_FAILPOINT", point)
            .env("SCOOP_TEST_CRASH", "1")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(86),
            "{point}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        f.ok(if point == "prepared" {
            &["--version"]
        } else if point == "current" {
            &["help"]
        } else {
            &["list"]
        });
        let committed = point == "committed";
        assert_eq!(f.current(), if committed { "2" } else { "1" });
        assert_eq!(
            f.output(if committed { "new" } else { "old" }),
            if committed {
                b"new".to_vec()
            } else {
                b"old".to_vec()
            }
        );
        assert!(
            !f.temp
                .path()
                .join(if committed {
                    "root/bin/old"
                } else {
                    "root/bin/new"
                })
                .exists()
        );
        f.empty_journal();
    }
}
#[test]
fn forced_active_version_backup_survives_crash_until_commit() {
    for point in ["prepared", "backup", "published", "current", "committed"] {
        let f = Fixture::new();
        let path = f.publish("1", "old", "tool");
        f.ok(&["install", &path]);
        f.publish("1", "replacement", "tool");
        let out = f
            .cli()
            .args(["update", "app", "-f"])
            .env("SCOOP_TEST_FAILPOINT", point)
            .env("SCOOP_TEST_CRASH", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(86), "{point}");
        f.ok(&["prefix", "app"]);
        assert_eq!(
            f.output("tool"),
            if point == "committed" {
                b"replacement".to_vec()
            } else {
                b"old".to_vec()
            }
        );
        f.empty_journal();
    }
}
#[test]
fn ordinary_errors_rollback_immediately_and_retained_reset_recovers() {
    let f = Fixture::new();
    let path = f.publish("1", "one", "one");
    f.ok(&["install", &path]);
    f.publish("2", "two", "two");
    let out = f
        .cli()
        .args(["update", "app"])
        .env("SCOOP_TEST_FAILPOINT", "links")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(f.current(), "1");
    assert_eq!(f.output("one"), b"one");
    f.empty_journal();
    f.ok(&["update", "app"]);
    let out = f
        .cli()
        .args(["reset", "app@1"])
        .env("SCOOP_TEST_FAILPOINT", "current")
        .env("SCOOP_TEST_CRASH", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(86));
    f.ok(&["list"]);
    assert_eq!(f.current(), "2");
    assert_eq!(f.output("two"), b"two");
    f.empty_journal();
}
#[test]
fn concurrent_installs_and_updates_serialize_without_lost_state() {
    let f = Fixture::new();
    let path = f.publish("1", "one", "tool");
    let mut a = f
        .cli()
        .args(["install", &path])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut b = f
        .cli()
        .args(["install", &path])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    f.publish("2", "two", "tool");
    let mut a = f
        .cli()
        .args(["update", "app"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut b = f
        .cli()
        .args(["update", "app"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    assert_eq!(f.current(), "2");
    assert_eq!(f.output("tool"), b"two");
    assert!(
        f.temp
            .path()
            .join("root/apps/app/1/scoop-manifest.json")
            .is_file()
    );
    f.empty_journal();
}
#[test]
fn recovery_preserves_user_replacements_and_external_xdg_data() {
    let f = Fixture::new();
    let path = f.publish("1", "old", "tool");
    f.ok(&["install", &path]);
    f.publish("2", "new", "new");
    let out = f
        .cli()
        .args(["update", "app"])
        .env("SCOOP_TEST_FAILPOINT", "links")
        .env("SCOOP_TEST_CRASH", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(86));
    let external = f.temp.path().join("external-xdg");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("data"), "keep").unwrap();
    let bin = f.temp.path().join("root/bin/new");
    fs::remove_file(&bin).unwrap();
    fs::write(&bin, "unrelated replacement").unwrap();
    let out = f.cli().arg("list").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Mutations of app are blocked"));
    let blocked = f.cli().args(["uninstall", "app"]).output().unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("app.json"));
    let other = f.temp.path().join("other.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["bin"] = serde_json::json!("other");
    value["url"] = serde_json::json!(format!(
        "{}#/other",
        value["url"].as_str().unwrap().split('#').next().unwrap()
    ));
    fs::write(&other, value.to_string()).unwrap();
    f.ok(&["install", other.to_str().unwrap(), "-u"]);
    f.ok(&["uninstall", "other"]);
    f.ok(&["config", "show_update_log", "true"]);
    f.ok(&["--version"]);
    assert_eq!(fs::read_to_string(&bin).unwrap(), "unrelated replacement");
    assert_eq!(fs::read_to_string(external.join("data")).unwrap(), "keep");
    fs::remove_file(bin).unwrap();
    f.ok(&["list"]);
    f.empty_journal();
}
