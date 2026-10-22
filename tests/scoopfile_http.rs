use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
};
#[test]
fn http_import_continues_after_config_errors_and_rejects_architecture_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("artifact");
    fs::write(&artifact, "#!/bin/sh\nexit 0\n").unwrap();
    let manifest = temp.path().join("app.json");
    fs::write(&manifest,json!({"version":"1","homepage":"https://example.org","license":"MIT","url":format!("{}#/app",reqwest::Url::from_file_path(&artifact).unwrap()),"hash":scoop_linux::download::digest(&artifact).unwrap(),"bin":"app"}).to_string()).unwrap();
    let host = scoop_linux::platform::host().unwrap();
    let foreign = if host == "arm64" { "64bit" } else { "arm64" };
    let document=json!({"CONFIG":{"root_path":"/never-import-this","force_update":true},"BUCKETS":[],"APPS":[{"name":"conflict","source":manifest,"Info":host,"Architecture":foreign},{"name":"app","source":manifest}]}).to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).unwrap() > 0);
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            document.len(),
            document
        )
        .unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_scoop"))
        .args(["import", &format!("http://{address}/scoopfile.json")])
        .env("HOME", temp.path())
        .env("SCOOP", temp.path().join("root"))
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("machine-specific config"), "{error}");
    assert!(
        error.contains("conflicting Scoopfile architecture fields"),
        "{error}"
    );
    assert!(temp.path().join("root/bin/app").is_file());
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.path().join("config/scoop/config.json")).unwrap())
            .unwrap();
    assert_eq!(config["force_update"], true);
    assert!(config.get("root_path").is_none());
}
