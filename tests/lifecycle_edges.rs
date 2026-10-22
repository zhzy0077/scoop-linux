use scoop_linux::{download, platform};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::{fs::symlink, process::ExitStatusExt},
    process::{Command, Stdio},
};
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", self.temp.path())
            .env("SCOOP", self.temp.path().join("root"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .output()
            .unwrap()
    }
    fn manifest(&self, name: &str, version: &str, content: &[u8], bin: &str) -> String {
        let artifact = self.temp.path().join(bin);
        fs::write(&artifact, content).unwrap();
        let path = self.temp.path().join(format!("{name}.json"));
        fs::write(&path,json!({"version":version,"homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap(),"bin":bin}).to_string()).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }
}
#[test]
fn explicit_versions_retain_old_content_and_uninstall_all_versions() {
    let f = Fixture::new();
    let path = f.manifest("tool", "1", b"#!/bin/sh\nprintf first", "Exact.exe");
    f.ok(&["install", &path]);
    let first =
        fs::read_to_string(f.temp.path().join("root/apps/tool/1/scoop-manifest.json")).unwrap();
    let path = f.manifest("tool", "2", b"#!/bin/sh\nprintf second", "Exact.exe");
    f.ok(&["install", &format!("{path}@2")]);
    assert_eq!(
        Command::new(f.temp.path().join("root/bin/Exact.exe"))
            .output()
            .unwrap()
            .stdout,
        b"second"
    );
    assert_eq!(
        fs::read_to_string(f.temp.path().join("root/apps/tool/1/scoop-manifest.json")).unwrap(),
        first
    );
    assert!(!f.temp.path().join("root/bin/Exact").exists());
    f.ok(&["uninstall", "tool"]);
    assert!(!f.temp.path().join("root/apps/tool").exists());
}
#[test]
fn installed_package_needs_no_live_source_and_foreign_records_cannot_activate() {
    let f = Fixture::new();
    let path = f.manifest("tool", "1", b"#!/bin/sh\nexit 0", "tool");
    f.ok(&["install", &path]);
    fs::remove_file(path).unwrap();
    f.ok(&["install", "tool"]);
    let record = f
        .temp
        .path()
        .join("root/apps/tool/current/scoop-install.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    value["architecture"] = json!(if platform::host().unwrap() == "arm64" {
        "64bit"
    } else {
        "arm64"
    });
    fs::write(&record, value.to_string()).unwrap();
    assert!(
        String::from_utf8_lossy(&f.run(&["install", "tool"]).stderr)
            .contains("foreign architecture")
    );
}
#[test]
fn inherited_io_environment_and_signal_semantics() {
    let f = Fixture::new();
    let path = f.manifest(
        "stream",
        "1",
        b"#!/bin/sh\ncat\nprintf '%s' \"$CUSTOM_ENV\" >&2\nkill -TERM $$\n",
        "stream",
    );
    f.ok(&["install", &path]);
    let mut child = Command::new(f.temp.path().join("root/bin/stream"))
        .env("CUSTOM_ENV", "inherited")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"input bytes\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.stdout, b"input bytes\n");
    assert_eq!(output.stderr, b"inherited");
    assert_eq!(output.status.signal(), Some(15));
}
#[test]
fn reject_unmanaged_destinations_and_reserved_download_metadata() {
    let f = Fixture::new();
    let path = f.manifest("tool", "1", b"#!/bin/sh\nexit 0", "tool");
    let bin = f.temp.path().join("root/bin");
    fs::create_dir_all(&bin).unwrap();
    let external = f.temp.path().join("unrelated");
    fs::write(&external, "keep").unwrap();
    symlink(&external, bin.join("tool")).unwrap();
    assert!(!f.run(&["install", &path]).status.success());
    assert_eq!(fs::read_to_string(&external).unwrap(), "keep");
    fs::remove_file(bin.join("tool")).unwrap();
    let path = f.manifest("metadata", "1", b"{}", "scoop-install.json");
    assert!(!f.run(&["install", &path]).status.success());
    assert!(!f.temp.path().join("root/apps/metadata/current").exists());
    for version in [".", "..", "current"] {
        let path = f.manifest("invalid", version, b"#!/bin/sh\nexit 0", "tool");
        assert!(!f.run(&["install", &path]).status.success());
    }
}
