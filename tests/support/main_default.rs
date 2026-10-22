use scoop_linux::{
    buckets::{self, Buckets},
    download, platform, self_update,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const MAIN: &str = "https://github.com/zhzy0077/scoop-linux-pkgs";
struct Fixture {
    temp: tempfile::TempDir,
    source: PathBuf,
    git_config: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("published-main");
        fs::create_dir_all(source.join("bucket")).unwrap();
        for entry in
            fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("main/bucket")).unwrap()
        {
            let path = entry.unwrap().path();
            fs::copy(&path, source.join("bucket").join(path.file_name().unwrap())).unwrap();
        }
        for args in [
            &["init", "-q"][..],
            &["add", "."][..],
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.org",
                "commit",
                "-qm",
                "Reviewed main payload",
            ][..],
        ] {
            buckets::git(&source, args).unwrap();
        }
        let git_config = temp.path().join("gitconfig");
        let rewrite = format!(
            "url.{}.insteadOf",
            reqwest::Url::from_directory_path(&source).unwrap()
        );
        assert!(
            Command::new("git")
                .args(["config", "--file"])
                .arg(&git_config)
                .args([&rewrite, MAIN])
                .status()
                .unwrap()
                .success()
        );
        Self {
            temp,
            source,
            git_config,
        }
    }
    fn root(&self, profile: &str) -> PathBuf {
        self.temp.path().join(profile).join("root")
    }
    fn command(&self, profile: &str, binary: &Path) -> Command {
        let home = self.temp.path().join(profile);
        let mut c = Command::new(binary);
        c.current_dir(self.temp.path())
            .env("HOME", &home)
            .env("SCOOP", self.root(profile))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("GIT_CONFIG_GLOBAL", &self.git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("SCOOP_MAIN_BUCKET")
            .env_remove("SCOOP_RELEASE_CATALOG");
        c
    }
    fn cli(&self, profile: &str) -> Command {
        self.command(profile, Path::new(env!("CARGO_BIN_EXE_scoop")))
    }
    fn good(output: Output) -> String {
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn ok(&self, profile: &str, args: &[&str]) -> String {
        Self::good(self.cli(profile).args(args).output().unwrap())
    }
    fn origin(&self, profile: &str) -> String {
        buckets::git(
            &self.root(profile).join("buckets/main"),
            &["config", "--get", "remote.origin.url"],
        )
        .unwrap()
    }
}
#[test]
fn canonical_main_known_add_and_reviewed_manifest_discovery() {
    let f = Fixture::new();
    assert_eq!(Buckets::known()["main"], MAIN);
    assert_eq!(
        f.ok("default", &["bucket", "known"]),
        format!("main\t{MAIN}\n")
    );
    f.ok("default", &["bucket", "add", "main"]);
    assert_eq!(f.origin("default"), MAIN);
    let actual: Value = serde_json::from_str(&f.ok("default", &["cat", "jq"])).unwrap();
    let reviewed: Value =
        serde_json::from_slice(&fs::read(f.source.join("bucket/jq.json")).unwrap()).unwrap();
    assert_eq!(actual, reviewed);
    assert!(f.ok("default", &["search", "jq"]).contains("jq"));
}
#[test]
fn explicit_environment_and_config_bucket_sources_override_canonical_main() {
    let f = Fixture::new();
    let path = f.source.to_str().unwrap();
    let url = reqwest::Url::from_directory_path(&f.source)
        .unwrap()
        .to_string();
    f.ok("config", &["config", "main_bucket_url", path]);
    assert_eq!(
        f.ok("config", &["bucket", "known"]),
        format!("main\t{path}\n")
    );
    f.ok("config", &["bucket", "add", "main"]);
    assert_eq!(f.origin("config"), path);
    f.ok("env", &["config", "main_bucket_url", path]);
    assert_eq!(
        Fixture::good(
            f.cli("env")
                .env("SCOOP_MAIN_BUCKET", &url)
                .args(["bucket", "known"])
                .output()
                .unwrap()
        ),
        format!("main\t{url}\n")
    );
    Fixture::good(
        f.cli("env")
            .env("SCOOP_MAIN_BUCKET", &url)
            .args(["bucket", "add", "main"])
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("env"), url);
    Fixture::good(
        f.cli("explicit")
            .env("SCOOP_MAIN_BUCKET", &url)
            .args(["bucket", "add", "main", path])
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("explicit"), path);
}
#[test]
fn bootstrap_uses_canonical_main_fallback_and_preserves_source_precedence() {
    let f = Fixture::new();
    let seed = Path::new(env!("CARGO_BIN_EXE_scoop"));
    let catalog = f.temp.path().join("catalog.json");
    let mut document = json!({"format_version":1,"version":env!("CARGO_PKG_VERSION"),"targets":{self_update::BUILD_TARGET:{"architecture":platform::host().unwrap(),"os":"linux","libc":"glibc","abi":"gnu","minimum_libc":"2.39","url":reqwest::Url::from_file_path(seed).unwrap().as_str(),"sha256":download::digest(seed).unwrap()}}});
    fs::write(&catalog, document.to_string()).unwrap();
    Fixture::good(
        f.cli("default")
            .arg("--bootstrap")
            .arg(&catalog)
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("default"), MAIN);
    assert_eq!(
        f.ok("default", &["config", "main_bucket_url"]),
        format!("{MAIN}\n")
    );
    assert_eq!(
        Fixture::good(
            f.command("default", &f.root("default").join("bin/scoop"))
                .arg("--version")
                .output()
                .unwrap()
        ),
        format!("scoop {}\n", env!("CARGO_PKG_VERSION"))
    );
    let url = reqwest::Url::from_directory_path(&f.source)
        .unwrap()
        .to_string();
    document["main_bucket_url"] = json!(url);
    fs::write(&catalog, document.to_string()).unwrap();
    Fixture::good(
        f.cli("catalog")
            .arg("--bootstrap")
            .arg(&catalog)
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("catalog"), url);
    let path = f.source.to_str().unwrap();
    f.ok("config", &["config", "main_bucket_url", path]);
    Fixture::good(
        f.cli("config")
            .arg("--bootstrap")
            .arg(&catalog)
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("config"), path);
    f.ok("env", &["config", "main_bucket_url", path]);
    Fixture::good(
        f.cli("env")
            .env("SCOOP_MAIN_BUCKET", MAIN)
            .arg("--bootstrap")
            .arg(&catalog)
            .output()
            .unwrap(),
    );
    assert_eq!(f.origin("env"), MAIN);
}
