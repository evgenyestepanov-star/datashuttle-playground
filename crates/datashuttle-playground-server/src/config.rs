//! Server configuration. Read once at startup from environment variables
//! so the binary stays a single drop-in container without a YAML file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub manifest_path: Option<PathBuf>,
    pub auth_token: Option<String>,
    pub session_ttl: Duration,
    pub session_quota_per_day: u32,
}

impl Config {
    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }
}

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8081";
const DEFAULT_TTL_SECS: u64 = 2 * 60 * 60;
const DEFAULT_QUOTA: u32 = 20;

pub fn load() -> anyhow::Result<Config> {
    let bind_addr =
        std::env::var("PLAYGROUND_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.into());

    let manifest_path = std::env::var("PLAYGROUND_MANIFEST").ok().map(PathBuf::from);

    let auth_token = std::env::var("PLAYGROUND_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());

    let session_ttl = match std::env::var("PLAYGROUND_TTL_SECS") {
        Ok(v) => Duration::from_secs(v.parse().context("PLAYGROUND_TTL_SECS must be a u64")?),
        Err(_) => Duration::from_secs(DEFAULT_TTL_SECS),
    };

    let session_quota_per_day = match std::env::var("PLAYGROUND_QUOTA_PER_DAY") {
        Ok(v) => v
            .parse()
            .context("PLAYGROUND_QUOTA_PER_DAY must be a u32")?,
        Err(_) => DEFAULT_QUOTA,
    };

    Ok(Config {
        bind_addr,
        manifest_path,
        auth_token,
        session_ttl,
        session_quota_per_day,
    })
}
