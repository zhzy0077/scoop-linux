use scoop_linux::{download, platform};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::{Command, Output},
};

#[path = "support/scoopfile_review.rs"]
mod review;
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn cli(&self, home: &str) -> Command {
        let base = self.temp.path().join(home);
        fs::create_dir_all(&base).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", &base)
            .env("SCOOP", base.join("root"))
            .env("XDG_CONFIG_HOME", base.join("config"));
        c
    }
    fn ok(&self, home: &str, args: &[&str]) -> String {
        let o = self.cli(home).args(args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }
    fn document(&self, value: Value) -> String {
        let p = self.temp.path().join("scoopfile.json");
        fs::write(&p, value.to_string()).unwrap();
        p.to_str().unwrap().into()
    }
    fn manifest(&self) -> String {
        let artifact = self.temp.path().join("artifact");
        fs::write(&artifact, "#!/bin/sh\nprintf roundtrip").unwrap();
        let p = self.temp.path().join("hello.json");
        fs::write(&p,json!({"version":"1","homepage":"https://example.org/path?q=hello&x=1","license":"MIT","url":format!("{}#/hello",reqwest::Url::from_file_path(&artifact).unwrap()),"hash":download::digest(&artifact).unwrap(),"bin":"hello"}).to_string()).unwrap();
        p.to_str().unwrap().into()
    }
    fn failed(o: Output, expected: &str) {
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}
#[test]
fn file_roundtrip_config_hold_and_native_architecture() {
    let f = Fixture::new();
    let manifest = f.manifest();
    f.ok("a", &["install", &manifest, "-u"]);
    f.ok("a", &["hold", "hello"]);
    f.ok("a", &["config", "force_update", "true"]);
    f.ok("a", &["config", "root_path", "/machine/specific"]);
    let exported: Value = serde_json::from_str(&f.ok("a", &["export", "-c"])).unwrap();
    assert_eq!(exported["apps"][0]["Name"], "hello");
    assert!(
        exported["apps"][0]["Info"]
            .as_str()
            .unwrap()
            .contains("Held package")
    );
    assert_eq!(exported["config"]["force_update"], true);
    assert!(exported["config"].get("root_path").is_none());
    let path = f.document(exported);
    f.ok("b", &["import", &path]);
    assert_eq!(
        Command::new(f.temp.path().join("b/root/bin/hello"))
            .output()
            .unwrap()
            .stdout,
        b"roundtrip"
    );
    let second: Value = serde_json::from_str(&f.ok("b", &["export"])).unwrap();
    assert_eq!(second["apps"][0]["Source"], manifest);
    assert!(
        second["apps"][0]["Info"]
            .as_str()
            .unwrap()
            .contains("Held package")
    );
    f.ok("b", &["import", &path]);
}
#[test]
fn bucket_import_uses_current_version_and_reports_each_unsupported_entry() {
    let f = Fixture::new();
    let manifest = f.manifest();
    let repo = f.temp.path().join("repo");
    fs::create_dir_all(repo.join("bucket")).unwrap();
    fs::copy(&manifest, repo.join("bucket/hello.json")).unwrap();
    for args in [
        vec!["init"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.org",
            "commit",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let foreign = if platform::host().unwrap() == "arm64" {
        "64bit"
    } else {
        "arm64"
    };
    let path=f.document(json!({"buckets":[{"Name":"main","Source":repo}],"apps":[{"Name":"bad","Source":"main","Info":"Global install"},{"Name":"foreign","Source":"main","Info":foreign},{"Name":"hello","Version":"obsolete","Source":"main","Info":"Held package"}]}));
    let output = f.cli("a").args(["import", &path]).output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("global Scoopfile"));
    assert!(error.contains("foreign architecture"));
    assert!(error.contains("app entry 0") && error.contains("app entry 1"));
    assert_eq!(
        fs::read_link(f.temp.path().join("a/root/apps/hello/current"))
            .unwrap()
            .to_str(),
        Some("1")
    );
    let exported: Value = serde_json::from_str(&f.ok("a", &["export"])).unwrap();
    let path = f.document(exported);
    f.ok("b", &["import", &path]);
    assert_eq!(
        f.ok("b", &["prefix", "hello"]).trim(),
        f.temp
            .path()
            .join("b/root/apps/hello/current")
            .to_str()
            .unwrap()
    );
}
#[test]
fn diagnostics_detect_broken_installations_and_home_uses_argument_array() {
    let f = Fixture::new();
    let manifest = f.manifest();
    f.ok("a", &["install", &manifest, "-u"]);
    let bin = f.temp.path().join("a/root/bin");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = f
        .cli("a")
        .env("PATH", &path)
        .arg("checkup")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::remove_file(bin.join("hello")).unwrap();
    symlink("../apps/hello/current/missing", bin.join("hello")).unwrap();
    Fixture::failed(
        f.cli("a")
            .env("PATH", &path)
            .arg("checkup")
            .output()
            .unwrap(),
        "checkup found",
    );
    f.ok("a", &["reset", "hello"]);
    let tools = f.temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let opener = tools.join("xdg-open");
    let captured = f.temp.path().join("opened");
    fs::write(
        &opener,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$#\" \"$1\" > '{}'\n",
            captured.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        f.cli("a")
            .env("PATH", &tools)
            .args(["home", &manifest])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        fs::read_to_string(&captured).unwrap(),
        "1\nhttps://example.org/path?q=hello&x=1\n"
    );
    fs::write(&opener, "#!/bin/sh\nexit 7\n").unwrap();
    Fixture::failed(
        f.cli("a")
            .env("PATH", &tools)
            .args(["home", &manifest])
            .output()
            .unwrap(),
        "xdg-open failed",
    );
}
