use scoop_linux::{manifest::Manifest, platform};
use serde_json::{Value, json};
fn base() -> Value {
    json!({"version":"1.0", "homepage":"https://example.org", "license":"MIT", "url":"https://example.org/data", "hash":"a".repeat(64), "bin":"bin/tool"})
}
fn parse(value: Value) -> anyhow::Result<Manifest> {
    Manifest::parse(value.to_string())
}

#[test]
fn portable_shapes_and_archive_order() {
    let manifest = Manifest::parse(include_str!("fixtures/portable.json").into()).unwrap();
    let target = manifest.target("riscv64", false).unwrap();
    assert_eq!(target.artifacts.len(), 4);
    assert_eq!(target.artifacts[0].extract_dir, None);
    assert_eq!(target.artifacts[1].extract_dir.as_deref(), Some("package"));
    assert_eq!(target.artifacts[1].extract_to.as_deref(), Some("lib"));
    assert_eq!(target.artifacts[2].extract_dir, None);
    assert_eq!(target.artifacts[3].extract_dir.as_deref(), Some("more"));
    assert_eq!(target.artifacts[1].hash, Some("b".repeat(64)));
    assert_eq!(target.bins, ["bin/tool.sh", "other/helper"]);
    assert_eq!(
        manifest.notes(
            std::path::Path::new("/active"),
            std::path::Path::new("/version")
        ),
        ["Active: /active", "Stored: /version"]
    );
}
#[test]
fn exact_architecture_and_inheritance() {
    let mut value = base();
    value["architecture"] = json!({"64bit":{"url":"https://example.org/x64"}, "arm64":{"bin":["arm/tool"], "hash":format!("sha256:{}", "B".repeat(64))}, "riscv64":{}});
    let manifest = parse(value).unwrap();
    let target = manifest.target("arm64", false).unwrap();
    assert_eq!(target.artifacts[0].url, "https://example.org/data");
    assert_eq!(target.artifacts[0].hash, Some("b".repeat(64)));
    assert_eq!(target.bins, ["arm/tool"]);
    assert!(
        manifest
            .target("arm", false)
            .unwrap_err()
            .to_string()
            .contains("exact architecture")
    );
    assert!(manifest.target("32bit", false).is_err());
    assert!(manifest.target("riscv64", false).is_ok());
    let mut value = base();
    value["architecture"] = json!({"64bit":{}});
    assert!(parse(value).unwrap().target("arm64", false).is_err());
}
#[test]
fn architecture_overrides_are_native_for_activation() {
    for (alias, expected) in [
        ("x86_64", "64bit"),
        ("amd64", "64bit"),
        ("i686", "32bit"),
        ("aarch64", "arm64"),
        ("armv7l", "arm"),
        ("riscv64", "riscv64"),
    ] {
        assert_eq!(platform::normalize(alias).unwrap(), expected);
    }
    assert!(platform::select(Some("64bit"), None, "arm64", true).is_err());
    assert!(platform::select(None, Some("64bit"), "arm64", true).is_err());
    assert_eq!(
        platform::select(Some("aarch64"), Some("64bit"), "arm64", true).unwrap(),
        "arm64"
    );
    assert_eq!(
        platform::select(Some("64bit"), None, "arm64", false).unwrap(),
        "64bit"
    );
    assert!(platform::select(Some("arm"), None, "arm64", true).is_err());
}
#[test]
fn required_metadata_and_types() {
    for key in ["version", "homepage", "license"] {
        let mut v = base();
        v.as_object_mut().unwrap().remove(key);
        assert!(parse(v).is_err(), "{key}");
    }
    for (key, bad) in [
        ("version", json!("1/2")),
        ("version", json!(3)),
        ("license", json!({"url":"https://example.org"})),
        ("description", json!([])),
        ("notes", json!([3])),
        ("cookie", json!({"a":3})),
        ("suggest", json!({"a":[3]})),
        ("url", json!([])),
        ("bin", json!([])),
    ] {
        let mut v = base();
        v[key] = bad;
        assert!(parse(v).is_err(), "{key}");
    }
    for version in ["1.0", "1_2-rc+3", "nightly"] {
        let mut v = base();
        v["version"] = json!(version);
        assert!(parse(v).is_ok());
    }
    let mut v = base();
    v["version"] = json!("nightly");
    assert!(parse(v).unwrap().target("arm64", false).is_err());
}
#[test]
fn reject_unsupported_fields_and_launchers() {
    for field in [
        "persist",
        "env_add_path",
        "env_set",
        "installer",
        "uninstaller",
        "pre_install",
        "post_install",
        "pre_uninstall",
        "post_uninstall",
        "innosetup",
        "psmodule",
        "shortcuts",
    ] {
        let mut v = base();
        v[field] = json!(false);
        assert!(parse(v).unwrap_err().to_string().contains(field));
        let mut v = base();
        v["architecture"] = json!({"arm64":{field:[]}});
        assert!(parse(v).unwrap_err().to_string().contains(field));
    }
    for bins in [
        json!([["tool", "alias"]]),
        json!([["tool", "alias", "--fixed"]]),
        json!(["one/tool", "two/tool"]),
        json!("scoop"),
        json!("bin/scoop"),
        json!("/bin/tool"),
        json!("../tool"),
        json!("foo/../../tool"),
        json!("C:\\tool.exe"),
        json!("bin/"),
    ] {
        let mut v = base();
        v["bin"] = bins;
        assert!(parse(v).is_err());
    }
    for field in ["mystery_action", "extract_to", "depends", "cookie"] {
        let mut v = base();
        v["architecture"] = json!({"arm64":{field:"x"}});
        assert!(parse(v).is_err());
    }
    let mut v = base();
    v["unknown"] = json!(true);
    assert!(parse(v).is_err());
}
#[test]
fn checksum_and_pair_validation() {
    for hash in [
        json!("sha1:abcd"),
        json!("g".repeat(64)),
        json!("sha512:".to_owned() + &"a".repeat(128)),
        json!(""),
    ] {
        let mut v = base();
        v["hash"] = hash;
        assert!(parse(v).is_err());
    }
    let mut v = base();
    v.as_object_mut().unwrap().remove("hash");
    let m = parse(v).unwrap();
    assert!(m.target("arm64", false).is_err());
    assert!(m.target("arm64", true).is_ok());
    let mut v = base();
    v["url"] = json!(["https://example.org/a", "https://example.org/b"]);
    let m = parse(v).unwrap();
    assert!(m.target("arm64", false).is_err());
    assert!(m.target("arm64", true).is_err());
    let mut v = base();
    v["extract_dir"] = json!("subdir");
    assert!(parse(v).unwrap().target("arm64", false).is_err());
    for field in ["extract_dir", "extract_to"] {
        let mut v = base();
        v[field] = json!("../escape");
        assert!(parse(v).is_err());
    }
}
