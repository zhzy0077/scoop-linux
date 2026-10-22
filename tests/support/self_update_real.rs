use super::*;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Stdio},
};
fn real_release() -> Option<(PathBuf, String)> {
    match std::env::var_os("SCOOP_REAL_RELEASE2") {
        Some(path) => {
            let path = PathBuf::from(path);
            let output = Command::new(&path).arg("--version").output().unwrap();
            assert!(output.status.success());
            let version = String::from_utf8(output.stdout)
                .unwrap()
                .split_whitespace()
                .last()
                .unwrap()
                .to_owned();
            assert_ne!(version, env!("CARGO_PKG_VERSION"));
            assert_ne!(
                download::digest(&path).unwrap(),
                download::digest(Path::new(env!("CARGO_BIN_EXE_scoop"))).unwrap()
            );
            Some((path, version))
        }
        None => {
            assert!(
                std::env::var_os("SCOOP_DISTRIBUTION_GATE").is_none(),
                "distribution gate requires an independent Rust release"
            );
            eprintln!(
                "Independent release tests run in tests/run-distribution.sh after tests/build-distribution.sh"
            );
            None
        }
    }
}
struct Server {
    child: Child,
    base: String,
    cert: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn new(f: &Fixture) -> Self {
        let cert = f.temp.path().join("cert.pem");
        let key = f.temp.path().join("key.pem");
        let o = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost,IP:127.0.0.1",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()
            .expect("openssl CLI is required for the HTTPS fixture; see README.md Native verification tools");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let script = r#"import functools,http.server,ssl,sys
class Handler(http.server.SimpleHTTPRequestHandler):
 def do_GET(self):
  if self.path=='/downgrade':
   self.send_response(302);self.send_header('Location','http://127.0.0.1:1/untrusted');self.end_headers()
  else: super().do_GET()
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Handler,directory=sys.argv[1]))
context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(sys.argv[2],sys.argv[3])
server.socket=context.wrap_socket(server.socket,server_side=True)
print(server.server_address[1],flush=True)
server.serve_forever()
"#;
        let mut child = Command::new("python3")
            .args(["-u", "-c", script])
            .arg(f.temp.path())
            .arg(&cert)
            .arg(key)
            .stdout(Stdio::piped())
            .stderr(fs::File::create(f.temp.path().join("https-requests.log")).unwrap())
            .spawn()
            .unwrap();
        let mut port = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut port)
            .unwrap();
        let port: u16 = port.trim().parse().unwrap();
        Self {
            child,
            base: format!("https://localhost:{port}"),
            cert,
        }
    }
    fn cli(&self, f: &Fixture, binary: &Path) -> Command {
        let mut c = f.cli(binary);
        c.env("SCOOP_TEST_CA_CERT", &self.cert);
        c
    }
}
#[test]
fn independent_rust_https_multitarget_status_hold_concurrency_and_handoff() {
    let Some((two, version)) = real_release() else {
        return;
    };
    let f = Fixture::new();
    f.publish(
        env!("CARGO_PKG_VERSION"),
        Path::new(env!("CARGO_BIN_EXE_scoop")),
    );
    f.bootstrap();
    fs::copy(&two, f.temp.path().join("release2")).unwrap();
    let server = Server::new(&f);
    f.publish(&version, &two);
    let mut catalog: Value = serde_json::from_slice(&fs::read(&f.catalog).unwrap()).unwrap();
    catalog["targets"][self_update::BUILD_TARGET]["url"] =
        json!(format!("{}/release2", server.base));
    let mut foreign = catalog["targets"][self_update::BUILD_TARGET].clone();
    foreign["architecture"] = json!(if platform::host().unwrap() == "arm64" {
        "64bit"
    } else {
        "arm64"
    });
    foreign["url"] = json!(format!("{}/never-foreign", server.base));
    let foreign_target = if platform::host().unwrap() == "arm64" {
        "x86_64-unknown-linux-gnu"
    } else {
        "aarch64-unknown-linux-gnu"
    };
    catalog["targets"][foreign_target] = foreign;
    let mut musl = catalog["targets"][self_update::BUILD_TARGET].clone();
    musl["libc"] = json!("musl-static");
    musl["abi"] = json!("musl");
    musl.as_object_mut().unwrap().remove("minimum_libc");
    musl["url"] = json!(format!("{}/never-musl", server.base));
    catalog["targets"][self_update::BUILD_TARGET.replace("-gnu", "-musl")] = musl;
    fs::write(&f.catalog, catalog.to_string()).unwrap();
    let catalog_url = format!("{}/catalog.json", server.base);
    assert!(
        f.seed()
            .args(["config", "release_catalog", &catalog_url])
            .status()
            .unwrap()
            .success()
    );
    // A production trust store rejects our private CA; test-hooks explicitly trusts it.
    let out = f
        .seed()
        .arg("--bootstrap")
        .arg(&catalog_url)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let path = f.temp.path().join("config/scoop/config.json");
    let before = fs::read(&path).unwrap();
    let out = server
        .cli(&f, &f.root().join("bin/scoop"))
        .arg("status")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Scoop is out of date"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.current(), env!("CARGO_PKG_VERSION"));
    assert_eq!(fs::read(&path).unwrap(), before);
    let requests = fs::read_to_string(f.temp.path().join("https-requests.log")).unwrap();
    assert!(!requests.contains("GET /release2"));
    assert!(f.seed().args(["hold", "scoop"]).status().unwrap().success());
    let out = server
        .cli(&f, &f.root().join("bin/scoop"))
        .arg("update")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(f.current(), env!("CARGO_PKG_VERSION"));
    assert!(
        f.seed()
            .args(["unhold", "scoop"])
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        f.source.join("bucket/data.json"),
        r#"{"version":"2","homepage":"https://example.org","license":"MIT"}"#,
    )
    .unwrap();
    git(&f.source, &["add", "."]).unwrap();
    git(
        &f.source,
        &["commit", "-qm", "second Rust observes this pull"],
    )
    .unwrap();
    let hook = f.root().join("buckets/main/.git/hooks/post-merge");
    fs::write(
        &hook,
        r#"#!/usr/bin/python3
import os,pathlib
pid=os.getppid()
while pid>1:
 try:
  executable=os.readlink(f'/proc/{pid}/exe')
  if '/apps/scoop/' in executable:
   pathlib.Path(os.environ['REFRESH_EXECUTION_LOG']).write_text(executable);break
  pid=int(pathlib.Path(f'/proc/{pid}/stat').read_text().split()[3])
 except (OSError,ValueError): break
"#,
    )
    .unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    let children: Vec<_> = (0..2)
        .map(|_| {
            server
                .cli(&f, Path::new(env!("CARGO_BIN_EXE_scoop")))
                .arg("update")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let o = child.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "{} {}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
    assert_eq!(f.current(), version);
    assert_eq!(
        download::digest(&f.root().join(format!("apps/scoop/{version}/scoop"))).unwrap(),
        download::digest(&two).unwrap()
    );
    assert!(
        f.root()
            .join(format!("apps/scoop/{}/scoop", env!("CARGO_PKG_VERSION")))
            .is_file()
    );
    let runner = fs::read_to_string(f.temp.path().join("refreshed")).unwrap();
    assert!(
        runner.ends_with(&format!("apps/scoop/{version}/scoop")),
        "{runner}"
    );
    let out = server
        .cli(&f, &f.root().join("bin/scoop"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("scoop {version}\n")
    );
    let requests = fs::read_to_string(f.temp.path().join("https-requests.log")).unwrap();
    assert!(requests.contains("GET /release2"));
    assert!(!requests.contains("GET /never-"));
    // An HTTPS catalog still cannot authorize plain-HTTP artifacts.
    catalog["version"] = json!("9.0.0");
    catalog["targets"][self_update::BUILD_TARGET]["url"] = json!("http://127.0.0.1:1/scoop");
    fs::write(f.temp.path().join("bad.json"), catalog.to_string()).unwrap();
    let out = server
        .cli(&f, Path::new(env!("CARGO_BIN_EXE_scoop")))
        .args(["--bootstrap", &format!("{}/bad.json", server.base)])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(f.current(), version);
    let out = server
        .cli(&f, Path::new(env!("CARGO_BIN_EXE_scoop")))
        .args(["--bootstrap", &format!("{}/downgrade", server.base)])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(f.current(), version);
}
#[test]
fn independent_rust_self_activation_recovers_every_applicable_boundary() {
    let Some((two, version)) = real_release() else {
        return;
    };
    for point in ["prepared", "published", "current", "links", "committed"] {
        let f = Fixture::new();
        f.publish(
            env!("CARGO_PKG_VERSION"),
            Path::new(env!("CARGO_BIN_EXE_scoop")),
        );
        f.bootstrap();
        f.publish(&version, &two);
        let o = f
            .seed()
            .arg("update")
            .env("SCOOP_TEST_FAILPOINT", point)
            .env("SCOOP_TEST_CRASH", "1")
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(86),
            "{point}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let o = f.seed().arg("--version").output().unwrap();
        assert!(o.status.success());
        assert_eq!(
            f.current(),
            if point == "committed" {
                version.as_str()
            } else {
                env!("CARGO_PKG_VERSION")
            }
        );
        assert_eq!(
            fs::read_dir(f.root().join(".transactions"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(fs::read_dir(f.root().join(".staging")).unwrap().count(), 0);

        // Execute the recovered stable entry point, not the unaffected seed.
        let stable = f.root().join("bin/scoop");
        let expected = if point == "committed" {
            version.as_str()
        } else {
            env!("CARGO_PKG_VERSION")
        };
        let recovered = f.cli(&stable).arg("--version").output().unwrap();
        assert!(
            recovered.status.success(),
            "{point}: recovered version command failed: {}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        assert_eq!(
            String::from_utf8(recovered.stdout).unwrap(),
            format!("scoop {expected}\n"),
            "{point}: recovered stable binary version"
        );
        let manifest = f.cli(&stable).args(["cat", "main/data"]).output().unwrap();
        assert!(
            manifest.status.success(),
            "{point}: recovered bucket command failed: {}",
            String::from_utf8_lossy(&manifest.stderr)
        );
        let manifest: Value = serde_json::from_slice(&manifest.stdout).unwrap();
        assert_eq!(manifest["version"], "1", "{point}: recovered bucket lookup");
        assert_eq!(manifest["homepage"], "https://example.org");
    }
}
