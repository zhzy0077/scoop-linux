use anyhow::{Result, bail};

pub fn normalize(value: &str) -> Result<String> {
    let arch = match value.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" | "64bit" => "64bit".into(),
        "i386" | "i486" | "i586" | "i686" | "x86" | "32bit" => "32bit".into(),
        "aarch64" | "arm64" => "arm64".into(),
        "arm" | "armv7" | "armv7l" | "armv6l" => "arm".into(),
        other => other.to_owned(),
    };
    if arch.is_empty()
        || !arch
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    {
        bail!("invalid architecture '{value}'");
    }
    Ok(arch)
}

pub fn host() -> Result<String> {
    normalize(std::env::consts::ARCH)
}

pub fn select(
    explicit: Option<&str>,
    configured: Option<&str>,
    native: &str,
    activation: bool,
) -> Result<String> {
    let selected = normalize(explicit.or(configured).unwrap_or(native))?;
    let native = normalize(native)?;
    if activation && selected != native {
        bail!("foreign architecture '{selected}' cannot be activated on '{native}'");
    }
    Ok(selected)
}
