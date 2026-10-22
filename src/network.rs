use anyhow::Result;
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
