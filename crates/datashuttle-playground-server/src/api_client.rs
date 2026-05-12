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

    /// Execute one DataShuttle SQL statement against the OSS api.
    ///
    /// Pre-Phase-4.A the OSS api shipped a generic dispatcher at
    /// `POST /api/v1/sql` that parsed the statement and routed it to
    /// the right typed handler. That endpoint was retired in commit
    /// 9e925433 (#1032 "Delete crates/datashuttle-api/src/query/"),
    /// so we now do the same dispatch client-side and post each
    /// statement to its typed handler. Supports the statement set the
    /// playground's shuttle.sql templates emit:
    ///
    ///   * `CREATE CONNECTION ...`   → POST /api/v1/connections
    ///   * `CREATE SHUTTLE ...`      → POST /api/v1/shuttles
    ///   * `RESUME SHUTTLE <name>`   → POST /api/v1/shuttles/<name>/resume
    ///   * `PAUSE SHUTTLE <name>`    → POST /api/v1/shuttles/<name>/pause
    ///   * `DROP SHUTTLE <name>`     → DELETE /api/v1/shuttles/<name>
    ///   * `DROP CONNECTION <name>`  → DELETE /api/v1/connections/<name>
    ///
    /// Anything else returns a structured `ApiCallError::Status` with
    /// a 400-equivalent that says "unsupported playground SQL".
    pub async fn exec_sql(&self, identity: &Identity, sql: &str) -> Result<Value, ApiCallError> {
        match classify_statement(sql) {
            Some(Dispatch::CreateConnection) => {
                let body = serde_json::json!({ "sql": sql });
                self.request("POST", "/api/v1/connections", Some(body), identity)
                    .await
                    .map(|(_, v)| v)
            }
            Some(Dispatch::CreateShuttle) => {
                let body = serde_json::json!({ "sql": sql });
                self.request("POST", "/api/v1/shuttles", Some(body), identity)
                    .await
                    .map(|(_, v)| v)
            }
            Some(Dispatch::ResumeShuttle(name)) => {
                let path = format!("/api/v1/shuttles/{name}/resume");
                self.request("POST", &path, None, identity)
                    .await
                    .map(|(_, v)| v)
            }
            Some(Dispatch::PauseShuttle(name)) => {
                let path = format!("/api/v1/shuttles/{name}/pause");
                self.request("POST", &path, None, identity)
                    .await
                    .map(|(_, v)| v)
            }
            Some(Dispatch::DropShuttle(name)) => {
                let path = format!("/api/v1/shuttles/{name}");
                self.request("DELETE", &path, None, identity)
                    .await
                    .map(|(_, v)| v)
            }
            Some(Dispatch::DropConnection(name)) => {
                let path = format!("/api/v1/connections/{name}");
                self.request("DELETE", &path, None, identity)
                    .await
                    .map(|(_, v)| v)
            }
            None => Err(ApiCallError::Status {
                method: "POST".to_string(),
                url_path: "/api/v1/sql".to_string(),
                status: 400,
                body: format!(
                    "unsupported playground SQL — only CREATE/DROP \
                     CONNECTION + CREATE/RESUME/PAUSE/DROP SHUTTLE are \
                     dispatched. Statement: {}",
                    first_chars(sql, 120)
                ),
            }),
        }
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

/// Statement classes the playground SQL templates emit. Determines
/// which typed api endpoint a `exec_sql` call lands on.
enum Dispatch {
    CreateConnection,
    CreateShuttle,
    ResumeShuttle(String),
    PauseShuttle(String),
    DropShuttle(String),
    DropConnection(String),
}

/// Strip line comments + leading whitespace, uppercase the leading
/// tokens, and pattern-match the playground statement shapes. We
/// don't pull in `datashuttle-core::sql::parser` here — that would
/// drag the whole core crate (DataFusion / Arrow chain) into the
/// playground server's dep graph. The template SQL we author is a
/// small enough surface that hand-rolled prefix matching is fine.
fn classify_statement(sql: &str) -> Option<Dispatch> {
    let stripped = strip_sql_comments(sql);
    let trimmed = stripped.trim().trim_end_matches(';').trim();
    let upper = trimmed.to_ascii_uppercase();

    if upper.starts_with("CREATE CONNECTION") {
        return Some(Dispatch::CreateConnection);
    }
    if upper.starts_with("CREATE SHUTTLE") {
        return Some(Dispatch::CreateShuttle);
    }
    if let Some(rest) = upper.strip_prefix("RESUME SHUTTLE") {
        return extract_name(rest, trimmed, "RESUME SHUTTLE").map(Dispatch::ResumeShuttle);
    }
    if let Some(rest) = upper.strip_prefix("PAUSE SHUTTLE") {
        return extract_name(rest, trimmed, "PAUSE SHUTTLE").map(Dispatch::PauseShuttle);
    }
    if let Some(rest) = upper.strip_prefix("DROP SHUTTLE") {
        let rest = rest.strip_prefix(" IF EXISTS").unwrap_or(rest);
        return extract_name(rest, trimmed, "DROP SHUTTLE").map(Dispatch::DropShuttle);
    }
    if let Some(rest) = upper.strip_prefix("DROP CONNECTION") {
        let rest = rest.strip_prefix(" IF EXISTS").unwrap_or(rest);
        return extract_name(rest, trimmed, "DROP CONNECTION").map(Dispatch::DropConnection);
    }
    None
}

/// Extract the (single) identifier following a keyword. `upper_rest`
/// is the uppercased remainder; `original` is the original-cased SQL
/// so the returned name preserves the user's casing. `keyword_len`
/// is the byte length of the leading keyword pair so we can index
/// `original` correctly.
fn extract_name(upper_rest: &str, original: &str, keyword: &str) -> Option<String> {
    // Find where the identifier starts in the original (case-preserving)
    // SQL by re-skipping the keyword + the same number of leading
    // whitespace chars `upper_rest` was trimmed to.
    let start = original.len() - upper_rest.trim_start().len();
    let after_kw = &original[start..];
    let name = after_kw
        .split(|c: char| c.is_whitespace() || c == ';' || c == ',' || c == '(')
        .next()?
        .trim();
    if name.is_empty() {
        return None;
    }
    let _ = keyword;
    Some(name.to_string())
}

/// Strip SQL line comments (`-- foo`) but leave string-literal
/// `--` alone (none in our templates, but the right thing to do).
/// Block comments aren't used by playground templates.
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    for line in sql.lines() {
        let trimmed = match line.find("--") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
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

    #[test]
    fn classify_create_connection() {
        let sql = "CREATE CONNECTION IF NOT EXISTS my_pg TYPE POSTGRES WITH (host = 'h');";
        assert!(matches!(
            classify_statement(sql),
            Some(Dispatch::CreateConnection)
        ));
    }

    #[test]
    fn classify_create_shuttle_multiline_with_comments() {
        let sql = "-- preamble\nCREATE SHUTTLE IF NOT EXISTS s SOURCE c TABLES ('t');";
        assert!(matches!(
            classify_statement(sql),
            Some(Dispatch::CreateShuttle)
        ));
    }

    #[test]
    fn classify_resume_shuttle_name() {
        let sql = "RESUME SHUTTLE my_shuttle_42;";
        match classify_statement(sql) {
            Some(Dispatch::ResumeShuttle(name)) => assert_eq!(name, "my_shuttle_42"),
            other => panic!("expected ResumeShuttle, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn classify_drop_shuttle_if_exists() {
        let sql = "DROP SHUTTLE IF EXISTS old_pipe;";
        match classify_statement(sql) {
            Some(Dispatch::DropShuttle(name)) => assert_eq!(name, "old_pipe"),
            other => panic!("expected DropShuttle, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn classify_pause_shuttle() {
        match classify_statement("PAUSE SHUTTLE foo") {
            Some(Dispatch::PauseShuttle(name)) => assert_eq!(name, "foo"),
            other => panic!("expected PauseShuttle, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn classify_drop_connection() {
        match classify_statement("DROP CONNECTION IF EXISTS conn1;") {
            Some(Dispatch::DropConnection(name)) => assert_eq!(name, "conn1"),
            other => panic!("expected DropConnection, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn classify_unsupported_returns_none() {
        assert!(classify_statement("SELECT 1").is_none());
        assert!(classify_statement("INSERT INTO t VALUES (1)").is_none());
    }

    #[test]
    fn strip_sql_comments_drops_line_comments() {
        let sql = "-- comment\nSELECT 1; -- inline\n";
        let out = strip_sql_comments(sql);
        assert!(!out.contains("comment"));
        assert!(!out.contains("inline"));
        assert!(out.contains("SELECT 1;"));
    }
}
