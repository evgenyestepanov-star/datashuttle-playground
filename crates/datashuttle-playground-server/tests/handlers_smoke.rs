//! End-to-end wiring smoke for the session-lifecycle handlers.
//!
//! Boots an `axum::Router` with a `ServerState` carrying an in-memory
//! manifest, a `NoopPlaygroundDispatcher`, and no `ApiClient` —
//! exercises the create / get / delete cycle to confirm the routes,
//! middleware, and handler wiring all line up.
//!
//! The `in-memory-generator` scenario is used because it has no
//! `init_sql` / `shuttle_sql` and no `docker_service` source, so the
//! handler exercises pure session-manager paths without trying to
//! reach an api callback or a docker sidecar.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use datashuttle_playground::manifest::Manifest;
use datashuttle_playground::metrics::PlaygroundMetrics;
use datashuttle_playground::quota::PlaygroundQuotaTracker;
use datashuttle_playground::sessions::SessionManager;
use datashuttle_playground::tcp::{NoopPlaygroundDispatcher, PlaygroundDispatcher};
use datashuttle_playground_server::config::Config;
use datashuttle_playground_server::router::{router, ServerState};
use prometheus::Registry;
use tower::ServiceExt;

fn workspace_examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("examples")
}

fn test_state() -> Arc<ServerState> {
    let manifest_path = workspace_examples_dir().join("manifest.json");
    let manifest =
        Arc::new(Manifest::load(&manifest_path).expect("examples/manifest.json should parse"));

    // `in-memory-generator` is allowed on `dev` only.
    let config = Config {
        bind_addr: "127.0.0.1:0".into(),
        manifest_path: Some(manifest_path),
        auth_token: None,
        session_ttl: Duration::from_secs(60 * 60),
        session_quota_per_day: 100,
        api_base_url: None,
        api_service_token: None,
        api_timeout: Duration::from_secs(5),
        deployment_kind: "dev".into(),
        examples_dir: workspace_examples_dir(),
        data_dir: std::env::temp_dir().join("playground-smoke"),
    };

    let prom_registry = Arc::new(Registry::new());
    let metrics = Arc::new(
        PlaygroundMetrics::new(&prom_registry).expect("register metrics"),
    );

    let sessions = Some(SessionManager::new(
        manifest.clone(),
        true,
        config.session_ttl,
    ));
    let quota = Arc::new(PlaygroundQuotaTracker::with_limit(
        config.session_quota_per_day,
    ));

    let dispatcher: Arc<dyn PlaygroundDispatcher> = Arc::new(NoopPlaygroundDispatcher);

    Arc::new(ServerState {
        config,
        manifest: Some(manifest),
        sessions,
        quota,
        metrics,
        prom_registry,
        dispatcher,
        api_client: None,
    })
}

#[tokio::test]
async fn create_then_delete_session_wires_correctly() {
    let app = router(test_state());

    // POST /api/v1/sessions
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("Content-Type", "application/json")
        .header("X-Datashuttle-User-Id", "test-user-1")
        .header("X-Datashuttle-Auth-Method", "oidc")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "scenario_id": "in-memory-generator"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    let body_bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);
    assert_eq!(
        status,
        StatusCode::CREATED,
        "create returned {status}, body: {body}"
    );
    let session_id = body["id"]
        .as_str()
        .expect("session_id should be a string in response")
        .to_string();

    // DELETE /api/v1/sessions/:id
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/sessions/{session_id}"))
        .header("X-Datashuttle-User-Id", "test-user-1")
        .header("X-Datashuttle-Auth-Method", "oidc")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.expect("delete request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "delete should succeed; got {}",
        resp.status()
    );
}

#[tokio::test]
async fn list_sessions_requires_identity() {
    let app = router(test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/sessions")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_path_does_not_require_identity() {
    let app = router(test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn manifest_is_reachable_without_identity() {
    let app = router(test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/manifest")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 256 * 1024).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v["scenarios"].is_array(), "scenarios array should be present");
}
