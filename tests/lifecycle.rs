use scoop_linux::{buckets::git, config::Config, download, lifecycle, resolve::Resolver};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process::{Command, Output},
};
struct Fixture {
    temp: tempfile::TempDir,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        Self {
            config: Config {
                path: temp.path().join("config/config.json"),
                cache: root.join("cache"),
                root,
                values: Default::default(),
            },
            temp,
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", self.temp.path())
            .env("SCOOP", &self.config.root)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_DATA_HOME", self.temp.path().join("data"))
            .output()
            .unwrap()
    }
    fn source(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.temp.path().join(name);
        fs::write(&path, bytes).unwrap();
        reqwest::Url::from_file_path(path).unwrap().to_string()
    }
    fn manifest(&self, name: &str, version: &str, url: &str, bin: &str) -> String {
        let path = self.temp.path().join(format!("{name}.json"));
        let source = reqwest::Url::parse(url).unwrap().to_file_path().unwrap();
        let mut value = json!({"version":version,"homepage":"https://example.org","license":"MIT","url":url,"hash":download::digest(&source).unwrap(),"bin":bin});
        value["notes"] = json!("$dir :: $original_dir");
        fs::write(&path, value.to_string()).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn edit(&self, path: &str, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, value.to_string()).unwrap();
    }
    fn success(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
const WRAPPER:&[u8]=b"#!/bin/sh\nmkdir -p \"$XDG_DATA_HOME/fixture\"\nprintf keep > \"$XDG_DATA_HOME/fixture/data\"\nprintf '%s\\n' \"$PWD\" \"$@\"\nexit 7\n";
#[test]
fn raw_wrapper_real_execution_shells_records_and_uninstall() {
    let f = Fixture::new();
    let url = f.source("tool.sh", WRAPPER);
    let manifest = f.manifest("wrapper", "1", &url, "tool.sh");
    let raw = fs::read_to_string(&manifest).unwrap();
    let output = f.success(&["install", &manifest, "-u"]);
    assert!(output.contains("Installed wrapper"));
    assert!(output.contains("/current :: "));
    let current = f.config.root.join("apps/wrapper/current");
    assert_eq!(fs::read_link(&current).unwrap(), Path::new("1"));
    assert_eq!(
        fs::read_to_string(current.join("scoop-manifest.json")).unwrap(),
        raw
    );
    let record: Value =
        serde_json::from_slice(&fs::read(current.join("scoop-install.json")).unwrap()).unwrap();
    assert_eq!(record["format_version"], 1);
    assert_eq!(record["url"], manifest);
    assert_eq!(
        record["architecture"],
        scoop_linux::platform::host().unwrap()
    );
    assert_eq!(record["hold"], false);
    let exe = f.config.root.join("bin/tool.sh");
    assert_eq!(
        fs::read_link(&exe).unwrap(),
        Path::new("../apps/wrapper/current/tool.sh")
    );
    assert!(fs::metadata(&exe).unwrap().permissions().mode() & 0o111 != 0);
    for shell in [None, Some("bash"), Some("zsh"), Some("fish")] {
        let mut command = if let Some(shell) = shell {
            let mut c = Command::new(shell);
            c.args(["-c", "\"$FIXTURE_EXE\" 'hello world' 'two' "]);
            c
        } else {
            let mut c = Command::new(&exe);
            c.args(["hello world", "two"]);
            c
        };
        let output = command
            .env("FIXTURE_EXE", &exe)
            .env("XDG_DATA_HOME", f.temp.path().join("external-data"))
            .current_dir(f.temp.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\nhello world\ntwo\n", f.temp.path().display())
        );
    }
    assert_eq!(
        f.success(&["prefix", "wrapper"]),
        format!("{}\n", current.display())
    );
    assert_eq!(
        f.success(&["which", "tool.sh"]),
        format!(
            "{}\n",
            f.config.root.join("apps/wrapper/1/tool.sh").display()
        )
    );
    assert!(f.success(&["list"]).contains("wrapper\t1\t"));
    fs::write(&manifest, "invalid changed source").unwrap();
    assert_eq!(
        fs::read_to_string(current.join("scoop-manifest.json")).unwrap(),
        raw
    );
    f.success(&["uninstall", "wrapper"]);
    assert!(!exe.exists());
    assert!(!f.config.root.join("apps/wrapper").exists());
    assert_eq!(
        fs::read_to_string(f.temp.path().join("external-data/fixture/data")).unwrap(),
        "keep"
    );
}
#[test]
fn cache_revalidation_no_cache_skip_and_partial_failures() {
    let f = Fixture::new();
    let url = f.source("tool", b"#!/bin/sh\nexit 0\n");
    let manifest = f.manifest("tool", "1", &url, "tool");
    f.success(&["download", &manifest]);
    let cache: Vec<_> = fs::read_dir(&f.config.cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(cache.len(), 1);
    fs::write(&cache[0], "corrupt").unwrap();
    f.success(&["download", &manifest]);
    assert_eq!(fs::read(&cache[0]).unwrap(), b"#!/bin/sh\nexit 0\n");
    let source = reqwest::Url::parse(&url).unwrap().to_file_path().unwrap();
    fs::remove_file(source).unwrap();
    f.success(&["download", &manifest]);
    assert!(!f.run(&["download", &manifest, "--force"]).status.success());
    assert!(
        !f.run(&["install", &manifest, "--no-cache"])
            .status
            .success()
    );
    assert!(!f.config.root.join("apps/tool/current").exists());
    f.success(&["install", &manifest]);
    let url = f.source("nocache", b"#!/bin/sh\nexit 0\n");
    let uncached = f.manifest("nocache", "1", &url, "nocache");
    f.success(&["install", &uncached, "-k"]);
    assert_eq!(fs::read_dir(&f.config.cache).unwrap().count(), 1);
    let bad = f.manifest("bad", "1", &url, "nocache");
    f.edit(&bad, |v| v["hash"] = json!("b".repeat(64)));
    assert!(!f.run(&["install", &bad]).status.success());
    assert!(!f.config.root.join("apps/bad/current").exists());
    assert_eq!(fs::read_dir(&f.config.cache).unwrap().count(), 1);
    f.success(&["install", &bad, "-s"]);
    let second = f.manifest("second", "1", &url, "nocache");
    let out = f.run(&["install", "/missing.json", &second]);
    assert!(!out.status.success());
    assert!(f.config.root.join("apps/second/current").exists());
}
fn tar_bytes(entries: &[(&str, &[u8], u32)], links: &[(&str, &str, bool)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(*mode);
        header.set_cksum();
        builder.append_data(&mut header, *name, *data).unwrap();
    }
    for (name, target, hard) in links {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(if *hard {
            tar::EntryType::Link
        } else {
            tar::EntryType::Symlink
        });
        h.set_size(0);
        h.set_mode(0o777);
        h.set_cksum();
        builder.append_link(&mut h, *name, *target).unwrap();
    }
    builder.into_inner().unwrap()
}
fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn archive_formats_mappings_modes_links_and_native_binary() {
    let f = Fixture::new();
    let tar = tar_bytes(
        &[
            ("pkg/bin/run.sh", b"#!/bin/sh\nprintf archive", 0o751),
            ("pkg/data", b"data", 0o640),
        ],
        &[
            ("pkg/bin/link.sh", "run.sh", false),
            ("pkg/hard", "pkg/data", true),
        ],
    );
    let url = f.source("package.tar.gz", &gz(&tar));
    let path = f.manifest("archive", "1", &url, "lib/bin/link.sh");
    f.edit(&path, |v| {
        v["extract_dir"] = json!("pkg");
        v["extract_to"] = json!("lib");
    });
    f.success(&["install", &path]);
    assert_eq!(
        fs::read(f.config.root.join("apps/archive/current/lib/hard")).unwrap(),
        b"data"
    );
    assert_eq!(
        fs::metadata(f.config.root.join("apps/archive/current/lib/data"))
            .unwrap()
            .ino(),
        fs::metadata(f.config.root.join("apps/archive/current/lib/hard"))
            .unwrap()
            .ino()
    );
    let exe = f.config.root.join("bin/link.sh");
    assert_eq!(Command::new(&exe).output().unwrap().stdout, b"archive");
    assert_eq!(
        fs::metadata(f.config.root.join("apps/archive/current/lib/bin/run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751 | 0o111
    );
    let mut xz = xz2::write::XzEncoder::new(Vec::new(), 6);
    xz.write_all(&tar).unwrap();
    let url = f.source("package.tar.xz", &xz.finish().unwrap());
    let path = f.manifest("xz", "1", &url, "pkg/bin/run.sh");
    f.success(&["install", &path]);
    let zip_path = f.temp.path().join("package.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    zip.start_file(
        "bin/zip.sh",
        zip::write::SimpleFileOptions::default().unix_permissions(0o700),
    )
    .unwrap();
    zip.write_all(b"#!/bin/sh\nprintf zip").unwrap();
    zip.finish().unwrap();
    let zip_url = reqwest::Url::from_file_path(zip_path).unwrap().to_string();
    let raw = f.source("readme", b"raw");
    let path = f.manifest("mixed", "1", &zip_url, "tools/bin/zip.sh");
    f.edit(&path, |v| {
        let hash = v["hash"].clone();
        v["url"] = json!([raw, zip_url]);
        v["hash"] = json!([
            download::digest(&f.temp.path().join("readme")).unwrap(),
            hash
        ]);
        v["extract_to"] = json!("tools");
    });
    f.success(&["install", &path]);
    assert_eq!(
        Command::new(f.config.root.join("bin/zip.sh"))
            .output()
            .unwrap()
            .stdout,
        b"zip"
    );
    let native = fs::read("/bin/echo").unwrap();
    let url = f.source("echo", &native);
    let path = f.manifest("native", "1", &url, "echo");
    assert!(!f.run(&["install", &path]).status.success());
    f.edit(&path, |v| {
        v["architecture"] = json!({scoop_linux::platform::host().unwrap():{}})
    });
    f.success(&["install", &path]);
    let native = f.config.root.join("bin/echo");
    assert_eq!(
        Command::new(&native)
            .arg("native works")
            .output()
            .unwrap()
            .stdout,
        b"native works\n"
    );
    for shell in ["bash", "zsh", "fish"] {
        let output = Command::new(shell)
            .args(["-c", "\"$NATIVE_EXE\" 'native works'"])
            .env("NATIVE_EXE", &native)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"native works\n");
    }
}
#[test]
fn containment_unmanaged_conflicts_and_failed_upgrade_preserve_current() {
    let f = Fixture::new();
    let url = f.source("tool", b"#!/bin/sh\nprintf old");
    let path = f.manifest("tool", "1", &url, "tool");
    f.success(&["install", &path]);
    let attacks = [
        tar_bytes(
            &[("data", b"data", 0o644)],
            &[("escape", "../../outside", false)],
        ),
        tar_bytes(&[], &[("escape", "/etc/passwd", false)]),
        tar_bytes(&[], &[("escape", "../../outside", true)]),
        tar_bytes(
            &[("sub/data", b"bad", 0o644)],
            &[("sub", "../outside", false)],
        ),
    ];
    for (i, bytes) in attacks.iter().enumerate() {
        let url = f.source(&format!("attack{i}.tar.gz"), &gz(bytes));
        let path = f.manifest(&format!("attack{i}"), "1", &url, "escape");
        assert!(!f.run(&["install", &path]).status.success());
        assert!(
            !f.config
                .root
                .join(format!("apps/attack{i}/current"))
                .exists()
        );
    }
    let conflict = f.manifest("conflict", "1", &url, "tool");
    fs::write(f.config.root.join("bin/unmanaged"), "keep").unwrap();
    f.edit(&conflict, |v| v["bin"] = json!("unmanaged"));
    assert!(!f.run(&["install", &conflict]).status.success());
    assert_eq!(
        fs::read_to_string(f.config.root.join("bin/unmanaged")).unwrap(),
        "keep"
    );
    f.edit(&path, |v| {
        v["version"] = json!("2");
        v["hash"] = json!("b".repeat(64));
    });
    let plan = lifecycle::plan(
        &Resolver::new(&f.config),
        &path,
        &scoop_linux::platform::host().unwrap(),
        false,
        false,
    )
    .unwrap();
    assert!(lifecycle::install_one(&f.config, &plan[0], download::Options::default()).is_err());
    assert_eq!(
        Command::new(f.config.root.join("bin/tool"))
            .output()
            .unwrap()
            .stdout,
        b"old"
    );
    assert_eq!(
        fs::read_link(f.config.root.join("apps/tool/current")).unwrap(),
        Path::new("1")
    );
}
#[test]
fn dependency_order_dedup_cycle_independent_and_collision_ownership() {
    let f = Fixture::new();
    let repo = f.config.root.join("buckets/local");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]).unwrap();
    git(&repo, &["config", "user.email", "fixture@example.org"]).unwrap();
    git(&repo, &["config", "user.name", "Fixture"]).unwrap();
    for (name, deps) in [
        ("base", vec![]),
        ("left", vec!["base"]),
        ("right", vec!["base"]),
        ("top", vec!["left", "right"]),
    ] {
        let url = f.source(
            &format!("{name}.sh"),
            format!("#!/bin/sh\nprintf {name}").as_bytes(),
        );
        let path = f.manifest(name, "1", &url, &format!("{name}.sh"));
        let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if !deps.is_empty() {
            value["depends"] = json!(deps);
        }
        fs::write(repo.join(format!("{name}.json")), value.to_string()).unwrap();
    }
    git(&repo, &["add", "."]).unwrap();
    git(&repo, &["commit", "-qm", "fixtures"]).unwrap();
    let deps = f.success(&["depends", "top"]);
    let names: Vec<_> = deps
        .lines()
        .skip(1)
        .map(|l| l.split('\t').next_back().unwrap())
        .collect();
    assert_eq!(names, ["base", "left", "right", "top"]);
    let out = f.success(&["install", "top"]);
    assert_eq!(out.matches("Installed base ").count(), 1);
    f.success(&["uninstall", "top"]);
    assert!(f.config.root.join("apps/base/current").exists());
    let path = repo.join("base.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["depends"] = json!("top");
    fs::write(&path, value.to_string()).unwrap();
    assert!(String::from_utf8_lossy(&f.run(&["depends", "top"]).stderr).contains("cycle"));
    f.success(&["install", "top", "--independent"]);
    let a = f.source("a", b"#!/bin/sh\nprintf a");
    let a = f.manifest("a", "1", &a, "a");
    f.edit(&a, |v| v["bin"] = json!("a"));
    f.success(&["install", &a]);
    let bytes = tar_bytes(&[("a", b"#!/bin/sh\nprintf b", 0o755)], &[]);
    let b = f.source("b.tar.gz", &gz(&bytes));
    let b = f.manifest("b", "1", &b, "a");
    let output = f.success(&["install", &b]);
    assert!(output.contains("Replacing 'a' from a with b"));
    f.success(&["uninstall", "a"]);
    assert_eq!(
        Command::new(f.config.root.join("bin/a"))
            .output()
            .unwrap()
            .stdout,
        b"b"
    );
    f.success(&["uninstall", "b"]);
    assert!(!f.config.root.join("bin/a").exists());
    // A symlink replacing a managed root subdirectory never grants ownership of external files.
    fs::remove_dir_all(f.config.root.join("bin")).unwrap();
    let external = f.temp.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("base.sh"), "keep").unwrap();
    symlink(&external, f.config.root.join("bin")).unwrap();
    assert!(!f.run(&["uninstall", "base"]).status.success());
    assert_eq!(
        fs::read_to_string(external.join("base.sh")).unwrap(),
        "keep"
    );
}
