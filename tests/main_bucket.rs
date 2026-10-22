#[path = "support/main_default.rs"]
mod main_default;
use scoop_linux::manifest::Manifest;
use std::{collections::HashSet, fs, path::Path};
#[test]
fn main_bucket_has_ten_valid_exact_native_manifests() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("main/bucket");
    let mut names = HashSet::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let manifest = Manifest::parse(fs::read_to_string(&path).unwrap()).unwrap();
        assert!(names.insert(path.file_stem().unwrap().to_string_lossy().into_owned()));
        let name = path.file_stem().unwrap().to_str().unwrap();
        let expected = if name == "ripgrep" { "rg" } else { name };
        assert_eq!(manifest.value["architecture"].as_object().unwrap().len(), 2);
        for arch in ["64bit", "arm64"] {
            let selected = manifest.target(arch, false).unwrap();
            assert_eq!(selected.bins, [expected]);
            assert!(!selected.artifacts.is_empty());
            for artifact in selected.artifacts {
                assert_eq!(
                    reqwest::Url::parse(&artifact.url).unwrap().scheme(),
                    "https"
                );
                let hash = artifact.hash.unwrap();
                assert_eq!(hash.len(), 64);
                assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
            }
        }
        match name {
            "ripgrep" => assert_eq!(manifest.value["license"], "Unlicense|MIT"),
            "bat" | "fd" => assert_eq!(manifest.value["license"], "MIT|Apache-2.0"),
            _ => {}
        }
        assert!(manifest.target("riscv64", false).is_err());
    }
    assert_eq!(names.len(), 10);
}
