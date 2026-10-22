use scoop_linux::{buckets::git, download, platform, self_update};
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
#[test]
fn bootstrap_installs_trusted_seed_bytes_and_rejects_swapped_catalog_bad_hash_nonelf_http() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main");
    fs::create_dir_all(source.join("bucket")).unwrap();
    fs::write(
        source.join("bucket/data.json"),
        r#"{"version":"1","homepage":"https://example.org","license":"MIT"}"#,
    )
    .unwrap();
    git(&source, &["init", "-q"]).unwrap();
    git(&source, &["add", "."]).unwrap();
    git(
        &source,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.org",
            "commit",
            "-qm",
            "fixture",
        ],
    )
    .unwrap();
    let seed = Path::new(env!("CARGO_BIN_EXE_scoop"));
    let digest = download::digest(seed).unwrap();
    let catalog = temp.path().join("catalog.json");
    let publish = |binary: &Path| {
        fs::write(&catalog,json!({"format_version":1,"version":env!("CARGO_PKG_VERSION"),"main_bucket_url":source,"targets":{self_update::BUILD_TARGET:{"architecture":platform::host().unwrap(),"os":"linux","libc":"glibc","abi":"gnu","minimum_libc":"2.17","url":reqwest::Url::from_file_path(binary).unwrap().as_str(),"sha256":download::digest(binary).unwrap()}}}).to_string()).unwrap();
    };
    let execute = |name: &str, catalog: &str, binary: &Path, hash: &str| -> Output {
        Command::new("sh")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bootstrap.sh"))
            .arg(catalog)
            .arg(reqwest::Url::from_file_path(binary).unwrap().as_str())
            .arg(hash)
            .env("HOME", temp.path().join(name))
            .env("SCOOP", temp.path().join(name).join("root"))
            .env("XDG_CONFIG_HOME", temp.path().join(name).join("config"))
            .output()
            .unwrap()
    };
    let swapped = temp.path().join("swapped");
    let mut bytes = fs::read(seed).unwrap();
    bytes.extend_from_slice(b"swapped-catalog-bytes");
    fs::write(&swapped, bytes).unwrap();
    publish(&swapped);
    let out = execute("swapped-home", catalog.to_str().unwrap(), seed, &digest);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not match the trusted seed"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!temp.path().join("swapped-home/root/bin/scoop").exists());
    publish(seed);
    let out = execute("bad-hash", catalog.to_str().unwrap(), seed, &"a".repeat(64));
    assert!(!out.status.success());
    let text = temp.path().join("nonelf");
    fs::write(&text, "#!/bin/sh\nexit 0\n").unwrap();
    let out = execute(
        "nonelf",
        catalog.to_str().unwrap(),
        &text,
        &download::digest(&text).unwrap(),
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("ELF"));
    let out = execute("http-catalog", "http://127.0.0.1:1/catalog", seed, &digest);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("HTTPS"));
    let out = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bootstrap.sh"))
        .arg(&catalog)
        .arg("http://127.0.0.1:1/scoop")
        .arg(&digest)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("HTTPS"));
    let out = execute("good", catalog.to_str().unwrap(), seed, &digest);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        download::digest(&temp.path().join(format!(
            "good/root/apps/scoop/{}/scoop",
            env!("CARGO_PKG_VERSION")
        )))
        .unwrap(),
        digest
    );
}
