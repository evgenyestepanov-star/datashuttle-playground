//! Playground session-lifecycle HTTP handlers.
//!
//! Ported from `datashuttle-api-core::playground::handlers` (verbatim
//! 2616-LOC source recovered before commit c8959ae6, which retired the
//! inline module). Translation summary:
//!
//! * `AuthContext` → [`crate::identity::Identity`]. The reverse-proxy
//!   strips the user's JWT hop-by-hop and replaces it with the
//!   playground shared bearer; identity rides in `X-Datashuttle-*`
//!   headers.
//! * `playground_runtime(&state)` → fields on
//!   [`crate::router::ServerState`].
//! * `loopback_post` / `loopback_url` →
//!   [`crate::api_client::ApiClient`]. Callbacks now go over the
//!   network with the service-token + impersonation headers.
//!
//! Endpoints under `/api/v1/playground/*`. Authenticated users pick a
//! scenario from the loaded manifest, the handler provisions a private
//! namespace, creates a shuttle bound to that namespace via the api
//! callback, and exposes the curated set of actions defined by the
//! manifest. Sessions carry a bounded TTL (default 2h) and a user is
//! limited to one active session at a time.
//!
//! The action whitelist is non-negotiable — free-form SQL is rejected.
//! Users can only invoke actions that were pre-defined in
//! `examples/manifest.json` by an operator.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use datashuttle_playground::manifest::{
    Action, ActionKind, ActionTarget, Deployment, HttpRequest, Scenario, Source,
};
use datashuttle_playground::metrics::{
    ActionErrorKind, ActionOutcomeKind, SessionStartOutcome, TeardownKind,
};
use datashuttle_playground::sessions::{
    Session, SessionError, SessionManager, SessionStatus, SessionView,
};
use datashuttle_playground::tcp::{
    is_safe_playground_shuttle_artifact, is_safe_resource_name, DispatchError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::api_client::{ApiCallError, ApiClient};
use crate::identity::Identity;
use crate::router::ServerState;

// --------------------------------------------------------------------- routes

/// Mount under `/api/v1/playground`. Wired in by `router.rs`.
pub fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/manifest", get(get_manifest))
        .route("/sessions", post(create_session).get(list_my_sessions))
        .route("/sessions/:id", get(get_session).delete(end_session))
        .route("/sessions/:id/reset", post(reset_session))
        .route("/sessions/:id/extend", post(extend_session))
        .route("/sessions/:id/actions/:action_id", post(execute_action))
}

// --------------------------------------------------------------------- types

/// JSON error envelope. Re-defined locally because the OSS `ErrorResponse`
/// (in `datashuttle-api-core::handlers::shuttles`) isn't reachable from
/// this binary.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct ManifestView {
    pub version: u32,
    pub deployment: String,
    pub sources: Vec<Value>,
    pub scenarios: Vec<Value>,
    pub config: PlaygroundConfig,
}

#[derive(Debug, Serialize)]
pub struct PlaygroundConfig {
    pub enabled: bool,
    pub default_ttl_seconds: u64,
    pub min_ttl_seconds: u64,
    pub max_ttl_seconds: u64,
    pub requires_auth: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionBody {
    pub scenario_id: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ExtendQuery {
    #[serde(default = "default_extend")]
    pub seconds: u64,
}

fn default_extend() -> u64 {
    3600
}

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub action_id: String,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// --------------------------------------------------------------------- error helpers

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: msg.into(),
        }),
    )
}

fn map_session_err(e: SessionError) -> (StatusCode, Json<ErrorResponse>) {
    let status = match &e {
        SessionError::Disabled => StatusCode::FORBIDDEN,
        SessionError::UnknownScenario(_) => StatusCode::NOT_FOUND,
        SessionError::NotFound(_) => StatusCode::NOT_FOUND,
        SessionError::Forbidden => StatusCode::FORBIDDEN,
        SessionError::UserLimit(_) => StatusCode::CONFLICT,
        SessionError::Cooldown(_) => StatusCode::TOO_MANY_REQUESTS,
        SessionError::InvalidTtl(_) => StatusCode::BAD_REQUEST,
    };
    err(status, e.to_string())
}

fn require_identity(req: &Request) -> Result<Identity, (StatusCode, Json<ErrorResponse>)> {
    req.extensions().get::<Identity>().cloned().ok_or_else(|| {
        err(
            StatusCode::UNAUTHORIZED,
            "playground requires authenticated identity",
        )
    })
}

fn require_sessions(
    state: &ServerState,
) -> Result<Arc<SessionManager>, (StatusCode, Json<ErrorResponse>)> {
    state.sessions.clone().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            "playground is not initialized (manifest missing)",
        )
    })
}

fn require_api_client(
    state: &ServerState,
) -> Result<Arc<ApiClient>, (StatusCode, Json<ErrorResponse>)> {
    state.api_client.clone().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            "playground api callback client is not configured \
             (set PLAYGROUND_API_BASE_URL + PLAYGROUND_SERVICE_TOKEN)",
        )
    })
}

// --------------------------------------------------------------------- handlers

pub async fn get_manifest(
    State(state): State<Arc<ServerState>>,
    _request: Request,
) -> Result<Json<ManifestView>, (StatusCode, Json<ErrorResponse>)> {
    // `/api/v1/playground/manifest` is exempt from both the bearer
    // gate (`auth_middleware`) and the identity middleware so the
    // public UI can render the scenario list before login. Returning
    // only manifest metadata — no tenant data, no per-user state —
    // keeps that safe.
    let Some(mgr) = state.sessions.as_ref() else {
        // Playground disabled or manifest absent → return the
        // disabled card payload so the UI can render a graceful
        // "Playground unavailable" tab instead of a fetch error.
        return Ok(Json(ManifestView {
            version: 0,
            deployment: deployment_str(deployment_of(&state)).to_string(),
            sources: Vec::new(),
            scenarios: Vec::new(),
            config: PlaygroundConfig {
                enabled: false,
                default_ttl_seconds: 0,
                min_ttl_seconds: datashuttle_playground::sessions::MIN_TTL.as_secs(),
                max_ttl_seconds: datashuttle_playground::sessions::MAX_TTL.as_secs(),
                requires_auth: true,
            },
        }));
    };
    let manifest = mgr.manifest();
    let deployment = deployment_of(&state);
    let visible: Vec<&Scenario> = manifest
        .scenarios
        .iter()
        .filter(|s| s.allowed_in(deployment))
        .collect();
    let sources = manifest
        .sources
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    let scenarios = visible
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    Ok(Json(ManifestView {
        version: manifest.version,
        deployment: deployment_str(deployment).to_string(),
        sources,
        scenarios,
        config: PlaygroundConfig {
            enabled: mgr.is_enabled(),
            default_ttl_seconds: mgr.default_ttl().as_secs(),
            min_ttl_seconds: datashuttle_playground::sessions::MIN_TTL.as_secs(),
            max_ttl_seconds: datashuttle_playground::sessions::MAX_TTL.as_secs(),
            requires_auth: true,
        },
    }))
}

pub async fn create_session(
    State(state): State<Arc<ServerState>>,
    request: Request,
) -> Result<(StatusCode, Json<SessionView>), (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;
    let tenant_id = identity.tenant_id.clone();

    let (_request_parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid body: {e}")))?;
    let body: CreateSessionBody = serde_json::from_slice(&bytes)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")))?;

    // Deployment gate — cloud users can't start chaos scenarios, etc.
    let manifest = mgr.manifest();
    let scenario = manifest.scenario(&body.scenario_id).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            format!("unknown scenario: {}", body.scenario_id),
        )
    })?;
    let deployment = deployment_of(&state);
    if !scenario.allowed_in(deployment) {
        return Err(err(
            StatusCode::FORBIDDEN,
            format!(
                "scenario {} is not allowed on deployment kind {}",
                scenario.id,
                deployment_str(deployment)
            ),
        ));
    }

    // Daily session-creation cap per tenant. Charged atomically here;
    // if the downstream insert fails for any reason (e.g. UserLimit)
    // the charge stays — fine, the user still consumed an intent slot
    // and a retry should still be rate-limited.
    if let Err(e) = state.quota.try_consume(tenant_id.as_deref()) {
        state
            .metrics
            .record_session_start(&body.scenario_id, SessionStartOutcome::Denied);
        return Err(err(StatusCode::TOO_MANY_REQUESTS, e.to_string()));
    }
    state
        .metrics
        .record_session_start(&body.scenario_id, SessionStartOutcome::Ok);

    let ttl_override = body.ttl_seconds.map(Duration::from_secs);
    let session = mgr
        .create(
            &identity.user_id,
            tenant_id.clone(),
            &body.scenario_id,
            ttl_override,
        )
        .await
        .map_err(map_session_err)?;

    info!(
        user_id = %identity.user_id,
        session_id = %session.id,
        scenario = %session.scenario_id,
        namespace = %session.namespace,
        "playground session created"
    );

    // Per-session source resource provisioning (postgres schema or
    // mysql database). Best-effort: on failure record + continue so
    // loopback-only parts of the scenario still work.
    if let Some(source) = manifest.source(&scenario.source_id) {
        if let Err(e) = provision_session_resources(&state, source, &session.namespace).await {
            warn!(
                session_id = %session.id,
                namespace = %session.namespace,
                service = ?source.docker_service,
                "create_session: source resource provisioning failed: {e}"
            );
            let _ = mgr
                .update(session.id, &identity.user_id, |s| {
                    s.record(
                        "resource-provision-failed",
                        format!("{e}"),
                        None,
                        Some(false),
                    );
                })
                .await;
        } else if matches!(
            source.docker_service.as_deref(),
            Some("postgres" | "mysql" | "clickhouse" | "redis")
        ) {
            let resource_label = match source.docker_service.as_deref() {
                Some("postgres") => "schema",
                Some("redis") => "key-prefix",
                _ => "database",
            };
            let _ = mgr
                .update(session.id, &identity.user_id, |s| {
                    s.record(
                        "resource-provisioned",
                        format!(
                            "{} {}={}",
                            source.docker_service.as_deref().unwrap_or("?"),
                            resource_label,
                            s.namespace,
                        ),
                        None,
                        Some(true),
                    );
                })
                .await;

            // init_sql against the freshly-provisioned schema/db.
            if let Some(init_rel) = &scenario.init_sql {
                match read_example_file(&state, init_rel) {
                    Ok(init_sql) => {
                        let init_sql = substitute_placeholders(&init_sql, &session);
                        let init_sql = substitute_source_coords(init_sql, Some(source));
                        match dispatch_source_sql(
                            &state,
                            source,
                            &session.namespace,
                            &init_sql,
                        )
                        .await
                        {
                            Ok(_) => {
                                let _ = mgr
                                    .update(session.id, &identity.user_id, |s| {
                                        s.record(
                                            "source-init-applied",
                                            format!("applied {init_rel}"),
                                            None,
                                            Some(true),
                                        );
                                    })
                                    .await;
                            }
                            Err(e) => {
                                warn!(
                                    session_id = %session.id,
                                    "create_session: init_sql dispatch failed: {e}"
                                );
                                let _ = mgr
                                    .update(session.id, &identity.user_id, |s| {
                                        s.record(
                                            "source-init-failed",
                                            format!("{e}"),
                                            None,
                                            Some(false),
                                        );
                                    })
                                    .await;
                            }
                        }
                    }
                    Err(e) => warn!(
                        session_id = %session.id,
                        "create_session: read init_sql failed: {e}"
                    ),
                }
            }
        }
    }

    // Apply the scenario's shuttle_sql via api callback so the user
    // lands on a live shuttle. Without this, subsequent actions would
    // fail because the shuttle_name referenced by the session doesn't
    // yet exist in the registry. The callback is per-statement
    // because /api/v1/sql parses one Statement at a time.
    if let Some(shuttle_rel) = &scenario.shuttle_sql {
        match read_example_file(&state, shuttle_rel) {
            Ok(shuttle_sql) => {
                let shuttle_sql = substitute_placeholders(&shuttle_sql, &session);
                let shuttle_sql = substitute_source_coords(
                    shuttle_sql,
                    manifest.source(&scenario.source_id),
                );
                let statements = split_ds_sql_statements(&shuttle_sql);
                let mut provision_outcome: Result<(), String> = Ok(());
                if let Some(api) = state.api_client.as_ref() {
                    for (idx, stmt) in statements.iter().enumerate() {
                        if let Err(e) = api.exec_sql(&identity, stmt).await {
                            provision_outcome = Err(format!(
                                "statement #{} of {}: {e}",
                                idx + 1,
                                shuttle_rel,
                            ));
                            break;
                        }
                    }
                } else {
                    provision_outcome = Err(
                        "api callback client is not configured — \
                         set PLAYGROUND_API_BASE_URL + PLAYGROUND_SERVICE_TOKEN"
                            .to_string(),
                    );
                }
                match provision_outcome {
                    Ok(()) => {
                        let count = statements.len();
                        let _ = mgr
                            .update(session.id, &identity.user_id, |s| {
                                s.record(
                                    "shuttle-provisioned",
                                    format!(
                                        "applied {shuttle_rel} ({count} statement{})",
                                        if count == 1 { "" } else { "s" }
                                    ),
                                    None,
                                    Some(true),
                                );
                            })
                            .await;
                    }
                    Err(msg) => {
                        warn!(
                            session_id = %session.id,
                            "create_session: shuttle_sql provisioning failed: {msg}"
                        );
                        let _ = mgr
                            .update(session.id, &identity.user_id, |s| {
                                s.record("shuttle-provision-failed", msg, None, Some(false));
                            })
                            .await;
                    }
                }
            }
            Err(e) => {
                warn!(session_id = %session.id, "create_session: read shuttle_sql failed: {e}");
            }
        }
    }

    if let Ok(()) = mgr
        .update(session.id, &identity.user_id, |s| {
            s.record(
                "session-created",
                format!(
                    "namespace={} shuttle={} scenario={}",
                    s.namespace, s.shuttle_name, s.scenario_id
                ),
                None,
                Some(true),
            );
            s.status = SessionStatus::Active;
        })
        .await
    {}

    let view = (&session).into();
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<SessionView>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;
    let session = mgr
        .get(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    Ok(Json((&session).into()))
}

pub async fn list_my_sessions(
    State(state): State<Arc<ServerState>>,
    request: Request,
) -> Result<Json<Vec<SessionView>>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;
    // One session per user, so this returns at most one entry.
    let active = mgr.get_user_session(&identity.user_id).await;
    let mut out = Vec::new();
    if let Some(s) = active {
        out.push((&s).into());
    }
    Ok(Json(out))
}

pub async fn end_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<SessionView>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;
    let mut session = mgr
        .end(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    session.record(
        "session-ended",
        "user ended session".into(),
        None,
        Some(true),
    );
    // Fire teardown: drop shuttle + drop namespace. Synchronous
    // best-effort; on failure return the session view anyway so the
    // UI surfaces the error from session events.
    teardown_session(
        &state,
        &identity,
        &session.shuttle_name,
        &session.connection_name,
        &session.namespace,
    )
    .await;
    info!(session_id = %session.id, "playground session ended");
    Ok(Json((&session).into()))
}

pub async fn reset_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Json<SessionView>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;

    // Pull session + scenario out before we start issuing SQL so we
    // can return clear errors on lookup failure.
    let session = mgr
        .get(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    let manifest = mgr.manifest().clone();
    let scenario = manifest
        .scenario(&session.scenario_id)
        .ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session's scenario no longer in manifest",
            )
        })?
        .clone();

    // Refuse to reset a scenario whose shuttle identifier isn't safe
    // to splice back into DDL. Prevents teardown drift from silently
    // running arbitrary SQL.
    if !is_safe_identifier(&session.shuttle_name)
        || !is_safe_identifier(&session.connection_name)
        || !is_safe_identifier(&session.namespace)
    {
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session names failed identifier validation",
        ));
    }

    mgr.update(id, &identity.user_id, |s| {
        s.status = SessionStatus::Resetting;
        s.record(
            "reset-start",
            "user-triggered reset — dropping + recreating shuttle".into(),
            None,
            Some(true),
        );
    })
    .await
    .map_err(map_session_err)?;

    let api = require_api_client(&state)?;

    // 1. Drop the shuttle so the flight buffer stops and Iceberg
    //    commits quiesce. Best-effort — log + continue on failure.
    if let Err(e) = api
        .exec_sql(
            &identity,
            &format!("DROP SHUTTLE IF EXISTS {}", session.shuttle_name),
        )
        .await
    {
        warn!(session_id = %id, "reset: DROP SHUTTLE failed: {e}");
    }

    // 2. Drop target tables inside the session's namespace.
    if let Err(e) = api
        .exec_sql(
            &identity,
            &format!("DROP NAMESPACE IF EXISTS {} CASCADE", session.namespace),
        )
        .await
    {
        warn!(session_id = %id, "reset: DROP NAMESPACE failed: {e}");
    }

    // 3. Re-run init SQL on the source.
    if let Some(init_rel) = &scenario.init_sql {
        match read_example_file(&state, init_rel) {
            Ok(init_sql) => {
                let source = manifest.source(&scenario.source_id).cloned();
                let init_sql = substitute_placeholders(&init_sql, &session);
                let init_sql = substitute_source_coords(init_sql, source.as_ref());
                if let Some(source) = source {
                    if let Err(e) =
                        dispatch_source_sql(&state, &source, &session.namespace, &init_sql)
                            .await
                    {
                        warn!(session_id = %id, "reset: init_sql failed: {e}");
                    }
                }
            }
            Err(e) => warn!(session_id = %id, "reset: read init_sql: {e}"),
        }
    }

    // 4. Re-run the shuttle SQL template (idempotent via CREATE …
    //    IF NOT EXISTS).
    if let Some(shuttle_rel) = &scenario.shuttle_sql {
        match read_example_file(&state, shuttle_rel) {
            Ok(shuttle_sql) => {
                let shuttle_sql = substitute_placeholders(&shuttle_sql, &session);
                let shuttle_sql = substitute_source_coords(
                    shuttle_sql,
                    manifest.source(&scenario.source_id),
                );
                for (idx, stmt) in split_ds_sql_statements(&shuttle_sql)
                    .into_iter()
                    .enumerate()
                {
                    if let Err(e) = api.exec_sql(&identity, &stmt).await {
                        warn!(
                            session_id = %id,
                            statement_index = idx + 1,
                            "reset: shuttle_sql stmt failed: {e}"
                        );
                        break;
                    }
                }
            }
            Err(e) => warn!(session_id = %id, "reset: read shuttle_sql: {e}"),
        }
    }

    mgr.update(id, &identity.user_id, |s| {
        s.status = SessionStatus::Active;
        s.record("reset-complete", "reset finished".into(), None, Some(true));
    })
    .await
    .map_err(map_session_err)?;

    let session = mgr
        .get(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    Ok(Json((&session).into()))
}

pub async fn extend_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<Uuid>,
    Query(q): Query<ExtendQuery>,
    request: Request,
) -> Result<Json<SessionView>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;
    mgr.update(id, &identity.user_id, |s| {
        s.extend(Duration::from_secs(q.seconds))
    })
    .await
    .map_err(map_session_err)?
    .map_err(|msg| err(StatusCode::BAD_REQUEST, msg.to_string()))?;
    let session = mgr
        .get(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    Ok(Json((&session).into()))
}

pub async fn execute_action(
    State(state): State<Arc<ServerState>>,
    Path((id, action_id)): Path<(Uuid, String)>,
    request: Request,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let identity = require_identity(&request)?;
    let mgr = require_sessions(&state)?;

    // Ownership + cooldown check in one round-trip.
    let session = mgr
        .get(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;
    mgr.touch_action(id, &identity.user_id)
        .await
        .map_err(map_session_err)?;

    let manifest = mgr.manifest();
    let scenario = manifest.scenario(&session.scenario_id).ok_or_else(|| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session's scenario not found in manifest (was it removed?)",
        )
    })?;
    let action = scenario.action(&action_id).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            format!("unknown action {action_id} for scenario {}", scenario.id),
        )
    })?;
    drop(request);

    // Time the action so the action_duration histogram captures
    // p50/p95/p99 per scenario/action/outcome.
    let action_start = std::time::Instant::now();
    let result = run_action(&state, &identity, &session, action).await;
    let action_dur = action_start.elapsed();
    let outcome_kind = if result.is_ok() {
        ActionOutcomeKind::Ok
    } else {
        ActionOutcomeKind::Err
    };
    state.metrics.observe_action_duration(
        &session.scenario_id,
        &action.id,
        outcome_kind,
        action_dur,
    );
    if let Err(e) = &result {
        state.metrics.record_action_error(
            &session.scenario_id,
            &action.id,
            ActionErrorKind::from_message(e),
        );
    }
    let (status_str, stdout, stderr, http_status, body, error) = match &result {
        Ok(r) => (
            "ok".to_string(),
            r.stdout.clone(),
            r.stderr.clone(),
            r.http_status,
            r.body.clone(),
            None,
        ),
        Err(e) => (
            "error".to_string(),
            None,
            None,
            None,
            None,
            Some(e.clone()),
        ),
    };

    // Record into session event log.
    let _ = mgr
        .update(id, &identity.user_id, |s| {
            s.record(
                "action",
                format!(
                    "{} {}",
                    action.id,
                    if result.is_ok() { "ok" } else { "err" }
                ),
                Some(action.id.clone()),
                Some(result.is_ok()),
            );
        })
        .await;

    if let Err(e) = &result {
        warn!(session_id = %id, action_id = %action_id, "playground action failed: {}", e);
    }

    Ok(Json(ActionResponse {
        action_id: action_id.clone(),
        kind: format!("{:?}", action.kind).to_lowercase(),
        status: status_str,
        stdout,
        stderr,
        http_status,
        body,
        error,
    }))
}

// --------------------------------------------------------------------- executor

struct ActionOutcome {
    stdout: Option<String>,
    stderr: Option<String>,
    http_status: Option<u16>,
    body: Option<Value>,
}

async fn run_action(
    state: &ServerState,
    identity: &Identity,
    session: &Session,
    action: &Action,
) -> Result<ActionOutcome, String> {
    match action.kind {
        ActionKind::Sql => run_sql(state, identity, session, action).await,
        ActionKind::Http => run_http(state, identity, session, action).await,
        ActionKind::Shell | ActionKind::Toxiproxy | ActionKind::ResetSnapshot => {
            let cmd = action
                .shell_cmd
                .clone()
                .ok_or_else(|| "missing shell_cmd".to_string())?;
            run_shell(&cmd, session)
                .await
                .map(|(stdout, stderr)| ActionOutcome {
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                    http_status: None,
                    body: None,
                })
        }
        ActionKind::ProduceKafka => {
            let payload_file = action
                .payload_file
                .as_deref()
                .ok_or_else(|| "missing payload_file".to_string())?;
            let repeat = action.repeat.unwrap_or(1);
            produce_kafka(payload_file, repeat, session)
                .await
                .map(|(stdout, stderr)| ActionOutcome {
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                    http_status: None,
                    body: None,
                })
        }
        ActionKind::UploadFile => {
            let payload_file = action
                .payload_file
                .as_deref()
                .ok_or_else(|| "missing payload_file".to_string())?;
            upload_file(payload_file, session)
                .await
                .map(|(stdout, stderr)| ActionOutcome {
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                    http_status: None,
                    body: None,
                })
        }
    }
}

async fn run_sql(
    state: &ServerState,
    identity: &Identity,
    session: &Session,
    action: &Action,
) -> Result<ActionOutcome, String> {
    let sql = match (&action.sql, &action.sql_file) {
        (Some(s), _) => s.clone(),
        (None, Some(path)) => read_example_file(state, path)?,
        (None, None) => return Err("sql action missing sql or sql_file".into()),
    };
    let sql = substitute_placeholders(&sql, session);
    match action.target {
        Some(ActionTarget::Source) => {
            // TCP dispatcher (postgres + mysql) takes priority over
            // docker-shell. The dispatcher handles its own error
            // mapping; the shell fallback covers the remaining
            // sources (clickhouse, mongodb, cassandra).
            let mgr = state
                .sessions
                .as_ref()
                .ok_or_else(|| "playground not initialized".to_string())?;
            let manifest = mgr.manifest().clone();
            let scenario = manifest
                .scenario(&session.scenario_id)
                .ok_or_else(|| "scenario missing".to_string())?;
            let source = manifest
                .source(&scenario.source_id)
                .ok_or_else(|| "source missing".to_string())?;
            let (stdout, stderr) =
                dispatch_source_sql(state, source, &session.namespace, &sql).await?;
            Ok(ActionOutcome {
                stdout: Some(stdout),
                stderr: Some(stderr),
                http_status: None,
                body: None,
            })
        }
        Some(ActionTarget::Ops) | Some(ActionTarget::Target) | None => {
            let api = state
                .api_client
                .as_ref()
                .ok_or_else(|| "api callback client not configured".to_string())?;
            match api.exec_sql(identity, &sql).await {
                Ok(body) => Ok(ActionOutcome {
                    stdout: None,
                    stderr: None,
                    http_status: Some(200),
                    body: Some(body),
                }),
                Err(e) => Err(format!("{e}")),
            }
        }
        Some(other) => Err(format!("SQL cannot target {:?}", other)),
    }
}

async fn run_http(
    state: &ServerState,
    identity: &Identity,
    session: &Session,
    action: &Action,
) -> Result<ActionOutcome, String> {
    let req: &HttpRequest = action
        .http_request
        .as_ref()
        .ok_or_else(|| "http action missing http_request".to_string())?;
    let url = substitute_placeholders(&req.url, session);
    let body = req
        .body
        .clone()
        .map(|v| substitute_placeholders_value(v, session));
    let method = req.method.to_uppercase();
    let api = state
        .api_client
        .as_ref()
        .ok_or_else(|| "api callback client not configured".to_string())?;
    let (status, body) = api
        .request(&method, &url, body, identity)
        .await
        .map_err(|e: ApiCallError| format!("{e}"))?;
    Ok(ActionOutcome {
        stdout: None,
        stderr: None,
        http_status: Some(status),
        body: Some(body),
    })
}

async fn run_shell(cmd: &str, session: &Session) -> Result<(String, String), String> {
    let cmd = substitute_placeholders(cmd, session);
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .await
        .map_err(|e| format!("spawn bash: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!(
            "command exited with {}: {}",
            output.status,
            stderr.lines().take(5).collect::<Vec<_>>().join(" | ")
        ));
    }
    Ok((stdout, stderr))
}

async fn produce_kafka(
    payload_file: &str,
    repeat: u32,
    session: &Session,
) -> Result<(String, String), String> {
    use base64::Engine;
    validate_example_relative_path(payload_file)?;
    let repeat = (repeat.min(100_000)) as usize;
    let topic = kafka_topic_for(session);

    // DS_KAFKA_PLAYGROUND_PORT in cloud deployments now points at the
    // native Kafka API (9092) because the connector binary speaks
    // librdkafka directly. produce_kafka itself still uses Pandaproxy
    // REST (HTTP) — the port for that is hardcoded to 8082 unless
    // overridden via DS_KAFKA_PLAYGROUND_REST_PORT.
    let host =
        std::env::var("DS_KAFKA_PLAYGROUND_HOST").unwrap_or_else(|_| "redpanda-playground".into());
    let port = std::env::var("DS_KAFKA_PLAYGROUND_REST_PORT").unwrap_or_else(|_| "8082".into());
    let broker = std::env::var("DS_KAFKA_PLAYGROUND_BROKER").unwrap_or_else(|_| {
        let native = std::env::var("DS_KAFKA_PLAYGROUND_PORT")
            .unwrap_or_else(|_| "9092".into());
        format!("{host}:{native}")
    });

    // Ensure the topic exists. Pandaproxy's REST produce does NOT
    // auto-create — first POST returns UNKNOWN_TOPIC_OR_PARTITION
    // even though broker `auto_create_topics_enabled=true`. Use rpk
    // (baked in the image) to create idempotently before producing.
    let create = tokio::process::Command::new("rpk")
        .args([
            "topic",
            "create",
            &topic,
            "--brokers",
            &broker,
            "--partitions",
            "1",
            "--replicas",
            "1",
        ])
        .output()
        .await
        .map_err(|e| format!("spawn rpk: {e}"))?;
    // rpk exits non-zero AND prints `TOPIC_ALREADY_EXISTS` to STDOUT
    // (not stderr) when the topic exists — verified empirically against
    // v24.2.7. So we have to check both streams. Treat the
    // already-exists case as success; only fail on genuine errors.
    if !create.status.success() {
        let stderr = String::from_utf8_lossy(&create.stderr);
        let stdout = String::from_utf8_lossy(&create.stdout);
        let already = stderr.contains("TOPIC_ALREADY_EXISTS")
            || stderr.contains("already exists")
            || stdout.contains("TOPIC_ALREADY_EXISTS")
            || stdout.contains("already exists");
        if !already {
            return Err(format!(
                "rpk topic create {topic} (broker={broker}): stdout={stdout} stderr={stderr}"
            ));
        }
    }

    // Read the payload from the baked examples tree. `.json` → produce
    // through Pandaproxy's JSON endpoint with raw JSON value; anything
    // else (e.g. `.raw` poison payloads) → binary endpoint with base64.
    let path = std::path::Path::new("/opt/datashuttle/examples").join(payload_file);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("read {payload_file}: {e}"))?;

    let is_json = std::path::Path::new(payload_file)
        .extension()
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    // Build one record template. Either typed JSON `value` or base64-
    // encoded `value` (binary endpoint). Pandaproxy chokes at ~10k
    // records per request, so we chunk the batch.
    let (content_type, record_template): (&str, Value) = if is_json {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => (
                "application/vnd.kafka.json.v2+json",
                serde_json::json!({"value": v}),
            ),
            Err(_) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                (
                    "application/vnd.kafka.binary.v2+json",
                    serde_json::json!({"value": encoded}),
                )
            }
        }
    } else {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        (
            "application/vnd.kafka.binary.v2+json",
            serde_json::json!({"value": encoded}),
        )
    };

    let url = format!("http://{host}:{port}/topics/{topic}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let chunk = 500;
    let mut produced = 0usize;
    let mut last_body = String::new();
    while produced < repeat {
        let n = chunk.min(repeat - produced);
        let records: Vec<Value> = (0..n).map(|_| record_template.clone()).collect();
        let body = serde_json::json!({"records": records});
        let resp = client
            .post(&url)
            .header("Content-Type", content_type)
            .body(serde_json::to_vec(&body).map_err(|e| format!("encode body: {e}"))?)
            .send()
            .await
            .map_err(|e| format!("pandaproxy POST {url}: {e}"))?;
        let status = resp.status();
        last_body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "pandaproxy returned {status} for topic {topic} (after {produced} produced): {last_body}"
            ));
        }
        produced += n;
    }
    Ok((
        format!("produced {produced} record(s) to {topic} via {host}:{port} ({content_type})"),
        last_body,
    ))
}

async fn upload_file(payload_file: &str, session: &Session) -> Result<(String, String), String> {
    validate_example_relative_path(payload_file)?;
    let bucket = s3_bucket_for(session);

    let endpoint = std::env::var("DS_MINIO_ENDPOINT").unwrap_or_else(|_| "http://minio:9000".into());
    let access = std::env::var("DS_MINIO_ACCESS_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_USER"))
        .map_err(|_| "missing DS_MINIO_ACCESS_KEY / MINIO_ROOT_USER".to_string())?;
    let secret = std::env::var("DS_MINIO_SECRET_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_PASSWORD"))
        .map_err(|_| "missing DS_MINIO_SECRET_KEY / MINIO_ROOT_PASSWORD".to_string())?;

    // Source path under the baked examples tree.
    let src_path = std::path::Path::new("/opt/datashuttle/examples").join(payload_file);
    if !src_path.exists() {
        return Err(format!(
            "payload not found in baked examples: {}",
            src_path.display()
        ));
    }

    // Per-session prefix so concurrent sessions don't see each
    // other's files. The shuttle template's PATH points at
    // `s3://{bucket}/{namespace}/`.
    let prefix = &session.namespace;

    // mc alias set is idempotent and writes to ~/.mc/. The playground
    // user has $HOME=/var/lib/playground so this stays out of /root.
    let alias_cmd = format!(
        "mc alias set local {endpoint} {access} {secret} >/dev/null && \
         mc mb --ignore-existing local/{bucket} >/dev/null && \
         mc cp {src} local/{bucket}/{prefix}/{payload_basename}",
        endpoint = shell_quote(&endpoint),
        access = shell_quote(&access),
        secret = shell_quote(&secret),
        bucket = shell_quote(&bucket),
        prefix = shell_quote(prefix),
        src = shell_quote(&src_path.to_string_lossy()),
        payload_basename = shell_quote(
            std::path::Path::new(payload_file)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| payload_file.to_string())
                .as_str()
        ),
    );
    run_shell(&alias_cmd, session).await
}

/// Create the per-session source resource (postgres schema or mysql
/// database). Returns `Ok(())` for sources that don't require sidecar
/// isolation (kafka, file, in-memory, etc.) so callers can wrap the
/// whole path in a single `if let Err`.
async fn provision_session_resources(
    state: &ServerState,
    source: &Source,
    namespace: &str,
) -> Result<(), String> {
    // The TCP dispatcher creates the per-session schema/db against
    // the shared sidecar. Falls back to docker-shell exec for sources
    // not covered by the dispatcher.
    let (tcp_result, shell_sql) = match source.docker_service.as_deref() {
        Some("postgres") => (
            state.dispatcher.provision_postgres_schema(namespace).await,
            format!("CREATE SCHEMA IF NOT EXISTS \"{namespace}\";"),
        ),
        Some("mysql") => (
            state.dispatcher.provision_mysql_database(namespace).await,
            format!("CREATE DATABASE IF NOT EXISTS `{namespace}`;"),
        ),
        Some("clickhouse") => (
            state
                .dispatcher
                .provision_clickhouse_database(namespace)
                .await,
            format!("CREATE DATABASE IF NOT EXISTS `{namespace}`;"),
        ),
        _ => return Ok(()),
    };
    match tcp_result {
        Ok(()) => Ok(()),
        Err(DispatchError::Unavailable) => {
            if !is_safe_resource_name(namespace) {
                return Err(format!("unsafe resource name: {namespace}"));
            }
            let tcp_active = state.dispatcher.is_tcp_backed();
            exec_source_sql(source, &shell_sql, tcp_active)
                .await
                .map(|_| ())
        }
        Err(other) => Err(other.to_string()),
    }
}

/// Pick the right transport for source SQL: TCP dispatcher for
/// postgres/mysql, docker-shell exec for everything else (clickhouse,
/// mongodb, cassandra).
async fn dispatch_source_sql(
    state: &ServerState,
    source: &Source,
    namespace: &str,
    sql: &str,
) -> Result<(String, String), String> {
    let service = source
        .docker_service
        .as_deref()
        .ok_or_else(|| format!("source {} has no docker_service", source.id))?;
    let use_isolation = is_safe_resource_name(namespace);
    match service {
        "postgres" | "postgres-playground" => {
            let result = if use_isolation {
                state
                    .dispatcher
                    .exec_postgres_in_schema(namespace, sql)
                    .await
            } else {
                state.dispatcher.exec_postgres(sql).await
            };
            match result {
                Ok(out) => return Ok(out),
                Err(DispatchError::Unavailable) => {}
                Err(other) => return Err(other.to_string()),
            }
        }
        "mysql" | "mysql-playground" => {
            let result = if use_isolation {
                state
                    .dispatcher
                    .exec_mysql_in_database(namespace, sql)
                    .await
            } else {
                state.dispatcher.exec_mysql(sql).await
            };
            match result {
                Ok(out) => return Ok(out),
                Err(DispatchError::Unavailable) => {}
                Err(other) => return Err(other.to_string()),
            }
        }
        "clickhouse" | "clickhouse-playground" => {
            let result = if use_isolation {
                state
                    .dispatcher
                    .exec_clickhouse_in_database(namespace, sql)
                    .await
            } else {
                state.dispatcher.exec_clickhouse(sql).await
            };
            match result {
                Ok(out) => return Ok(out),
                Err(DispatchError::Unavailable) => {}
                Err(other) => return Err(other.to_string()),
            }
        }
        "redis" | "redis-playground" => {
            // Redis has no SQL; the playground scenario authors a
            // newline-delimited script of Redis commands. Per-session
            // isolation is a key prefix (Redis logical DBs cap at 16,
            // too few for many concurrent sessions).
            let result = if use_isolation {
                state
                    .dispatcher
                    .exec_redis_in_namespace(namespace, sql)
                    .await
            } else {
                state.dispatcher.exec_redis(sql).await
            };
            match result {
                Ok(out) => return Ok(out),
                Err(DispatchError::Unavailable) => {
                    return Err(
                        "redis dispatcher unavailable — cloud playground requires \
                         DS_REDIS_PLAYGROUND_HOST + redis-playground sidecar"
                            .into(),
                    )
                }
                Err(other) => return Err(other.to_string()),
            }
        }
        _ => {}
    }
    // Shell fallback. Inject schema/db isolation so init.sql and
    // action scripts authored with unqualified table names land in
    // the session's private namespace.
    let isolated_sql = if use_isolation {
        match service {
            "postgres" | "postgres-playground" => Some(format!(
                "CREATE SCHEMA IF NOT EXISTS \"{namespace}\";\n\
                 SET search_path = \"{namespace}\", public;\n\
                 {sql}"
            )),
            "mysql" | "mysql-playground" => Some(format!(
                "CREATE DATABASE IF NOT EXISTS `{namespace}`;\n\
                 USE `{namespace}`;\n\
                 {sql}"
            )),
            "clickhouse" | "clickhouse-playground" => Some(format!(
                "CREATE DATABASE IF NOT EXISTS `{namespace}`;\n\
                 USE `{namespace}`;\n\
                 {sql}"
            )),
            _ => None,
        }
    } else {
        None
    };
    let sql = isolated_sql.as_deref().unwrap_or(sql);
    let tcp_active = state.dispatcher.is_tcp_backed();
    exec_source_sql(source, sql, tcp_active).await
}

/// Docker-shell fallback for source SQL. Reaches the source sidecar
/// containers via the `docker compose` CLI; requires the
/// playground-server container to have the docker socket mounted at
/// `/var/run/docker.sock` and `docker` available on `PATH`. Add to
/// deploy compose:
///
/// ```yaml
/// playground:
///   volumes:
///     - /var/run/docker.sock:/var/run/docker.sock
/// ```
///
/// When the TCP dispatcher is active, postgres/mysql callers MUST go
/// through the dispatcher — the shell branch is reserved for sources
/// the dispatcher doesn't cover.
async fn exec_source_sql(
    source: &Source,
    sql: &str,
    tcp_dispatcher_active: bool,
) -> Result<(String, String), String> {
    let service = source
        .docker_service
        .as_deref()
        .ok_or_else(|| format!("source {} has no docker_service", source.id))?;
    if tcp_dispatcher_active && matches!(service, "postgres" | "mysql") {
        return Err(format!(
            "playground shell dispatch is disabled for service={service} \
             when the TCP dispatcher is active — must go through the \
             dispatcher instead"
        ));
    }
    // Base64-encode to avoid heredoc / quoting problems with arbitrary SQL.
    let b64 = base64_encode(sql.as_bytes());
    let Some(spec) = docker_shell_registry().get(service) else {
        return Err(format!("no source SQL dispatcher for service {service}"));
    };
    let shell_body = (spec.build_shell)(&b64);
    let _ = spec.container_cmd; // retained for observability on failure
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&shell_body)
        .output()
        .await
        .map_err(|e| format!("spawn docker exec: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!(
            "source SQL failed on {service}: {}",
            stderr.lines().take(5).collect::<Vec<_>>().join(" | ")
        ));
    }
    Ok((stdout, stderr))
}

// --------------------------------------------------------------------- helpers

/// One entry in the docker-shell registry — describes how to pipe
/// base64-encoded SQL through `docker compose exec` for a given
/// playground service.
struct DockerShellSpec {
    build_shell: fn(&str) -> String,
    container_cmd: &'static str,
}

struct DockerShellRegistry {
    entries: std::collections::HashMap<&'static str, DockerShellSpec>,
}

impl DockerShellRegistry {
    fn get(&self, service: &str) -> Option<&DockerShellSpec> {
        self.entries.get(service)
    }
}

fn build_docker_shell_registry() -> DockerShellRegistry {
    let mut entries: std::collections::HashMap<&'static str, DockerShellSpec> =
        std::collections::HashMap::new();
    entries.insert(
        "postgres",
        DockerShellSpec {
            build_shell: |b64| {
                format!(
                    "echo {b64} | base64 -d | \
                     docker compose -f /opt/datashuttle/examples/docker-compose.yml exec -T postgres \
                     psql -U postgres -d ecommerce -v ON_ERROR_STOP=1"
                )
            },
            container_cmd: "psql",
        },
    );
    entries.insert(
        "mysql",
        DockerShellSpec {
            build_shell: |b64| {
                format!(
                    "echo {b64} | base64 -d | \
                     docker compose -f /opt/datashuttle/examples/docker-compose.yml exec -T mysql \
                     mysql -uroot -prootpass --default-character-set=utf8mb4 iot"
                )
            },
            container_cmd: "mysql",
        },
    );
    entries.insert(
        "clickhouse",
        DockerShellSpec {
            build_shell: |b64| {
                format!(
                    "echo {b64} | base64 -d | \
                     docker compose -f /opt/datashuttle/examples/docker-compose.yml exec -T clickhouse \
                     clickhouse-client --multiquery"
                )
            },
            container_cmd: "clickhouse-client",
        },
    );
    entries.insert(
        "mongodb",
        DockerShellSpec {
            build_shell: |b64| {
                format!(
                    "echo {b64} | base64 -d | \
                     docker compose -f /opt/datashuttle/examples/docker-compose.yml exec -T mongodb mongosh --quiet"
                )
            },
            container_cmd: "mongosh",
        },
    );
    entries.insert(
        "cassandra",
        DockerShellSpec {
            build_shell: |b64| {
                format!(
                    "echo {b64} | base64 -d | \
                     docker compose -f /opt/datashuttle/examples/docker-compose.yml exec -T cassandra cqlsh"
                )
            },
            container_cmd: "cqlsh",
        },
    );
    DockerShellRegistry { entries }
}

fn docker_shell_registry() -> &'static DockerShellRegistry {
    static R: std::sync::OnceLock<DockerShellRegistry> = std::sync::OnceLock::new();
    R.get_or_init(build_docker_shell_registry)
}

struct SourceCoordsSpec {
    env_prefix: &'static str,
    default_host: &'static str,
    default_port: &'static str,
    default_db: &'static str,
    default_user: &'static str,
    default_pw: &'static str,
}

struct SourceCoordsRegistry {
    entries: std::collections::HashMap<&'static str, SourceCoordsSpec>,
}

impl SourceCoordsRegistry {
    fn get(&self, service: &str) -> Option<&SourceCoordsSpec> {
        self.entries.get(service)
    }
}

fn build_source_coords_registry() -> SourceCoordsRegistry {
    let mut entries: std::collections::HashMap<&'static str, SourceCoordsSpec> =
        std::collections::HashMap::new();
    entries.insert(
        "postgres",
        SourceCoordsSpec {
            env_prefix: "DS_PG_PLAYGROUND",
            default_host: "localhost",
            default_port: "5432",
            default_db: "ecommerce",
            default_user: "postgres",
            default_pw: "postgres",
        },
    );
    entries.insert(
        "mysql",
        SourceCoordsSpec {
            env_prefix: "DS_MYSQL_PLAYGROUND",
            default_host: "localhost",
            default_port: "3306",
            default_db: "iot",
            default_user: "root",
            default_pw: "rootpass",
        },
    );
    // Kafka (Redpanda single-broker sidecar). `db` / `user` / `pw`
    // are unused for the kafka client but the substitution helper
    // expects them populated; templates only touch
    // `{source_host}` + `{source_port}`. Port `9092` is Redpanda's
    // PLAINTEXT listener inside the compose network; on the local
    // demo bundle (which exposes 19092 to host) the env block
    // overrides this default.
    entries.insert(
        "redpanda",
        SourceCoordsSpec {
            env_prefix: "DS_KAFKA_PLAYGROUND",
            default_host: "localhost",
            default_port: "19092",
            default_db: "",
            default_user: "",
            default_pw: "",
        },
    );
    // ClickHouse playground sidecar — HTTP interface on 8123 used by
    // both the playground dispatcher (this binary) and the cloud
    // shuttle's clickhouse connector. The init.sql / shuttle.sql
    // templates only touch `{source_host}` + `{source_port}` plus
    // `{source_user}` / `{source_password}` so they can authenticate;
    // `{source_db}` defaults to the per-session isolated database
    // overridden by the substitute_session_db hook below.
    entries.insert(
        "clickhouse",
        SourceCoordsSpec {
            env_prefix: "DS_CLICKHOUSE_PLAYGROUND",
            default_host: "clickhouse-playground",
            default_port: "8123",
            default_db: "playground",
            default_user: "playground",
            default_pw: "playground",
        },
    );
    // Redis playground sidecar — host:port for the redis-streams-events
    // scenario. The shuttle.sql template stashes the connection coords in
    // `host`/`port` properties; the connector itself reads them from the
    // JSON-schema config. No user/password (redis defaults to auth-less
    // inside the playground network); `db` index defaults to 0.
    entries.insert(
        "redis",
        SourceCoordsSpec {
            env_prefix: "DS_REDIS_PLAYGROUND",
            default_host: "redis-playground",
            default_port: "6379",
            default_db: "0",
            default_user: "",
            default_pw: "",
        },
    );
    // WireMock — fake REST API backing the rest-api-polling scenario.
    // The shuttle.sql template uses `{source_host}` + `{source_port}` to
    // build `base_url`; user/password/db are unused but the placeholder
    // substituter expects them so we hand back empty strings.
    entries.insert(
        "wiremock",
        SourceCoordsSpec {
            env_prefix: "DS_WIREMOCK_PLAYGROUND",
            default_host: "wiremock-playground",
            default_port: "8080",
            default_db: "",
            default_user: "",
            default_pw: "",
        },
    );
    SourceCoordsRegistry { entries }
}

fn source_coords_registry() -> &'static SourceCoordsRegistry {
    static R: std::sync::OnceLock<SourceCoordsRegistry> = std::sync::OnceLock::new();
    R.get_or_init(build_source_coords_registry)
}

fn deployment_of(state: &ServerState) -> Deployment {
    match state.config.deployment_kind.as_str() {
        "cloud" => Deployment::Cloud,
        "dev" => Deployment::Dev,
        _ => Deployment::SelfManaged,
    }
}

fn deployment_str(d: Deployment) -> &'static str {
    match d {
        Deployment::Dev => "dev",
        Deployment::SelfManaged => "self-managed",
        Deployment::Cloud => "cloud",
    }
}

fn substitute_placeholders(s: &str, session: &Session) -> String {
    // S3 / MinIO substitutions for file scenarios. Cloud deployments
    // pass real credentials via DS_MINIO_*; standalone demos default
    // to `minioadmin` / `minioadmin`.
    let minio_endpoint = std::env::var("DS_MINIO_ENDPOINT")
        .unwrap_or_else(|_| "http://minio:9000".into());
    let minio_access = std::env::var("DS_MINIO_ACCESS_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_USER"))
        .unwrap_or_else(|_| "minioadmin".into());
    let minio_secret = std::env::var("DS_MINIO_SECRET_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_PASSWORD"))
        .unwrap_or_else(|_| "minioadmin".into());

    let out = s
        .replace("{shuttle}", &session.shuttle_name)
        .replace("{namespace}", &session.namespace)
        .replace("{connection}", &session.connection_name)
        .replace("{session}", &session.id.to_string())
        .replace("{minio_endpoint}", &minio_endpoint)
        .replace("{minio_access_key}", &minio_access)
        .replace("{minio_secret_key}", &minio_secret);
    if let Some(unresolved) = detect_unresolved_placeholder(&out) {
        tracing::warn!(
            session_id = %session.id,
            scenario = %session.scenario_id,
            placeholder = %unresolved,
            "playground: unresolved {{placeholder}} in substituted text — check the scenario authoring"
        );
    }
    out
}

fn substitute_source_coords(s: String, source: Option<&Source>) -> String {
    let Some(source) = source else {
        return s;
    };
    let Some(service) = source.docker_service.as_deref() else {
        return s;
    };
    let Some(coords) = source_coords_registry().get(service) else {
        return s;
    };
    let (prefix, default_host, default_port, default_db, default_user, default_pw) = (
        coords.env_prefix,
        coords.default_host,
        coords.default_port,
        coords.default_db,
        coords.default_user,
        coords.default_pw,
    );
    let host = std::env::var(format!("{prefix}_HOST")).unwrap_or_else(|_| default_host.into());
    let port = std::env::var(format!("{prefix}_PORT")).unwrap_or_else(|_| default_port.into());
    let db = std::env::var(format!("{prefix}_DB")).unwrap_or_else(|_| default_db.into());
    let user = std::env::var(format!("{prefix}_USER")).unwrap_or_else(|_| default_user.into());
    let pw = std::env::var(format!("{prefix}_PASSWORD")).unwrap_or_else(|_| default_pw.into());
    s.replace("{source_host}", &host)
        .replace("{source_port}", &port)
        .replace("{source_db}", &db)
        .replace("{source_user}", &user)
        .replace("{source_password}", &pw)
}

/// First `{...}` fragment that looks like an unresolved placeholder.
/// Conservative: skips `source_*` (handled by the next pass) and
/// rejects strings that don't look like simple identifier-style
/// names.
fn detect_unresolved_placeholder(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let Some(end_off) = bytes[start..].iter().position(|&b| b == b'}') else {
            break;
        };
        let end = start + end_off;
        let inner = &s[start..end];
        let looks_like_placeholder = !inner.is_empty()
            && inner.len() <= 40
            && !inner.starts_with("source_")
            && inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if looks_like_placeholder {
            return Some(format!("{{{inner}}}"));
        }
        i = end + 1;
    }
    None
}

fn substitute_placeholders_value(v: Value, session: &Session) -> Value {
    match v {
        Value::String(s) => Value::String(substitute_placeholders(&s, session)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| substitute_placeholders_value(item, session))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k, substitute_placeholders_value(v, session));
            }
            Value::Object(out)
        }
        other => other,
    }
}

fn read_example_file(state: &ServerState, path: &str) -> Result<String, String> {
    validate_example_relative_path(path)?;
    let full = state.config.examples_dir.join(path);
    std::fs::read_to_string(&full).map_err(|e| format!("read {}: {e}", full.display()))
}

/// Split a DataShuttle SQL script into individual statements.
///
/// `crate::sql::parser::parse` (the /api/v1/sql backend) handles one
/// Statement per call, so multi-statement scenario templates
/// (CREATE CONNECTION + CREATE SHUTTLE + RESUME) must be pre-split.
/// The splitter is deliberately minimal — DataShuttle DDL doesn't use
/// dollar-quoted blocks or BEGIN/END blocks — but does respect
/// single-quoted string literals and `--` line comments.
pub(crate) fn split_ds_sql_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_line_comment {
            cur.push(ch);
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if !in_string && ch == '-' && chars.peek() == Some(&'-') {
            cur.push(ch);
            cur.push(chars.next().unwrap());
            in_line_comment = true;
            continue;
        }
        if ch == '\'' {
            if in_string && chars.peek() == Some(&'\'') {
                cur.push(ch);
                cur.push(chars.next().unwrap());
                continue;
            }
            in_string = !in_string;
            cur.push(ch);
            continue;
        }
        if ch == ';' && !in_string {
            let trimmed = cur.trim().to_string();
            if !trimmed.is_empty() {
                out.push(trimmed);
            }
            cur.clear();
            continue;
        }
        cur.push(ch);
    }
    let trimmed = cur.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

fn validate_example_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("absolute path rejected: {path}"));
    }
    for seg in path.split(['/', '\\']) {
        if seg == ".." || seg == "~" {
            return Err(format!("path traversal rejected: {path}"));
        }
    }
    Ok(())
}

fn kafka_topic_for(session: &Session) -> String {
    format!("{}_src", session.shuttle_name)
}

fn s3_bucket_for(_session: &Session) -> String {
    "file-ingestion".into()
}

fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Teardown: drop shuttle, drop namespace via catalog DELETE, drop
/// connection, drop per-session postgres/mysql resources + cluster-
/// level publication/replication-slot artifacts.
///
/// Best-effort: every step logs + continues on failure so a single
/// stuck step doesn't strand the session in a half-torn state.
async fn teardown_session(
    state: &ServerState,
    identity: &Identity,
    shuttle_name: &str,
    connection_name: &str,
    namespace: &str,
) {
    let td_start = std::time::Instant::now();
    if !is_safe_identifier(shuttle_name) {
        warn!(
            shuttle = %shuttle_name,
            "teardown skipped — shuttle name failed identifier validation"
        );
        return;
    }
    let Some(api) = state.api_client.as_ref() else {
        warn!(
            shuttle = %shuttle_name,
            "teardown skipped — api callback client not configured"
        );
        return;
    };

    if let Err(e) = api
        .exec_sql(identity, &format!("DROP SHUTTLE IF EXISTS {shuttle_name}"))
        .await
    {
        warn!(shuttle = %shuttle_name, "teardown DROP SHUTTLE failed: {e}");
    }

    if is_safe_identifier(namespace) {
        // Drop the namespace via the catalog REST API with cascade +
        // purge so iceberg tables and their parquet files are
        // removed. Plain `DROP NAMESPACE` only updates the in-memory
        // namespace manager.
        let path = format!(
            "/api/v1/catalog/namespaces/{namespace}?cascade=true&purge=true"
        );
        if let Err(e) = api.request("DELETE", &path, None, identity).await {
            warn!(
                namespace = %namespace,
                "teardown DROP NAMESPACE failed: {e}",
            );
        }
        // Polaris' purgeRequested doesn't actually wipe S3 objects in
        // our setup (catalog drops the table reference but leaves
        // parquet + manifest blobs behind). Purge the namespace
        // prefix directly via mc so the warehouse doesn't accumulate
        // orphan data files. Same belt-and-suspenders pattern for
        // the file-ingestion bucket (upload-file scenarios). Best-
        // effort: log + continue, never fail the teardown.
        if let Err(e) = purge_s3_namespace(namespace).await {
            warn!(
                namespace = %namespace,
                "teardown S3 purge failed: {e}",
            );
        }
    } else {
        warn!(
            namespace = %namespace,
            "teardown namespace drop skipped — name failed identifier validation"
        );
    }

    if is_safe_identifier(connection_name) {
        if let Err(e) = api
            .exec_sql(
                identity,
                &format!("DROP CONNECTION IF EXISTS {connection_name}"),
            )
            .await
        {
            warn!(connection = %connection_name, "teardown DROP CONNECTION failed: {e}");
        }
    } else {
        warn!(
            connection = %connection_name,
            "teardown connection drop skipped — connection name failed identifier validation"
        );
    }

    // Drop the per-session postgres schema + mysql database. Fire-
    // and-forget both regardless of the session's scenario: DROP …
    // IF EXISTS is a cheap no-op when the session didn't provision
    // that resource.
    if is_safe_resource_name(namespace) {
        // Drop cluster-level postgres artifacts BEFORE the schema so
        // the replication decoder is quiesced first.
        if is_safe_playground_shuttle_artifact(shuttle_name) {
            let pub_name = format!("{shuttle_name}_pub");
            let slot_name = format!("{shuttle_name}_slot");
            if let Err(e) = state.dispatcher.drop_postgres_publication(&pub_name).await {
                if !matches!(e, DispatchError::Unavailable) {
                    warn!(publication = %pub_name, "teardown DROP PUBLICATION failed: {e}");
                }
            }
            if let Err(e) = state
                .dispatcher
                .drop_postgres_replication_slot(&slot_name)
                .await
            {
                if !matches!(e, DispatchError::Unavailable) {
                    warn!(slot = %slot_name, "teardown DROP REPLICATION SLOT failed: {e}");
                }
            }
        }
        if let Err(e) = state.dispatcher.teardown_postgres_schema(namespace).await {
            if !matches!(e, DispatchError::Unavailable) {
                warn!(
                    namespace = %namespace,
                    "teardown postgres schema failed: {e}"
                );
            }
        }
        if let Err(e) = state.dispatcher.teardown_mysql_database(namespace).await {
            if !matches!(e, DispatchError::Unavailable) {
                warn!(
                    namespace = %namespace,
                    "teardown mysql database failed: {e}"
                );
            }
        }
        if let Err(e) = state
            .dispatcher
            .teardown_clickhouse_database(namespace)
            .await
        {
            if !matches!(e, DispatchError::Unavailable) {
                warn!(
                    namespace = %namespace,
                    "teardown clickhouse database failed: {e}"
                );
            }
        }
        if let Err(e) = state
            .dispatcher
            .teardown_redis_namespace(namespace)
            .await
        {
            if !matches!(e, DispatchError::Unavailable) {
                warn!(
                    namespace = %namespace,
                    "teardown redis namespace failed: {e}"
                );
            }
        }
    }

    state
        .metrics
        .observe_teardown(TeardownKind::Session, td_start.elapsed());
}

/// Shell out to `mc` and remove every object whose key starts with
/// `<namespace>/` in the warehouse + file-ingestion buckets. Called
/// from `teardown_session` because Polaris' `purgeRequested=true`
/// only drops catalog entries, not the S3 blobs underneath.
async fn purge_s3_namespace(namespace: &str) -> Result<(), String> {
    let endpoint =
        std::env::var("DS_MINIO_ENDPOINT").unwrap_or_else(|_| "http://minio:9000".into());
    let access = std::env::var("DS_MINIO_ACCESS_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_USER"))
        .map_err(|_| "missing DS_MINIO_ACCESS_KEY / MINIO_ROOT_USER".to_string())?;
    let secret = std::env::var("DS_MINIO_SECRET_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_PASSWORD"))
        .map_err(|_| "missing DS_MINIO_SECRET_KEY / MINIO_ROOT_PASSWORD".to_string())?;
    // The warehouse bucket name is the host portion of DS_WAREHOUSE
    // (e.g. `s3://warehouse/` → `warehouse`). Default matches the
    // cloud-local compose.
    let warehouse_bucket = std::env::var("DS_WAREHOUSE")
        .ok()
        .and_then(|s| {
            s.strip_prefix("s3://")
                .map(|rest| rest.trim_end_matches('/').split('/').next().unwrap_or("").to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "warehouse".into());

    let cmd = format!(
        "mc alias set local {endpoint} {access} {secret} >/dev/null && \
         mc rm --recursive --force local/{wh}/{ns}/ >/dev/null 2>&1 || true; \
         mc rm --recursive --force local/file-ingestion/{ns}/ >/dev/null 2>&1 || true",
        endpoint = shell_quote(&endpoint),
        access = shell_quote(&access),
        secret = shell_quote(&secret),
        wh = shell_quote(&warehouse_bucket),
        ns = shell_quote(namespace),
    );
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .await
        .map_err(|e| format!("spawn mc: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "mc rm exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Periodically reap TTL-expired sessions. Each expired session goes
/// through `teardown_session` so its catalog namespace, parquet
/// files, source-side schema, and api connection all land in the
/// same cleanup path as an explicit DELETE — without it a TTL
/// expiration left the catalog full of orphan namespaces.
///
/// Spawned from `main`. Returns once the receiver is dropped, but
/// in practice the task runs for the lifetime of the server.
pub fn spawn_session_reaper(state: Arc<ServerState>, interval: std::time::Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // First tick fires immediately — skip it so the server has a
        // moment to settle before we start touching the catalog.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Some(mgr) = state.sessions.as_ref() else {
                continue;
            };
            let expired = mgr.sweep_expired().await;
            if expired.is_empty() {
                continue;
            }
            info!(count = expired.len(), "reaping expired playground sessions");
            for session in expired {
                let identity = Identity {
                    user_id: session.user_id.clone(),
                    tenant_id: None,
                    actor_id: None,
                    auth_method: "ttl-reaper".into(),
                };
                teardown_session(
                    state.as_ref(),
                    &identity,
                    &session.shuttle_name,
                    &session.connection_name,
                    &session.namespace,
                )
                .await;
            }
        }
    });
}

/// One-shot orphan sweep against the api registry. Finds shuttles whose
/// target namespace looks playground-owned (`warehouse.playground_*`)
/// but isn't referenced by any live session in the hydrated session
/// map, and tears them down through the standard cleanup path.
///
/// Necessary because the api-side registry (Pg) outlives the playground
/// container — shuttles, connections, Iceberg namespaces, parquet, and
/// per-session schemas in postgres/mysql/clickhouse all persist a
/// playground restart. With persistence enabled
/// (`SessionManager::new_with_persistence`) the TTL reaper alone covers
/// the live-restart case, but artifacts created BEFORE persistence was
/// turned on, or sessions whose `sessions.json` was deleted, still
/// orphan. This sweep is the catch-up pass that catches both.
///
/// Identity used for the cleanup calls is a synthesized
/// `playground-orphan-sweeper` user — orphan artifacts have
/// `owner=null, tenant_id=null` so any service-token-authenticated
/// caller can drop them.
pub async fn sweep_api_orphans(state: Arc<ServerState>) {
    let Some(api) = state.api_client.as_ref() else {
        info!("orphan sweep skipped — api callback client not configured");
        return;
    };
    let Some(mgr) = state.sessions.as_ref() else {
        return;
    };

    let live_shuttles = mgr.live_shuttles().await;
    let live_namespaces = mgr.live_namespaces().await;

    let identity = Identity {
        user_id: "playground-orphan-sweeper".into(),
        tenant_id: None,
        actor_id: None,
        auth_method: "orphan-sweeper".into(),
    };

    let shuttles = match api
        .request("GET", "/api/v1/shuttles", None, &identity)
        .await
    {
        Ok((_, v)) => v.as_array().cloned().unwrap_or_default(),
        Err(e) => {
            warn!(error = %e, "orphan sweep: GET /api/v1/shuttles failed");
            return;
        }
    };

    let mut swept = 0usize;
    for s in &shuttles {
        let Some(name) = s.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(target) = s.get("target").and_then(|v| v.as_str()) else {
            continue;
        };
        let conn = s.get("connection").and_then(|v| v.as_str()).unwrap_or("");
        // target = "warehouse.playground_<hex>_<hex>" — strip warehouse
        // prefix to get the namespace identifier.
        let Some((_, ns)) = target.split_once('.') else {
            continue;
        };
        if !ns.starts_with("playground_") {
            continue;
        }
        if live_shuttles.contains(name) {
            continue;
        }
        if live_namespaces.contains(ns) {
            continue;
        }

        warn!(
            shuttle = %name,
            connection = %conn,
            namespace = %ns,
            "playground orphan sweep: tearing down stale api artifacts"
        );
        teardown_session(state.as_ref(), &identity, name, conn, ns).await;
        swept += 1;
    }

    // Drop any leftover playground connection whose backing shuttles
    // were already dropped (e.g. partial-teardown carry-over from
    // pre-persistence days). teardown_session above also drops the
    // connection paired with each shuttle, but the api lets a
    // connection outlive its shuttles, so we close that gap.
    let connections = match api
        .request("GET", "/api/v1/connections", None, &identity)
        .await
    {
        Ok((_, v)) => v.as_array().cloned().unwrap_or_default(),
        Err(e) => {
            warn!(error = %e, "orphan sweep: GET /api/v1/connections failed");
            return;
        }
    };
    let mut conn_swept = 0usize;
    for c in &connections {
        let Some(name) = c.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        // Playground connections follow the pg_<hex>_<hex>_src naming
        // contract (see substitute_placeholders).
        if !name.starts_with("pg_") || !name.ends_with("_src") {
            continue;
        }
        // Skip connections still referenced by a live shuttle.
        let still_used = live_shuttles.iter().any(|s| {
            // The shuttle's connection follows the same prefix so we
            // can cheaply test by string-equality.
            shuttles.iter().any(|sv| {
                sv.get("name").and_then(|v| v.as_str()) == Some(s.as_str())
                    && sv.get("connection").and_then(|v| v.as_str()) == Some(name)
            })
        });
        if still_used {
            continue;
        }
        if let Err(e) = api
            .exec_sql(&identity, &format!("DROP CONNECTION IF EXISTS {name}"))
            .await
        {
            warn!(connection = %name, "orphan sweep: DROP CONNECTION failed: {e}");
        } else {
            conn_swept += 1;
        }
    }

    if swept > 0 || conn_swept > 0 {
        info!(
            shuttles = swept,
            connections = conn_swept,
            "playground orphan sweep complete"
        );
    } else {
        info!("playground orphan sweep: nothing to clean");
    }
}

/// Is this string safe to splice into a DataShuttle SQL DDL identifier
/// position? Accept only the character set produced by `derive_namespace`
/// so any drift in name derivation that introduces punctuation fails
/// closed.
fn is_safe_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// --------------------------------------------------------------------- tests

#[cfg(test)]
mod splitter_tests {
    use super::split_ds_sql_statements;

    #[test]
    fn splits_three_statement_shuttle_template() {
        let sql = "CREATE CONNECTION foo TYPE POSTGRES PROPERTIES (host='localhost');\n\
                   CREATE SHUTTLE bar SOURCE foo TABLES (t) TARGET w.ns;\n\
                   RESUME SHUTTLE bar;";
        let stmts = split_ds_sql_statements(sql);
        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].starts_with("CREATE CONNECTION"));
        assert!(stmts[1].starts_with("CREATE SHUTTLE"));
        assert!(stmts[2].starts_with("RESUME SHUTTLE"));
    }

    #[test]
    fn preserves_semicolon_inside_string_literal() {
        let sql = "CREATE CONNECTION x TYPE POSTGRES PROPERTIES (host='a;b');\n\
                   RESUME SHUTTLE p;";
        let stmts = split_ds_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("'a;b'"));
    }

    #[test]
    fn ignores_terminator_inside_line_comment() {
        let sql = "-- terminator; in comment\nRESUME SHUTTLE p;";
        let stmts = split_ds_sql_statements(sql);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("RESUME"));
    }

    #[test]
    fn drops_trailing_whitespace_only_segments() {
        let sql = ";;RESUME SHUTTLE p;;;   ;\n;";
        let stmts = split_ds_sql_statements(sql);
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0], "RESUME SHUTTLE p");
    }

    #[test]
    fn handles_escaped_single_quote_inside_string() {
        let sql = "CREATE CONNECTION x PROPERTIES (note='don''t; split');\n\
                   RESUME SHUTTLE p;";
        let stmts = split_ds_sql_statements(sql);
        assert_eq!(
            stmts.len(),
            2,
            "escaped quote must not terminate the string"
        );
    }
}

#[cfg(test)]
mod path_tests {
    use super::validate_example_relative_path;

    #[test]
    fn empty_rejected() {
        assert!(validate_example_relative_path("").is_err());
    }

    #[test]
    fn absolute_rejected() {
        assert!(validate_example_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn traversal_rejected() {
        assert!(validate_example_relative_path("../etc/passwd").is_err());
        assert!(validate_example_relative_path("a/../b").is_err());
    }

    #[test]
    fn ordinary_relative_path_ok() {
        assert!(validate_example_relative_path("postgres/init.sql").is_ok());
    }
}

#[cfg(test)]
mod identifier_tests {
    use super::is_safe_identifier;

    #[test]
    fn alnum_underscore_ok() {
        assert!(is_safe_identifier("shuttle_42"));
        assert!(is_safe_identifier("playground_aB_cd"));
    }

    #[test]
    fn dash_rejected() {
        assert!(!is_safe_identifier("shuttle-42"));
    }

    #[test]
    fn empty_rejected() {
        assert!(!is_safe_identifier(""));
    }

    #[test]
    fn space_rejected() {
        assert!(!is_safe_identifier("a b"));
    }
}
