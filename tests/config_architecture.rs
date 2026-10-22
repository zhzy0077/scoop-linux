use std::{fs, process::Command};
#[test]
fn configuration_cannot_enable_foreign_activation_and_explicit_native_wins() {
    let temp = tempfile::tempdir().unwrap();
    let config_home = temp.path().join("config");
    fs::create_dir_all(config_home.join("scoop")).unwrap();
    let host = scoop_linux::platform::host().unwrap();
    let foreign = if host == "arm64" { "64bit" } else { "arm64" };
    fs::write(
        config_home.join("scoop/config.json"),
        format!(r#"{{"default_architecture":"{foreign}"}}"#),
    )
    .unwrap();
    let path = temp.path().join("tool.json");
    let artifact = temp.path().join("artifact");
    fs::write(&artifact, b"architecture-independent data").unwrap();
    fs::write(&path, serde_json::json!({"version":"1","homepage":"https://example.org","license":"MIT","url":reqwest::Url::from_file_path(&artifact).unwrap().as_str(),"hash":scoop_linux::download::digest(&artifact).unwrap(),"architecture":{host.clone():{},foreign:{}}}).to_string()).unwrap();
    let run = |arch: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scoop"));
        command
            .arg("install")
            .arg(&path)
            .env("HOME", temp.path())
            .env("SCOOP", temp.path().join("root"))
            .env("XDG_CONFIG_HOME", &config_home);
        if let Some(arch) = arch {
            command.args(["--arch", arch]);
        }
        command.output().unwrap()
    };
    let output = run(None);
    assert!(String::from_utf8_lossy(&output.stderr).contains("foreign architecture"));
    let output = run(Some(&host));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp.path().join("root/apps/tool/current").exists());
}
