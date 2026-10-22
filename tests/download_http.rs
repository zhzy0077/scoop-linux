use scoop_linux::{
    config::Config,
    download::{self, Options},
    manifest::Manifest,
};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    request.push_str(&line);
                }
                log.lock().unwrap().push(request.clone());
                let (status, extra, body) = if request.starts_with("GET /redirect ") {
                    ("302 Found", "Location: /cookie\r\n", "")
                } else if request.starts_with("GET /cookie ")
                    && request
                        .to_ascii_lowercase()
                        .contains("cookie: token=secret")
                {
                    ("200 OK", "", "verified body")
                } else if request.starts_with("GET /truncated ") {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort"
                    )
                    .unwrap();
                    continue;
                } else {
                    ("403 Forbidden", "", "denied")
                };
                write!(stream,"HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        Self {
            base,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
#[test]
fn http_cookie_redirect_cache_force_skip_hash_and_failed_atomic_publication() {
    let server = Server::new();
    let temp = tempfile::tempdir().unwrap();
    let body = temp.path().join("body");
    fs::write(&body, b"verified body").unwrap();
    let config = Config {
        path: temp.path().join("config"),
        root: temp.path().join("root"),
        cache: temp.path().join("separate-cache"),
        values: Default::default(),
    };
    let make = |endpoint: &str, hash: String| {
        Manifest::parse(json!({"version":"1","homepage":"https://example.org","license":"MIT","url":format!("{}{endpoint}#/renamed.bin",server.base),"hash":hash,"cookie":{"token":"secret"}}).to_string()).unwrap()
    };
    let m = make("/redirect", download::digest(&body).unwrap());
    let target = m.target("arm64", false).unwrap();
    let file = download::fetch(
        &config,
        "fixture",
        "1",
        &target.artifacts[0],
        &m,
        Options::default(),
    )
    .unwrap();
    assert_eq!(file.filename, "renamed.bin");
    let cache = file.path.clone();
    assert_eq!(fs::read(&cache).unwrap(), b"verified body");
    let before = server.requests.lock().unwrap().len();
    download::fetch(
        &config,
        "fixture",
        "1",
        &target.artifacts[0],
        &m,
        Options::default(),
    )
    .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), before);
    download::fetch(
        &config,
        "fixture",
        "1",
        &target.artifacts[0],
        &m,
        Options {
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), before + 2);
    fs::write(&cache, b"corrupt").unwrap();
    download::fetch(
        &config,
        "fixture",
        "1",
        &target.artifacts[0],
        &m,
        Options::default(),
    )
    .unwrap();
    assert_eq!(fs::read(&cache).unwrap(), b"verified body");
    let count = fs::read_dir(&config.cache).unwrap().count();
    let temporary = download::fetch(
        &config,
        "uncached",
        "1",
        &target.artifacts[0],
        &m,
        Options {
            no_cache: true,
            ..Default::default()
        },
    )
    .unwrap();
    let path = temporary.path.clone();
    assert!(path.exists());
    drop(temporary);
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&config.cache).unwrap().count(), count);
    for endpoint in ["/cookie", "/truncated", "/403"] {
        let bad = make(endpoint, "b".repeat(64));
        let target = bad.target("arm64", false).unwrap();
        assert!(
            download::fetch(
                &config,
                "bad",
                "1",
                &target.artifacts[0],
                &bad,
                Options::default()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&config.cache).unwrap().count(), count);
    }
    let bad = make("/cookie", "b".repeat(64));
    let target = bad.target("arm64", false).unwrap();
    let skipped = download::fetch(
        &config,
        "skip",
        "1",
        &target.artifacts[0],
        &bad,
        Options {
            skip_hash: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut data = String::new();
    fs::File::open(skipped.path)
        .unwrap()
        .read_to_string(&mut data)
        .unwrap();
    assert_eq!(data, "verified body");
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| !r.contains("#/"))
    );
}
