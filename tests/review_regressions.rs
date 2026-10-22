use scoop_linux::buckets::git;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Output},
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
    fn cli(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scoop"));
        c.env("HOME", self.temp.path().join("home"))
            .env("SCOOP", self.temp.path().join("root"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .current_dir(self.temp.path());
        c
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.cli().args(args).output().unwrap();
        success(o)
    }
    fn repo(&self, name: &str) -> std::path::PathBuf {
        let repo = self.temp.path().join("root/buckets").join(name);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]).unwrap();
        git(&repo, &["config", "user.email", "fixture@example.org"]).unwrap();
        git(&repo, &["config", "user.name", "Fixture"]).unwrap();
        repo
    }
}
fn success(o: Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
fn manifest(version: &str) -> String {
    json!({"version":version,"homepage":"https://example.org","license":"MIT","url":"https://example.org/data"}).to_string()
}
fn commit(repo: &std::path::Path) {
    git(repo, &["add", "."]).unwrap();
    git(repo, &["commit", "-qm", "fixture"]).unwrap();
}
#[test]
fn f1_empty_and_relative_environment_values() {
    let f = Fixture::new();
    for xdg in ["", "relative"] {
        success(
            f.cli()
                .env("SCOOP", "")
                .env("XDG_CONFIG_HOME", xdg)
                .args(["config", "example", "value"])
                .output()
                .unwrap(),
        );
        assert!(
            f.temp
                .path()
                .join("home/.config/scoop/config.json")
                .exists()
        );
        assert!(!f.temp.path().join("relative").exists());
        assert!(!f.temp.path().join("scoop/config.json").exists());
    }
    success(
        f.cli()
            .env("HOME", "")
            .args(["config", "example", "value"])
            .output()
            .unwrap(),
    );
    assert!(f.temp.path().join("config/scoop/config.json").exists());
}
#[test]
fn f2_bucket_list_preserves_and_reports_unusual_directories() {
    let f = Fixture::new();
    fs::create_dir_all(f.temp.path().join("root/buckets/plain")).unwrap();
    f.repo("empty");
    let repo = f.repo("no-origin");
    fs::write(repo.join("tool.json"), manifest("1")).unwrap();
    commit(&repo);
    let output = f.ok(&["bucket", "list"]);
    for name in ["plain", "empty", "no-origin"] {
        assert!(output.contains(&format!("{name}\t")));
    }
    assert!(output.contains("<unavailable>"));
}
#[test]
fn f3_history_skips_broken_json_and_preserves_unicode_paths() {
    let f = Fixture::new();
    let repo = f.repo("local");
    fs::create_dir_all(repo.join("bucket/日本語")).unwrap();
    let path = repo.join("bucket/日本語/tool.json");
    fs::write(&path, manifest("1")).unwrap();
    fs::write(repo.join("bucket/日本語/sibling.json"), manifest("sibling")).unwrap();
    commit(&repo);
    fs::write(&path, "{broken}").unwrap();
    commit(&repo);
    fs::write(&path, manifest("3")).unwrap();
    commit(&repo);
    assert_eq!(f.ok(&["cat", "local/tool@1"]), manifest("1"));
}
#[test]
fn f4_cwd_shadowing_and_literal_at_filenames() {
    let f = Fixture::new();
    let repo = f.repo("local");
    fs::write(repo.join("tool.json"), manifest("1")).unwrap();
    commit(&repo);
    fs::write(f.temp.path().join("tool"), "not json").unwrap();
    assert_eq!(f.ok(&["cat", "tool"]), manifest("1"));
    let file = f.temp.path().join("a@b.json");
    fs::write(&file, manifest("2")).unwrap();
    assert_eq!(f.ok(&["cat", file.to_str().unwrap()]), manifest("2"));
    assert_eq!(
        f.ok(&["cat", &format!("{}@2", file.display())]),
        manifest("2")
    );
}
#[test]
fn f5_metadata_display_never_enforces_installability() {
    let f = Fixture::new();
    let path = f.temp.path().join("info.json");
    for value in [
        serde_json::from_str::<Value>(&manifest("nightly")).unwrap(),
        json!({"version":"1","homepage":"https://example.org","license":{"identifier":"MIT","url":"https://example.org/license"},"architecture":{"riscv64":{"url":"https://example.org/foreign"}}}),
    ] {
        fs::write(&path, value.to_string()).unwrap();
        let output = f.ok(&["info", path.to_str().unwrap(), "-v"]);
        assert!(output.contains("License:     MIT"));
        assert!(output.contains("Provenance:  file="));
        assert!(!output.contains("File(\""));
    }
    fs::write(&path, manifest("1")).unwrap();
    assert!(
        f.ok(&["info", path.to_str().unwrap(), "-v"])
            .contains("URL:         https://example.org/data")
    );
}
#[test]
fn f6_config_strings_booleans_sentinel_and_corruption_preservation() {
    let f = Fixture::new();
    for (key, value) in [
        ("proxy", "8080"),
        ("array", "[1,2]"),
        ("object", "{\"a\":1}"),
        ("flag", "true"),
    ] {
        f.ok(&["config", key, value]);
    }
    let path = f.temp.path().join("config/scoop/config.json");
    let v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(v["proxy"], "8080");
    assert_eq!(v["array"], "[1,2]");
    assert_eq!(v["object"], "{\"a\":1}");
    assert_eq!(v["flag"], true);
    f.ok(&["config", "flag", "$null"]);
    let v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(v.get("flag").is_none());
    fs::write(&path, "{broken original").unwrap();
    let o = f.cli().args(["config", "rm", "flag"]).output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains(path.to_str().unwrap()));
    assert_eq!(fs::read_to_string(path).unwrap(), "{broken original");
}
#[test]
fn f7_canonical_bin_paths_and_f8_double_dash() {
    for bin in ["bin/.", "./x", "a//b", "x/", "."] {
        let mut v: Value = serde_json::from_str(&manifest("1")).unwrap();
        v["bin"] = json!(bin);
        assert!(
            scoop_linux::manifest::Manifest::parse(v.to_string()).is_err(),
            "{bin}"
        );
    }
    let f = Fixture::new();
    f.ok(&["search", "--", "-p"]);
    f.ok(&["config", "literal", "--", "--global"]);
    assert_eq!(f.ok(&["config", "literal"]), "--global\n");
}
#[test]
fn f9_manifest_body_limit() {
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        use std::io::Read;
        let (mut stream, _) = server.accept().unwrap();
        let mut b = [0; 4096];
        assert!(stream.read(&mut b).unwrap() > 0);
        let size = 4 * 1024 * 1024 + 1;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(&vec![b'x'; size]);
    });
    let error =
        scoop_linux::buckets::http_text(&format!("http://{address}/large.json")).unwrap_err();
    assert!(error.to_string().contains("4 MiB"));
    thread.join().unwrap();
}
#[test]
fn f10_mutation_edges_and_dirty_provenance() {
    let f = Fixture::new();
    let repo = f.repo("source");
    fs::write(repo.join("tool.json"), manifest("1")).unwrap();
    commit(&repo);
    let registry = f.temp.path().join("root/.bucket-sources.json");
    fs::create_dir(&registry).unwrap();
    assert!(
        !f.cli()
            .args(["bucket", "add", "failed", repo.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!f.temp.path().join("root/buckets/failed").exists());
    assert!(
        !f.cli()
            .args(["bucket", "add", ".bucket-hidden", repo.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    let o = f.cli().args(["bucket", "rm", "missing"]).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("not found"));
    fs::write(repo.join("tool.json"), manifest("2")).unwrap();
    let output = f.ok(&["info", "source/tool", "-v"]);
    assert!(output.contains("working_tree=dirty"));
}
#[test]
fn f11_help_cache_bom_and_archive_case() {
    let f = Fixture::new();
    assert!(f.ok(&["help", "install"]).contains("Usage: scoop install"));
    let o = f.cli().arg("cache").output().unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("Total: 0 files"));
    let path = f.temp.path().join("bom.json");
    let raw = format!("\u{feff}{}", manifest("1"));
    fs::write(&path, &raw).unwrap();
    assert_eq!(f.ok(&["cat", path.to_str().unwrap()]), raw);
    let unsupported =
        json!({"version":"1","homepage":"https://example.org","license":"MIT","persist":"data"})
            .to_string();
    fs::write(&path, &unsupported).unwrap();
    let output = f
        .cli()
        .args(["cat", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), unsupported);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported manifest feature: persist")
    );
    assert!(scoop_linux::manifest::is_archive("https://example.org/UPPER.ZIP").unwrap());
    assert!(scoop_linux::manifest::is_archive("https://example.org/UPPER.TAR.GZ").unwrap());
    assert!(scoop_linux::manifest::is_archive("https://example.org/archive%2EZIP").unwrap());
}
