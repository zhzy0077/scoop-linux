use super::*;
#[test]
fn update_rejects_version_elf_and_musl_metadata_and_preserves_active_release() {
    for failure in ["version", "elf", "musl", "library", "foreign-loader"] {
        let f = Fixture::new();
        let one = f.binary("1.0.0");
        f.publish("1.0.0", &one);
        f.bootstrap();
        let two = f.binary("2.0.0");
        if failure == "elf" {
            let mut bytes = fs::read(&two).unwrap();
            let foreign: u16 = if platform::host().unwrap() == "arm64" {
                62
            } else {
                183
            };
            bytes[18..20].copy_from_slice(&foreign.to_le_bytes());
            fs::write(&two, bytes).unwrap();
        }
        f.publish(
            if failure == "version" {
                "3.0.0"
            } else {
                "2.0.0"
            },
            &two,
        );
        let mut catalog: Value = serde_json::from_slice(&fs::read(&f.catalog).unwrap()).unwrap();
        if failure == "musl" {
            catalog["targets"][self_update::BUILD_TARGET]["libc"] = json!("musl-static");
            catalog["targets"][self_update::BUILD_TARGET]["abi"] = json!("musl");
            catalog["targets"][self_update::BUILD_TARGET]
                .as_object_mut()
                .unwrap()
                .remove("minimum_libc");
        }
        if failure == "library" {
            catalog["targets"][self_update::BUILD_TARGET]["required_libraries"] =
                json!(["libextra.so.1"]);
        } else if failure == "foreign-loader" {
            catalog["targets"][self_update::BUILD_TARGET]["required_libraries"] =
                json!([if platform::host().unwrap() == "arm64" {
                    "ld-linux-x86-64.so.2"
                } else {
                    "ld-linux-aarch64.so.1"
                }]);
        }
        fs::write(&f.catalog, catalog.to_string()).unwrap();
        let out = f.seed().arg("update").output().unwrap();
        assert!(!out.status.success(), "{failure}");
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            message.contains(match failure {
                "version" => "version does not match",
                "elf" => "artifact CPU",
                "musl" => "libc/ABI",
                _ => "outside the validated GNU runtime baseline",
            }),
            "{message}"
        );
        assert_eq!(f.current(), "1.0.0");
        assert_eq!(
            download::digest(&f.root().join("apps/scoop/1.0.0/scoop")).unwrap(),
            download::digest(&one).unwrap()
        );
        let out = f
            .cli(&f.root().join("bin/scoop"))
            .arg("--version")
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"scoop 1.0.0\n");
    }
}
