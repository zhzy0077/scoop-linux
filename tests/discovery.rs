use scoop_linux::{
    buckets::{Buckets, git},
    config::Config,
    resolve::{Resolver, Source},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;
struct Fixture {
    temp: TempDir,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let config = Config {
            path: temp.path().join("config/scoop/config.json"),
            values: Default::default(),
            cache: root.join("cache"),
            root,
        };
        Self { temp, config }
    }
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scoop"))
            .args(args)
            .env("HOME", self.temp.path())
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("SCOOP", &self.config.root)
            .output()
            .unwrap()
    }
    fn repo(&self, bucket: &str, modern: bool) -> PathBuf {
        let repo = self.config.root.join("buckets").join(bucket);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]).unwrap();
        git(&repo, &["config", "user.email", "fixture@example.org"]).unwrap();
        git(&repo, &["config", "user.name", "Fixture"]).unwrap();
        if modern {
            fs::create_dir(repo.join("bucket")).unwrap();
        }
        repo
    }
}
fn manifest(version: &str, bin: &str) -> String {
    json!({"version":version,"homepage":"https://example.org","license":"MIT","url":"https://example.org/data","hash":"a".repeat(64),"bin":bin}).to_string()+"\n"
}
fn commit(repo: &Path, path: &str, version: &str) -> String {
    let path = repo.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, manifest(version, "bin/EXACT.exe")).unwrap();
    git(repo, &["add", "."]).unwrap();
    git(repo, &["commit", "-qm", version]).unwrap();
    git(repo, &["rev-parse", "HEAD"]).unwrap()
}
fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn error(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
#[test]
fn local_git_history_precedence_and_provenance() {
    let f = Fixture::new();
    let a = f.repo("alpha", true);
    let z = f.repo("zeta", false);
    let old = commit(&a, "bucket/nested/tool.json", "1.0");
    let current = commit(&a, "bucket/nested/tool.json", "2.0");
    commit(&z, "tool.json", "9.0");
    let resolver = Resolver::new(&f.config);
    let r = resolver.resolve("tool").unwrap();
    assert_eq!(r.manifest.version(), "2.0");
    assert_eq!(r.alternatives, ["zeta"]);
    assert!(
        matches!(r.source,Source::Bucket {ref name,ref commit,..} if name=="alpha" && commit==&current)
    );
    let r = resolver.resolve("alpha/tool@1.0").unwrap();
    assert_eq!(r.manifest.version(), "1.0");
    assert_eq!(r.manifest.raw, manifest("1.0", "bin/EXACT.exe"));
    assert!(matches!(r.source,Source::Bucket {ref commit,..} if commit==&old));
    assert_eq!(
        resolver.resolve("zeta/tool").unwrap().manifest.version(),
        "9.0"
    );
    assert!(resolver.resolve("tool@9.0").is_err());
    assert!(
        resolver
            .resolve("alpha/tool@404")
            .unwrap_err()
            .to_string()
            .contains("matching manifest")
    );
    assert!(resolver.resolve("../tool").is_err());
    let output = f.cli(&["cat", "alpha/tool"]);
    assert!(output.status.success(), "{}", error(&output));
    assert_eq!(text(&output), manifest("2.0", "bin/EXACT.exe"));
    let output = f.cli(&["search", "^EXACT\\.exe$"]);
    assert!(output.status.success(), "{}", error(&output));
    assert!(text(&output).contains("tool\t2.0\talpha\tEXACT.exe"));
    let output = f.cli(&["search", "[invalid"]);
    assert!(!output.status.success());
    let output = f.cli(&["info", "tool", "-v"]);
    assert!(output.status.success(), "{}", error(&output));
    assert!(text(&output).contains("Source:      alpha"));
    assert!(text(&output).contains(&current));
}
#[test]
fn installed_source_and_explicit_selection() {
    let f = Fixture::new();
    let a = f.repo("alpha", true);
    let z = f.repo("zeta", false);
    commit(&a, "bucket/tool.json", "2");
    commit(&z, "tool.json", "9");
    let current = f.config.root.join("apps/tool/current");
    fs::create_dir_all(&current).unwrap();
    fs::write(current.join("scoop-install.json"), r#"{"bucket":"zeta"}"#).unwrap();
    let resolver = Resolver::new(&f.config);
    assert_eq!(resolver.resolve("tool").unwrap().manifest.version(), "9");
    assert_eq!(
        resolver.resolve("alpha/tool").unwrap().manifest.version(),
        "2"
    );
}
#[test]
fn local_file_qualified_version_and_removed_history() {
    let f = Fixture::new();
    let path = f.temp.path().join("local.json");
    fs::write(&path, manifest("1.0", "tool.sh")).unwrap();
    let resolver = Resolver::new(&f.config);
    let operand = format!("{}@1.0", path.display());
    assert_eq!(resolver.resolve(&operand).unwrap().name, "local");
    assert!(
        resolver
            .resolve(&format!("{}@2", path.display()))
            .unwrap_err()
            .to_string()
            .contains("matching manifest required")
    );
    let repo = f.repo("old", false);
    commit(&repo, "tool.json", "1");
    git(&repo, &["rm", "tool.json"]).unwrap();
    git(&repo, &["commit", "-qm", "remove"]).unwrap();
    assert_eq!(
        resolver.resolve("old/tool@1").unwrap().manifest.version(),
        "1"
    );
}
#[test]
fn actual_bucket_clone_list_and_safe_remove() {
    let f = Fixture::new();
    let source = f.temp.path().join("source");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q"]).unwrap();
    git(&source, &["config", "user.email", "fixture@example.org"]).unwrap();
    git(&source, &["config", "user.name", "Fixture"]).unwrap();
    commit(&source, "bucket/tool.json", "1");
    let output = f.cli(&["bucket", "add", "test", source.to_str().unwrap()]);
    assert!(output.status.success(), "{}", error(&output));
    let output = f.cli(&["bucket", "list"]);
    assert!(output.status.success(), "{}", error(&output));
    assert!(text(&output).contains("test\t"));
    assert!(
        !f.cli(&["bucket", "add", "test", source.to_str().unwrap()])
            .status
            .success()
    );
    let repo = f.config.root.join("buckets/test");
    fs::write(repo.join("unrelated"), "keep").unwrap();
    assert!(!f.cli(&["bucket", "rm", "test"]).status.success());
    assert_eq!(fs::read_to_string(repo.join("unrelated")).unwrap(), "keep");
    fs::remove_file(repo.join("unrelated")).unwrap();
    assert!(f.cli(&["bucket", "rm", "test"]).status.success());
    assert!(!repo.exists());
    let unrelated = f.config.root.join("buckets/unrelated");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("data"), "keep").unwrap();
    assert!(Buckets::new(&f.config.root).remove("unrelated").is_err());
    assert!(unrelated.join("data").exists());
    assert!(!f.cli(&["bucket", "add", "unconfigured"]).status.success());
}
#[test]
fn configuration_roundtrip_and_root_precedence() {
    let f = Fixture::new();
    for args in [
        vec!["config", "force_update", "true"],
        vec!["config", "custom_number", "7"],
        vec!["config", "custom_text", "你好"],
        vec!["config", "default_architecture", "aarch64"],
    ] {
        let output = f.cli(&args);
        assert!(output.status.success(), "{}", error(&output));
    }
    assert_eq!(text(&f.cli(&["config", "force_update"])), "true\n");
    assert_eq!(text(&f.cli(&["config", "custom_number"])), "7\n");
    assert_eq!(text(&f.cli(&["config", "custom_text"])), "你好\n");
    assert!(f.cli(&["config", "rm", "custom_number"]).status.success());
    assert!(text(&f.cli(&["config", "custom_number"])).contains("not set"));
    let output = f.cli(&["config", "aria2_enabled", "true"]);
    assert!(!output.status.success());
    assert!(error(&output).contains("unsupported config feature"));
    assert!(!f.cli(&["config", "root_path", "relative"]).status.success());
    let configured = f.temp.path().join("configured");
    assert!(
        f.cli(&["config", "root_path", configured.to_str().unwrap()])
            .status
            .success()
    );
    let output = f.cli(&["bucket", "add", "new", "/missing/source"]);
    assert!(!output.status.success());
    assert!(f.config.root.exists());
    assert!(!configured.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
        .args(["bucket", "add", "new", "/missing/source"])
        .env("HOME", f.temp.path())
        .env("XDG_CONFIG_HOME", f.temp.path().join("config"))
        .env_remove("SCOOP")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(configured.exists());
}
#[test]
fn http_manifest_and_version_qualification() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let f = Fixture::new();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = server.accept().unwrap();
            let mut buffer = [0; 4096];
            let received = stream.read(&mut buffer).unwrap();
            assert!(received > 0);
            let body = manifest("1", "tool");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let resolver = Resolver::new(&f.config);
    let url = format!("http://{address}/remote.json");
    let r = resolver.resolve(&format!("{url}@1")).unwrap();
    assert_eq!(r.name, "remote");
    assert!(matches!(r.source, Source::Url(_)));
    assert!(resolver.resolve(&format!("{url}@2")).is_err());
    thread.join().unwrap();
}
