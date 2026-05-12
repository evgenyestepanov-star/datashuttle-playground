//! HTTP surface for the standalone playground server.
//!
//! Phase 5.C wired in the full session-lifecycle handler suite ported
//! from OSS api-core (commit `c8959ae6^`). The binary now serves:
//!
//!   * `GET /health`                    — liveness probe (always 200).
//!   * `GET /metrics`                   — prometheus exposition.
//!   * `GET /api/v1/playground/health`  — unauthenticated probe.
//!   * `GET /api/v1/playground/manifest`— scenario list (unauth so
//!     the UI can render the marketing view; sessions still need auth).
//!   * `POST/GET/DELETE /api/v1/playground/sessions[/...]`
//!     — full session lifecycle.
//!   * `POST /api/v1/playground/sessions/:id/actions/:action_id`
//!     — execute one of the manifest's whitelisted actions.
//!
//! Auth layering: the inbound `Authorization: Bearer <PLAYGROUND_TOKEN>`
//! gate is the same for every protected path (handled by
//! [`auth_middleware`]). On top of that, [`identity_middleware`] reads
//! the `X-Datashuttle-*` headers the OSS reverse-proxy injects and
//! builds an [`Identity`](crate::identity::Identity) that handlers
//! downcast out of request extensions.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use datashuttle_playground::manifest::Manifest;
use datashuttle_playground::metrics::PlaygroundMetrics;
use datashuttle_playground::quota::PlaygroundQuotaTracker;
use datashuttle_playground::sessions::SessionManager;
use datashuttle_playground::tcp::PlaygroundDispatcher;
use prometheus::{Encoder, Registry, TextEncoder};
use serde::Serialize;

use crate::api_client::ApiClient;
use crate::config::Config;
use crate::handlers;
use crate::identity::identity_middleware;

pub struct ServerState {
    pub config: Config,
    /// Loaded manifest. Held on state so future routes (e.g. catalog
    /// of source connectors) can reach it without going through the
    /// session manager. Handlers today read it via `sessions.manifest()`.
    #[allow(dead_code)]
    pub manifest: Option<Arc<Manifest>>,
    pub sessions: Option<Arc<SessionManager>>,
    pub quota: Arc<PlaygroundQuotaTracker>,
    pub metrics: Arc<PlaygroundMetrics>,
    pub prom_registry: Arc<Registry>,
    /// Concrete source dispatcher (postgres + mysql TCP pools). Built
    /// once at boot; lazy-init internally so unused pools don't pay
    /// connection cost.
    pub dispatcher: Arc<dyn PlaygroundDispatcher>,
    /// HTTP client for callbacks to the OSS api (`/api/v1/sql` and
    /// catalog DELETE). `None` when the deploy hasn't set
    /// `PLAYGROUND_API_BASE_URL` / `PLAYGROUND_SERVICE_TOKEN`; handlers
    /// that need it return 503.
    pub api_client: Option<Arc<ApiClient>>,
}

pub fn router(state: Arc<ServerState>) -> Router {
    // Routes are mounted directly under `/api/v1`, NOT nested at
    // `/playground` — the OSS api's reverse-proxy strips the
    // `/api/v1/playground` prefix on inbound and forwards as
    // `/api/v1/<rest>` to this binary. So when a browser hits
    //
    //     POST https://api/api/v1/playground/sessions
    //
    // the standalone server sees
    //
    //     POST /api/v1/sessions
    //
    // and the route below has to match that flattened form. The
    // session-lifecycle handlers ported from OSS api-core kept their
    // original `/manifest`, `/sessions/...` etc. route paths from
    // `handlers::routes()`, so merging them directly under `/api/v1`
    // produces the right mapping. See
    // `datashuttle-api-core::handlers::playground_proxy::strip_playground_prefix`.
    let session_routes = handlers::routes();

    let api_v1 = Router::new()
        .route("/health", get(health))
        .merge(session_routes);

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1", api_v1);

    // Layer order (axum applies bottom-up; topmost listed runs first
    // on inbound):
    //   1. auth_middleware  — verify shared bearer.
    //   2. identity_middleware — extract Identity from headers.
    //   3. handlers — see Identity in extensions.
    let auth_state = state.clone();
    app.layer(middleware::from_fn(identity_middleware))
        .layer(middleware::from_fn(move |req, next| {
            let st = auth_state.clone();
            async move { auth_middleware(st, req, next).await }
        }))
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResp {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<HealthResp> {
    Json(HealthResp {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn metrics_handler(State(state): State<Arc<ServerState>>) -> Response {
    let metric_families = state.prom_registry.gather();
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("encoding metrics failed: {e}"),
        )
            .into_response();
    }
    (StatusCode::OK, buffer).into_response()
}

async fn auth_middleware(
    state: Arc<ServerState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    // Health and metrics are unauthenticated — standard probe
    // surfaces dashboards/load balancers must reach without
    // credentials. Manifest is also unauth so the public UI can
    // render the scenario list pre-login (`get_manifest` only
    // exposes scenario metadata, no tenant data).
    // Paths after the api proxy's `/api/v1/playground` strip — see
    // `datashuttle-api-core::handlers::playground_proxy::strip_playground_prefix`.
    if path == "/health"
        || path == "/metrics"
        || path == "/api/v1/health"
        || path == "/api/v1/manifest"
    {
        return next.run(req).await;
    }

    if let Some(expected) = state.config.auth_token.as_deref() {
        if !valid_bearer(req.headers(), expected) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "missing or invalid bearer token"
                })),
            )
                .into_response();
        }
    }
    next.run(req).await
}

fn valid_bearer(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|tok| constant_time_eq(tok.as_bytes(), expected.as_bytes()))
        .unwrap_or(false)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
