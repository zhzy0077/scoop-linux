use scoop_linux::download;
use serde_json::json;
use std::{fs, io::Write, os::unix::fs::symlink, process::Command};
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn cli(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", self.temp.path())
            .env("SCOOP", self.temp.path().join("root"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"));
        c
    }
    fn install(&self) {
        let archive = self.temp.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for name in ["bin/my tool", "bin/t~x"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        }
        zip.finish().unwrap();
        let manifest = self.temp.path().join("good.json");
        fs::write(&manifest,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&archive).unwrap().as_str(),"hash":download::digest(&archive).unwrap(),"bin":["bin/my tool","bin/t~x"]}).to_string()).unwrap();
        let out = self.cli().arg("install").arg(manifest).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
#[test]
fn p2_1_listing_warns_for_each_stray_or_corrupt_entry_and_keeps_healthy_apps() {
    let f = Fixture::new();
    f.install();
    let apps = f.temp.path().join("root/apps");
    fs::write(apps.join("stray-file"), "unrelated").unwrap();
    fs::create_dir(apps.join("My Stuff")).unwrap();
    fs::create_dir(apps.join("partial")).unwrap();
    fs::create_dir_all(apps.join("corrupt/1")).unwrap();
    fs::write(apps.join("corrupt/1/scoop-install.json"), "{broken}").unwrap();
    symlink("1", apps.join("corrupt/current")).unwrap();
    fs::create_dir(apps.join("dangling")).unwrap();
    symlink("missing", apps.join("dangling/current")).unwrap();
    let out = f.cli().arg("list").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("good\t1\t"));
    let warnings = String::from_utf8(out.stderr).unwrap();
    for name in ["stray-file", "My Stuff", "partial", "corrupt", "dangling"] {
        assert!(warnings.contains(name), "{warnings}");
        assert!(apps.join(name).exists());
    }
    assert_eq!(
        fs::read_to_string(apps.join("stray-file")).unwrap(),
        "unrelated"
    );
    let updated = stdout
        .lines()
        .find(|line| line.starts_with("good\t"))
        .unwrap()
        .split('\t')
        .nth(3)
        .unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(updated).is_ok());
}
#[test]
fn p2_2_which_accepts_exact_linux_basenames_and_names_unmanaged_files() {
    let f = Fixture::new();
    f.install();
    for name in ["my tool", "t~x"] {
        let out = f.cli().args(["which", name]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!(
                "{}\n",
                f.temp
                    .path()
                    .join("root/apps/good/1/bin")
                    .join(name)
                    .display()
            )
        );
    }
    let unmanaged = f.temp.path().join("root/bin/unmanaged");
    fs::write(&unmanaged, "keep").unwrap();
    let out = f.cli().args(["which", "unmanaged"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a managed executable"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Invalid argument"));
    assert_eq!(fs::read_to_string(unmanaged).unwrap(), "keep");
    for name in ["../tool", "bin/tool", ".", ".."] {
        assert_eq!(
            f.cli()
                .args(["which", name])
                .output()
                .unwrap()
                .status
                .code(),
            Some(2)
        );
    }
}
#[test]
fn p2_5_checksum_diagnostic_identifies_source_filename() {
    let f = Fixture::new();
    let source = f.temp.path().join("payload.sh");
    fs::write(&source, b"#!/bin/sh\nexit 0\n").unwrap();
    let manifest = f.temp.path().join("bad.json");
    let url = reqwest::Url::from_file_path(source).unwrap();
    fs::write(&manifest,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":url.as_str(),"hash":"a".repeat(64),"bin":"payload.sh"}).to_string()).unwrap();
    let out = f
        .cli()
        .arg("install")
        .arg(manifest)
        .arg("-k")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(
        error.contains("checksum mismatch for payload.sh (file://"),
        "{error}"
    );
    assert!(!error.contains("checksum mismatch for /tmp/.tmp"));
}
