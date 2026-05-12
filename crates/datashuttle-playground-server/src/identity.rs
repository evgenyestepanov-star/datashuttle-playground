//! Identity extracted from the `X-Datashuttle-*` headers the OSS api
//! reverse-proxy injects.
//!
//! The proxy strips the user's original `Authorization` hop-by-hop and
//! replaces it with the playground shared bearer
//! (`PLAYGROUND_TOKEN`), so user identity has to come in via these
//! trusted headers. We trust them because only api can set them — see
//! `datashuttle-api-core::handlers::playground_proxy`, which strips any
//! inbound copies before forwarding.
//!
//! When a request comes in directly (not through the reverse-proxy)
//! these headers will be absent and the middleware returns 401 — the
//! standalone playground-server is internal-only by design.
//!
//! Public probe endpoints (`/health`, `/metrics`,
//! `/api/v1/playground/health`, `/api/v1/manifest`) skip
//! the check so dashboards and load balancers can reach them.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};

/// Header names sent by the OSS reverse-proxy. Lowercased for the
/// `HeaderMap::get` lookup which is case-insensitive but explicit
/// here keeps the contract visible.
pub const HEADER_USER_ID: &str = "x-datashuttle-user-id";
pub const HEADER_TENANT_ID: &str = "x-datashuttle-tenant-id";
pub const HEADER_ACTOR_ID: &str = "x-datashuttle-actor-id";
pub const HEADER_AUTH_METHOD: &str = "x-datashuttle-auth-method";

/// Authenticated identity forwarded by the OSS reverse-proxy.
///
/// Inserted into request extensions by [`identity_middleware`]. Every
/// session-lifecycle handler downcasts this out of extensions; missing
/// identity is treated as 401 by the middleware itself, never by the
/// handler.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Subject the request acts as. Used as the session-manager key
    /// for "one active session per user" and for audit attribution.
    pub user_id: String,
    /// Tenant the user belongs to. `None` on OSS / on-prem deployments
    /// where the api is single-tenant.
    pub tenant_id: Option<String>,
    /// Platform-admin who is impersonating, when set. Echoed into
    /// session events so audit logs can attribute actions to the
    /// admin rather than the impersonated user. Read by handlers
    /// once Phase 5.D wires the audit-attribution path; carried on
    /// `Identity` so the data is there when the wiring lands.
    #[allow(dead_code)]
    pub actor_id: Option<String>,
    /// `oidc` / `api_key` / `basic` / `local` — recorded for audit.
    /// Defaults to `"unknown"` when the header was absent (the
    /// reverse-proxy always sets it, but we don't crash on missing
    /// data).
    #[allow(dead_code)]
    pub auth_method: String,
}

/// Paths exempt from the identity check. Matches the
/// `auth_middleware` exemptions in `router.rs` — keep both lists in
/// sync.
fn is_exempt(path: &str) -> bool {
    // Paths reflect the post-strip layout the OSS api's reverse-proxy
    // produces: `/api/v1/playground/<x>` arrives here as `/api/v1/<x>`.
    matches!(
        path,
        "/health" | "/metrics" | "/api/v1/health" | "/api/v1/manifest"
    )
}

/// Axum middleware: extract `Identity` from the forwarded headers and
/// insert into request extensions, OR reject with 401.
pub async fn identity_middleware(mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if is_exempt(path) {
        return next.run(req).await;
    }

    let user_id = match req
        .headers()
        .get(HEADER_USER_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(uid) => uid,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "playground requires authenticated identity"
                })),
            )
                .into_response();
        }
    };

    let tenant_id = req
        .headers()
        .get(HEADER_TENANT_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let actor_id = req
        .headers()
        .get(HEADER_ACTOR_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let auth_method = req
        .headers()
        .get(HEADER_AUTH_METHOD)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let identity = Identity {
        user_id,
        tenant_id,
        actor_id,
        auth_method,
    };
    req.extensions_mut().insert(identity);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn probe(req: HttpRequest<Body>) -> Response {
        match req.extensions().get::<Identity>() {
            Some(id) => (
                StatusCode::OK,
                serde_json::json!({
                    "user_id": id.user_id,
                    "tenant_id": id.tenant_id,
                    "actor_id": id.actor_id,
                    "auth_method": id.auth_method,
                })
                .to_string(),
            )
                .into_response(),
            None => (StatusCode::OK, "null".to_string()).into_response(),
        }
    }

    fn test_app() -> Router {
        Router::new()
            .route("/health", get(probe))
            .route("/api/v1/manifest", get(probe))
            .route("/api/v1/sessions", get(probe))
            .layer(from_fn(identity_middleware))
    }

    #[tokio::test]
    async fn rejects_protected_path_without_user_header() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_protected_path_with_user_header() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/sessions")
                    .header("X-Datashuttle-User-Id", "user-1")
                    .header("X-Datashuttle-Tenant-Id", "tenant-1")
                    .header("X-Datashuttle-Auth-Method", "oidc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["user_id"], "user-1");
        assert_eq!(v["tenant_id"], "tenant-1");
        assert_eq!(v["auth_method"], "oidc");
    }

    #[tokio::test]
    async fn exempt_paths_skip_identity_check() {
        for path in ["/health", "/api/v1/manifest"] {
            let resp = test_app()
                .oneshot(
                    HttpRequest::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "exempt path {path} returned non-200"
            );
        }
    }
}
