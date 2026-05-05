//! Minimal HTTP surface for the standalone playground server.
//!
//! The full handler suite (session lifecycle, action execution, scenario
//! orchestration) lives inside OSS api-core today and depends on the
//! private DataShuttle stack. Phase 5.B will introduce a public
//! extension point in OSS so those handlers can be ported here without
//! pulling in private internals.
//!
//! Until then this binary exposes:
//!   * `GET /health`           — liveness probe (always 200).
//!   * `GET /api/v1/playground/manifest` — reads the configured manifest
//!     and returns the validated, parsed scenario list.
//!   * `GET /metrics`          — prometheus exposition for the
//!     playground metric bundle.
//!
//! Authenticated requests must carry `Authorization: Bearer <token>`
//! when `PLAYGROUND_TOKEN` is set; unset means dev mode (no auth).

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
use prometheus::{Encoder, Registry, TextEncoder};
use serde::Serialize;

use crate::config::Config;

pub struct ServerState {
    pub config: Config,
    pub manifest: Option<Arc<Manifest>>,
    // Phase 5.B will wire these into the full session-lifecycle handlers
    // currently still living inside OSS api-core. Holding them on
    // `ServerState` now keeps the binary's wiring stable across the
    // cutover.
    #[allow(dead_code)]
    pub sessions: Option<Arc<SessionManager>>,
    #[allow(dead_code)]
    pub quota: Arc<PlaygroundQuotaTracker>,
    #[allow(dead_code)]
    pub metrics: Arc<PlaygroundMetrics>,
    pub prom_registry: Arc<Registry>,
}

pub fn router(state: Arc<ServerState>) -> Router {
    let api_v1 = Router::new()
        .route("/playground/manifest", get(get_manifest))
        .route("/playground/health", get(health));

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1", api_v1);

    let auth_state = state.clone();
    app.layer(middleware::from_fn(move |req, next| {
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

async fn get_manifest(State(state): State<Arc<ServerState>>) -> Response {
    match state.manifest.as_deref() {
        Some(m) => Json(m).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "playground manifest not loaded — set PLAYGROUND_MANIFEST or mount one at /opt/datashuttle/examples/manifest.json"
            })),
        )
            .into_response(),
    }
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
    // Health and metrics are unauthenticated — they're standard
    // probe surfaces that ops dashboards/load balancers must reach
    // without credentials.
    if path == "/health" || path == "/metrics" || path == "/api/v1/playground/health" {
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
