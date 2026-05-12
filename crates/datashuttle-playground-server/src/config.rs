//! Server configuration. Read once at startup from environment variables
//! so the binary stays a single drop-in container without a YAML file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;

use crate::api_client::DEFAULT_API_TIMEOUT_SECS;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub manifest_path: Option<PathBuf>,
    pub auth_token: Option<String>,
    pub session_ttl: Duration,
    pub session_quota_per_day: u32,
    /// OSS api base URL for shuttle/SQL callbacks (e.g.
    /// `http://api:8080`). When unset, handlers that need an
    /// ApiClient return 503. Read from `PLAYGROUND_API_BASE_URL`.
    pub api_base_url: Option<String>,
    /// Service token paired with `X-Datashuttle-Impersonate-User-Id`
    /// when calling the OSS api. Distinct from `auth_token` (which
    /// protects the inbound direction). Read from
    /// `PLAYGROUND_SERVICE_TOKEN`.
    pub api_service_token: Option<String>,
    /// Per-callback timeout in seconds. Defaults to
    /// [`DEFAULT_API_TIMEOUT_SECS`]; override via
    /// `PLAYGROUND_API_TIMEOUT_SECS`.
    pub api_timeout: Duration,
    /// Deployment classification — drives manifest visibility filters
    /// (cloud-only scenarios, etc.). Read from
    /// `PLAYGROUND_DEPLOYMENT`; defaults to `cloud` since this binary
    /// is the cloud distribution.
    pub deployment_kind: String,
    /// Filesystem root for scenario asset reads (`init_sql`,
    /// `shuttle_sql`, `payload_file`, …). Resolved against the
    /// container image layout. Override via
    /// `PLAYGROUND_EXAMPLES_DIR`.
    pub examples_dir: PathBuf,
}

impl Config {
    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }
}

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8081";
const DEFAULT_TTL_SECS: u64 = 2 * 60 * 60;
const DEFAULT_QUOTA: u32 = 20;
const DEFAULT_DEPLOYMENT_KIND: &str = "cloud";
const DEFAULT_EXAMPLES_DIR: &str = "/opt/datashuttle/examples";

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

    let api_base_url = std::env::var("PLAYGROUND_API_BASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let api_service_token = std::env::var("PLAYGROUND_SERVICE_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let api_timeout = match std::env::var("PLAYGROUND_API_TIMEOUT_SECS") {
        Ok(v) => Duration::from_secs(
            v.parse()
                .context("PLAYGROUND_API_TIMEOUT_SECS must be a u64")?,
        ),
        Err(_) => Duration::from_secs(DEFAULT_API_TIMEOUT_SECS),
    };

    let deployment_kind = std::env::var("PLAYGROUND_DEPLOYMENT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_DEPLOYMENT_KIND.to_string());

    let examples_dir = std::env::var("PLAYGROUND_EXAMPLES_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_EXAMPLES_DIR));

    Ok(Config {
        bind_addr,
        manifest_path,
        auth_token,
        session_ttl,
        session_quota_per_day,
        api_base_url,
        api_service_token,
        api_timeout,
        deployment_kind,
        examples_dir,
    })
}
