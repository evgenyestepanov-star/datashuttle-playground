//! HTTP client for callbacks to the OSS api.
//!
//! Used by handlers that need to perform shuttle-runtime operations
//! (`/api/v1/sql`, `/api/v1/catalog/...`) — those endpoints live in the
//! api process and aren't reachable from this binary directly.
//!
//! Authentication: the playground holds a long-lived service token
//! (`PLAYGROUND_SERVICE_TOKEN`) which it presents as
//! `Authorization: Bearer ...`. The api accepts that bearer ONLY when
//! paired with `X-Datashuttle-Impersonate-User-Id` (and optional
//! `X-Datashuttle-Impersonate-Tenant-Id`) and constructs an
//! `AuthContext` for the impersonated identity — see
//! `datashuttle-api-core::auth::auth_middleware`.
//!
//! Construction is fallible-by-design: when either `api_base_url` or
//! `service_token` is unset in config, `ApiClient::new_from_config`
//! returns `None` and the handlers that need it return 503. This lets
//! a partial deploy (e.g. no api integration yet) still boot and
//! serve `/health` / `/metrics`.

use std::time::Duration;

use reqwest::Client as HttpClient;
use serde_json::Value;
use thiserror::Error;

use crate::identity::Identity;

/// Default per-request timeout. Matches the OSS-side
/// `playground.timeout_secs` default so a hung api call doesn't pin a
/// playground worker indefinitely. Configurable via
/// `PLAYGROUND_API_TIMEOUT_SECS`.
pub const DEFAULT_API_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Error)]
pub enum ApiCallError {
    #[error("api build request: {0}")]
    Build(String),
    #[error("api transport: {0}")]
    Transport(String),
    #[error("api {method} {url_path}: HTTP {status}: {body}")]
    Status {
        method: String,
        url_path: String,
        status: u16,
        body: String,
    },
    #[error("api decode body: {0}")]
    Decode(String),
}

impl ApiCallError {
    /// Convenience: build a [`Self::Build`] with the call coordinates
    /// baked into the message.
    fn build(method: &str, path: &str, err: impl ToString) -> Self {
        Self::Build(format!("{method} {path}: {}", err.to_string()))
    }

    /// Map a `reqwest` send error.
    fn transport(method: &str, path: &str, err: impl ToString) -> Self {
        Self::Transport(format!("{method} {path}: {}", err.to_string()))
    }

    /// HTTP status code if this error is a 4xx/5xx response. Kept
    /// for the (forthcoming) action-error metric label that needs to
    /// distinguish transport failures from upstream-rejected calls.
    #[allow(dead_code)]
    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Best-effort body extraction (only set on `Status`). Used by
    /// tests today; carried forward for handlers that want to surface
    /// the upstream body verbatim in session events.
    #[allow(dead_code)]
    pub fn body_snippet(&self) -> Option<&str> {
        match self {
            Self::Status { body, .. } => Some(body.as_str()),
            _ => None,
        }
    }
}

/// HTTP client for callbacks back into the OSS api.
///
/// Cheap to clone (`Arc<reqwest::Client>` under the hood). One
/// instance is built at boot and shared across all handlers via
/// `ServerState`.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    service_token: String,
    http: HttpClient,
}

impl ApiClient {
    /// Construct directly. `base_url` should be the api base
    /// (e.g. `http://api:8080`), without trailing slash.
    pub fn new(base_url: String, service_token: String, timeout: Duration) -> anyhow::Result<Self> {
        let http = HttpClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| anyhow::anyhow!("build reqwest client: {e}"))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            service_token,
            http,
        })
    }

    /// POST `/api/v1/sql` with the user's effective identity carried
    /// via impersonation headers. Returns the response body as JSON on
    /// success; on HTTP >= 400 returns [`ApiCallError::Status`] so
    /// callers can record the failure as a session event.
    pub async fn exec_sql(&self, identity: &Identity, sql: &str) -> Result<Value, ApiCallError> {
        let body = serde_json::json!({ "sql": sql });
        self.request("POST", "/api/v1/sql", Some(body), identity)
            .await
            .map(|(_, v)| v)
    }

    /// General-purpose call. Method + path; `body` is JSON-serialised
    /// when `Some`. Returns `(status, body_as_json_value)` on success.
    /// Non-JSON bodies are wrapped in `Value::String`.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        identity: &Identity,
    ) -> Result<(u16, Value), ApiCallError> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| ApiCallError::build(method, path, e))?;
        let mut req = self
            .http
            .request(m, &url)
            .bearer_auth(&self.service_token)
            .header("X-Datashuttle-Impersonate-User-Id", &identity.user_id);
        if let Some(tid) = identity.tenant_id.as_deref() {
            req = req.header("X-Datashuttle-Impersonate-Tenant-Id", tid);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ApiCallError::transport(method, path, e))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| ApiCallError::Decode(format!("{method} {path}: {e}")))?;
        let body_value: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if status >= 400 {
            let snippet = match &body_value {
                Value::String(s) => s.chars().take(400).collect::<String>(),
                other => serde_json::to_string(other)
                    .unwrap_or_default()
                    .chars()
                    .take(400)
                    .collect::<String>(),
            };
            return Err(ApiCallError::Status {
                method: method.to_string(),
                url_path: path.to_string(),
                status,
                body: snippet,
            });
        }
        Ok((status, body_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_is_trimmed() {
        let c = ApiClient::new(
            "http://api:8080/".into(),
            "svc".into(),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(c.base_url, "http://api:8080");
    }

    #[test]
    fn error_body_snippet_only_set_on_status() {
        let s = ApiCallError::Status {
            method: "POST".into(),
            url_path: "/x".into(),
            status: 500,
            body: "boom".into(),
        };
        assert_eq!(s.body_snippet(), Some("boom"));
        assert_eq!(s.http_status(), Some(500));

        let t = ApiCallError::Transport("net dead".into());
        assert!(t.body_snippet().is_none());
        assert!(t.http_status().is_none());
    }
}
