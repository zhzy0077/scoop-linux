use anyhow::Result;
use reqwest::{
    Url,
    blocking::{Client, RequestBuilder},
    header::{AUTHORIZATION, HeaderValue},
};
use std::process::{Command, Stdio};

fn github_asset(url: &Url) -> bool {
    if url.scheme() != "https" || url.port_or_known_default() != Some(443) {
        return false;
    }
    let parts: Vec<_> = url.path().split('/').collect();
    match url.host_str() {
        Some("github.com") => {
            parts.len() >= 7
                && !parts[1].is_empty()
                && !parts[2].is_empty()
                && parts[3] == "releases"
                && (parts[4] == "download" || (parts[4] == "latest" && parts[5] == "download"))
        }
        Some("api.github.com") => {
            parts.len() == 7
                && parts[1] == "repos"
                && !parts[2].is_empty()
                && !parts[3].is_empty()
                && parts[4] == "releases"
                && parts[5] == "assets"
                && parts[6].parse::<u64>().is_ok()
        }
        _ => false,
    }
}

fn gh_authorization(command: &mut Command) -> Option<HeaderValue> {
    let output = command
        .args(["auth", "token", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = std::str::from_utf8(&output.stdout).ok()?.trim();
    if token.is_empty() {
        return None;
    }
    let mut value = HeaderValue::from_str(&format!("Bearer {token}")).ok()?;
    value.set_sensitive(true);
    Some(value)
}

pub fn asset_request(client: &Client, url: Url) -> RequestBuilder {
    let authenticated = github_asset(&url);
    let request = client.get(url);
    if authenticated && let Some(value) = gh_authorization(&mut Command::new("gh")) {
        request.header(AUTHORIZATION, value)
    } else {
        request
    }
}

pub fn client(timeout_seconds: u64) -> Result<reqwest::blocking::Client> {
    let builder = reqwest::blocking::Client::builder();
    #[cfg(feature = "test-hooks")]
    let builder = if let Some(path) = std::env::var_os("SCOOP_TEST_CA_CERT") {
        builder.add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?)
    } else {
        builder
    };
    Ok(builder
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10
                || (attempt.url().scheme() == "http"
                    && attempt.previous().iter().any(|url| url.scheme() == "https"))
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .timeout(std::time::Duration::from_secs(timeout_seconds))
        .user_agent(concat!("scoop-linux/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn authentication_is_scoped_to_github_release_assets() {
        for url in [
            "https://github.com/owner/repo/releases/download/v1/app.tar.gz",
            "https://github.com/owner/repo/releases/latest/download/app.tar.gz",
            "https://api.github.com/repos/owner/repo/releases/assets/123",
        ] {
            assert!(github_asset(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "http://github.com/owner/repo/releases/download/v1/app.tar.gz",
            "https://github.com:8443/owner/repo/releases/download/v1/app.tar.gz",
            "https://github.com/owner/repo/archive/v1.tar.gz",
            "https://github.com/owner/repo/releases/tag/v1",
            "https://github.com.evil.test/owner/repo/releases/download/v1/app.tar.gz",
            "https://release-assets.githubusercontent.com/asset",
            "https://api.github.com/repos/owner/repo/releases/assets",
        ] {
            assert!(!github_asset(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn gh_token_lookup_handles_missing_failed_and_invalid_credentials() {
        assert!(gh_authorization(&mut Command::new("/nonexistent/scoop-test-gh")).is_none());
        for script in ["exit 1", "printf ''", "printf 'bad\\ntoken'"] {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            assert!(gh_authorization(&mut command).is_none());
        }
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "test \"$*\" = 'auth token --hostname github.com' && printf 'test-token\\n'",
            "gh",
        ]);
        let header = gh_authorization(&mut command).unwrap();
        assert_eq!(header, "Bearer test-token");
        assert!(header.is_sensitive());
    }

    #[test]
    fn authorization_is_removed_when_redirecting_to_another_origin() {
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin_addr = origin.local_addr().unwrap();
        let target_addr = target.local_addr().unwrap();
        let server = thread::spawn(move || {
            let read_request = |stream: &mut std::net::TcpStream| {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut data = Vec::new();
                while !data.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                String::from_utf8(data).unwrap().to_ascii_lowercase()
            };
            let (mut stream, _) = origin.accept().unwrap();
            assert!(read_request(&mut stream).contains("authorization: bearer test-token"));
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/asset\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            let (mut stream, _) = target.accept().unwrap();
            let request = read_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            request
        });
        let response = client(5)
            .unwrap()
            .get(format!("http://{origin_addr}/download"))
            .header(AUTHORIZATION, "Bearer test-token")
            .send()
            .unwrap();
        assert_eq!(response.text().unwrap(), "ok");
        assert!(!server.join().unwrap().contains("authorization:"));
    }
}
