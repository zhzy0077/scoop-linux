use scoop_linux::{buckets::git, download, platform, self_update};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
#[cfg(feature = "test-hooks")]
#[path = "support/self_update_real.rs"]
mod real;
#[path = "support/self_update_update.rs"]
mod update_failures;
struct Fixture {
    temp: tempfile::TempDir,
    source: PathBuf,
    catalog: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("bucket")).unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "fixture@example.org"][..],
            &["config", "user.name", "Fixture"][..],
        ] {
            git(&source, args).unwrap();
        }
        fs::write(
            source.join("bucket/data.json"),
            r#"{"version":"1","homepage":"https://example.org","license":"MIT"}"#,
        )
        .unwrap();
        git(&source, &["add", "."]).unwrap();
        git(&source, &["commit", "-qm", "first"]).unwrap();
        let catalog = temp.path().join("catalog.json");
        Self {
            temp,
            source,
            catalog,
        }
    }
    fn root(&self) -> PathBuf {
        self.temp.path().join("root")
    }
    fn cli(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        command
            .env("HOME", self.temp.path())
            .env("SCOOP", self.root())
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env(
                "SCOOP_RELEASE_EXECUTION_LOG",
                self.temp.path().join("executed"),
            )
            .env("REFRESH_EXECUTION_LOG", self.temp.path().join("refreshed"));
        command
    }
    fn seed(&self) -> Command {
        self.cli(Path::new(env!("CARGO_BIN_EXE_scoop")))
    }
    fn binary(&self, version: &str) -> PathBuf {
        let source = self.temp.path().join(format!("release-{version}.c"));
        let binary = self.temp.path().join(format!("release-{version}"));
        let real = serde_json::to_string(env!("CARGO_BIN_EXE_scoop")).unwrap();
        let text = format!(
            r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc,char **argv){{if(argc==2&&!strcmp(argv[1],"--version")){{puts("scoop {version}");return 0;}}char *log=getenv("SCOOP_RELEASE_EXECUTION_LOG");if(log){{FILE *file=fopen(log,"a");if(!file)return 110;fputs("{version}\n",file);fclose(file);}}setenv("SCOOP_FIXTURE_RELEASE","{version}",1);execv({real},argv);perror("execv");return 111;}}
"#
        );
        fs::write(&source, text).unwrap();
        let result = Command::new("cc")
            .args(["-O0", "-o"])
            .arg(&binary)
            .arg(source)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        binary
    }
    fn publish(&self, version: &str, binary: &Path) {
        let (libc, _) = self_update::runtime_libc();
        let abi = if cfg!(target_env = "gnu") {
            "gnu"
        } else {
            "musl"
        };
        let value = json!({"format_version":1,"version":version,"main_bucket_url":self.source.to_str().unwrap(),"targets":{self_update::BUILD_TARGET:{"architecture":platform::host().unwrap(),"os":"linux","libc":libc,"abi":abi,"minimum_libc":if libc=="glibc"{Some("2.17")}else{None},"url":reqwest::Url::from_file_path(binary).unwrap().as_str(),"sha256":download::digest(binary).unwrap()}}});
        fs::write(&self.catalog, value.to_string()).unwrap();
    }
    fn bootstrap(&self) {
        let out = self
            .seed()
            .arg("--bootstrap")
            .arg(&self.catalog)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout)
                .contains(self.root().join("bin").to_str().unwrap())
        );
    }
    fn current(&self) -> String {
        fs::read_link(self.root().join("apps/scoop/current"))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
}
#[test]
fn native_frontends_verify_hashes_handoff_order_and_bucket_refresh() {
    let f = Fixture::new();
    let one = f.binary("1.0.0");
    f.publish("1.0.0", &one);
    f.bootstrap();
    assert_eq!(f.current(), "1.0.0");
    let known = f.seed().args(["bucket", "known"]).output().unwrap();
    assert!(known.status.success());
    assert!(String::from_utf8_lossy(&known.stdout).contains(f.source.to_str().unwrap()));
    assert!(
        f.seed()
            .args(["bucket", "rm", "main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        f.seed()
            .args(["bucket", "add", "main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read_link(f.root().join("bin/scoop")).unwrap(),
        Path::new("../apps/scoop/current/scoop")
    );
    let hook = f.root().join("buckets/main/.git/hooks/post-merge");
    fs::write(
        &hook,
        b"#!/bin/sh\nprintf '%s' \"$SCOOP_FIXTURE_RELEASE\" > \"$REFRESH_EXECUTION_LOG\"\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        f.source.join("bucket/data.json"),
        r#"{"version":"2","homepage":"https://example.org","license":"MIT"}"#,
    )
    .unwrap();
    git(&f.source, &["add", "."]).unwrap();
    git(&f.source, &["commit", "-qm", "second"]).unwrap();
    let two = f.binary("2.0.0");
    f.publish("2.0.0", &two);
    let out = f
        .cli(&f.root().join("bin/scoop"))
        .arg("update")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.current(), "2.0.0");
    assert_eq!(
        fs::read_to_string(f.temp.path().join("executed")).unwrap(),
        "1.0.0\n2.0.0\n"
    );
    assert_eq!(
        fs::read_to_string(f.temp.path().join("refreshed")).unwrap(),
        "2.0.0"
    );
    assert!(
        fs::read_to_string(f.root().join("buckets/main/bucket/data.json"))
            .unwrap()
            .contains("\"2\"")
    );
    assert_eq!(
        download::digest(&f.root().join("apps/scoop/1.0.0/scoop")).unwrap(),
        download::digest(&one).unwrap()
    );
    assert_eq!(
        download::digest(&f.root().join("apps/scoop/2.0.0/scoop")).unwrap(),
        download::digest(&two).unwrap()
    );
    let mut value: Value = serde_json::from_slice(&fs::read(&f.catalog).unwrap()).unwrap();
    value["version"] = json!("3.0.0");
    value["targets"][self_update::BUILD_TARGET]["sha256"] = json!("b".repeat(64));
    fs::write(&f.catalog, value.to_string()).unwrap();
    assert!(
        !f.cli(&f.root().join("bin/scoop"))
            .arg("update")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(f.current(), "2.0.0");
    let out = f
        .cli(&f.root().join("bin/scoop"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"scoop 2.0.0\n");
}
#[test]
fn automatic_download_refresh_honors_no_update_and_reexecutes() {
    let f = Fixture::new();
    let one = f.binary("1.0.0");
    f.publish("1.0.0", &one);
    f.bootstrap();
    let two = f.binary("2.0.0");
    f.publish("2.0.0", &two);
    let artifact = f.temp.path().join("payload");
    fs::write(&artifact, b"independent data").unwrap();
    let manifest = f.temp.path().join("payload.json");
    fs::write(&manifest, json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":download::digest(&artifact).unwrap()}).to_string()).unwrap();
    assert!(
        f.seed()
            .args(["config", "last_update", "2000-01-01T00:00:00Z"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let out = f
        .cli(&f.root().join("bin/scoop"))
        .arg("download")
        .arg(&manifest)
        .arg("-u")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(f.current(), "1.0.0");
    let out = f
        .cli(&f.root().join("bin/scoop"))
        .arg("download")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.current(), "2.0.0");
    assert!(
        fs::read_to_string(f.temp.path().join("executed"))
            .unwrap()
            .ends_with("2.0.0\n")
    );
}
#[cfg(feature = "test-hooks")]
#[test]
fn interrupted_self_activation_recovers_before_next_command() {
    for point in ["prepared", "published", "current", "links", "committed"] {
        let f = Fixture::new();
        let one = f.binary("1.0.0");
        f.publish("1.0.0", &one);
        f.bootstrap();
        let two = f.binary("2.0.0");
        f.publish("2.0.0", &two);
        let out = f
            .seed()
            .arg("update")
            .env("SCOOP_TEST_FAILPOINT", point)
            .env("SCOOP_TEST_CRASH", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(86));
        let out = f.seed().arg("cache").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            f.current(),
            if point == "committed" {
                "2.0.0"
            } else {
                "1.0.0"
            }
        );
        assert_eq!(
            fs::read_dir(f.root().join(".transactions"))
                .unwrap()
                .count(),
            0
        );
    }
}
#[test]
fn compatibility_checksum_and_version_fail_before_self_activation() {
    for failure in ["cpu", "abi", "minimum", "checksum", "missing", "version"] {
        let f = Fixture::new();
        let binary = f.binary("1.0.0");
        f.publish("1.0.0", &binary);
        let mut value: Value = serde_json::from_slice(&fs::read(&f.catalog).unwrap()).unwrap();
        match failure {
            "cpu" => {
                value["targets"][self_update::BUILD_TARGET]["architecture"] =
                    json!(if platform::host().unwrap() == "arm64" {
                        "64bit"
                    } else {
                        "arm64"
                    })
            }
            "abi" => value["targets"][self_update::BUILD_TARGET]["abi"] = json!("foreign"),
            "minimum" => {
                value["targets"][self_update::BUILD_TARGET]["minimum_libc"] = json!("999.0")
            }
            "checksum" => {
                value["targets"][self_update::BUILD_TARGET]["sha256"] = json!("a".repeat(64))
            }
            "missing" => value["targets"] = json!({}),
            "version" => value["version"] = json!("2.0.0"),
            _ => unreachable!(),
        };
        fs::write(&f.catalog, value.to_string()).unwrap();
        let out = f
            .seed()
            .arg("--bootstrap")
            .arg(&f.catalog)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{failure}");
        assert!(!f.root().join("apps/scoop/current").exists());
        assert!(!f.root().join("bin/scoop").exists());
    }
}
#[test]
fn self_hold_still_refreshes_buckets_and_unmanaged_bootstrap_files_survive() {
    let f = Fixture::new();
    let one = f.binary("1.0.0");
    f.publish("1.0.0", &one);
    f.bootstrap();
    let two = f.binary("2.0.0");
    f.publish("2.0.0", &two);
    let path = f.temp.path().join("config/scoop/config.json");
    let stale = scoop_linux::config::Config {
        path: path.clone(),
        root: f.root(),
        cache: f.root().join("cache"),
        values: serde_json::from_slice(&fs::read(&path).unwrap()).unwrap(),
    };
    let out = f.seed().args(["hold", "scoop"]).output().unwrap();
    assert!(out.status.success());
    assert!(self_update::update(&stale).unwrap().is_none());
    assert_eq!(f.current(), "1.0.0");
    let out = f
        .cli(&f.root().join("bin/scoop"))
        .arg("update")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.current(), "1.0.0");
    assert!(String::from_utf8_lossy(&out.stdout).contains("Updated bucket main"));
    let f = Fixture::new();
    let binary = f.binary("1.0.0");
    f.publish("1.0.0", &binary);
    fs::create_dir_all(f.root().join("bin")).unwrap();
    fs::write(f.root().join("bin/scoop"), "unrelated").unwrap();
    assert!(
        !f.seed()
            .arg("--bootstrap")
            .arg(&f.catalog)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(f.root().join("bin/scoop")).unwrap(),
        "unrelated"
    );
}
