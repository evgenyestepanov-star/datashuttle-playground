//! Playground source-database dispatch trait (#829, #817 Phase C).
//!
//! Until Phase C the api crate hosted a concrete `PlaygroundDispatcher`
//! struct holding `sqlx::PgPool` + `mysql_async::Pool` fields behind 19
//! `#[cfg(feature = "saas")]` gates. Phase C splits the surface:
//!
//! * The trait + validators + `DispatchError` + a `NoopPlaygroundDispatcher`
//!   default stay in api. These have no sqlx / mysql_async dependency
//!   so the OSS dep graph is clean.
//! * The concrete TCP-backed implementation moves to
//!   `datashuttle-cloud::playground::TcpPlaygroundDispatcher`. That
//!   crate owns the pool types + the AWS-less sidecar provisioning
//!   behaviour. Cloud deployments pick it up in the cli's `saas` block
//!   via `state.playground_dispatcher = Arc::new(...)`.
//!
//! The validators (`is_safe_resource_name`,
//! `is_safe_playground_shuttle_artifact`) stay in api because multiple
//! handler paths reach for them regardless of which dispatcher is
//! active — they're pure input-sanitisation, not dispatch itself.
//!
//! Handlers never branch on which impl backs the trait object; they
//! just call the trait method and fall through on `DispatchError::Unavailable`.
//! OSS therefore exercises exactly the same call paths as cloud, with
//! the Noop impl short-circuiting to `Unavailable` and the handler's
//! existing `Err(Unavailable)` branch firing the local-shell fallback.

use async_trait::async_trait;
use thiserror::Error;

/// Bounded per-protocol connection pool size used by the concrete
/// cloud impl. Declared here because the value is part of the
/// dispatcher's documented public contract (ops docs call it out);
/// moving the constant would break those refs silently.
pub const POOL_MAX_CONNECTIONS: u32 = 8;

/// How long the pool is allowed to wait for a free connection before
/// surfacing a timeout to the caller.
pub const POOL_ACQUIRE_TIMEOUT_SECS: u64 = 10;

/// Per-statement timeout — enforced both client-side in the cloud impl
/// and server-side via sidecar `statement_timeout` (see
/// `deploy/jarvis-cloud/docker-compose.yaml`).
pub const STATEMENT_TIMEOUT_SECS: u64 = 30;

/// Errors the dispatcher can surface. Handlers match on these to
/// decide whether to fall back to the local shell branch
/// (`Unavailable`) or to surface a user-facing error (everything
/// else).
#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("playground dispatcher misconfigured: {0}")]
    Config(String),
    #[error("could not connect to playground sidecar: {0}")]
    Connect(String),
    #[error("authentication to playground sidecar failed: {0}")]
    Auth(String),
    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),
    #[error("duplicate key: {0}")]
    DuplicateKey(String),
    #[error("query timed out after {0}s")]
    Timeout(u64),
    #[error("playground dispatcher protocol error: {0}")]
    Protocol(String),
    #[error("playground dispatcher unavailable in this build (saas feature off)")]
    Unavailable,
}

/// The dispatch surface the playground handlers call through.
///
/// Implementations:
/// * [`NoopPlaygroundDispatcher`] — default in every build. Every
///   method returns [`DispatchError::Unavailable`] so the handler
///   path fires its local-shell fallback. Zero dep graph cost.
/// * `datashuttle_cloud::playground::TcpPlaygroundDispatcher` — cloud
///   build. Holds `sqlx::PgPool` + `mysql_async::Pool` and talks to
///   the sidecar containers directly.
///
/// The trait is object-safe (every method takes `&self`, has concrete
/// arg/return types, no generics) so `Arc<dyn PlaygroundDispatcher>`
/// works at runtime on `AppState`.
#[async_trait]
pub trait PlaygroundDispatcher: Send + Sync + std::fmt::Debug {
    /// Execute SQL against the postgres playground sidecar.
    async fn exec_postgres(&self, sql: &str) -> Result<(String, String), DispatchError>;

    /// Schema-scoped postgres execute — prepends `SET LOCAL search_path`
    /// so unqualified table references resolve inside the session's
    /// private schema.
    async fn exec_postgres_in_schema(
        &self,
        schema: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError>;

    /// Execute SQL against the mysql playground sidecar.
    async fn exec_mysql(&self, sql: &str) -> Result<(String, String), DispatchError>;

    /// Database-scoped mysql execute — issues `USE <db>` before the
    /// user SQL.
    async fn exec_mysql_in_database(
        &self,
        db: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError>;

    /// Reachability probe used by smoke endpoints + orphan sweeper.
    async fn ping_postgres(&self) -> Result<(), DispatchError>;

    async fn ping_mysql(&self) -> Result<(), DispatchError>;

    /// Create an isolated postgres schema for a playground session.
    /// Idempotent — repeated calls with the same name succeed.
    async fn provision_postgres_schema(&self, name: &str) -> Result<(), DispatchError>;

    /// Drop a playground schema + every table inside it. Idempotent.
    async fn teardown_postgres_schema(&self, name: &str) -> Result<(), DispatchError>;

    /// Drop a publication owned by this session.
    async fn drop_postgres_publication(&self, name: &str) -> Result<(), DispatchError>;

    /// Drop a logical replication slot for this session. Callers must
    /// have already quiesced the logical decoder (i.e. dropped the
    /// shuttle) before invoking.
    async fn drop_postgres_replication_slot(&self, name: &str) -> Result<(), DispatchError>;

    /// Enumerate session-owned publications (orphan sweeper input).
    async fn list_postgres_playground_publications(&self) -> Result<Vec<String>, DispatchError>;

    /// Enumerate session-owned replication slots.
    async fn list_postgres_playground_replication_slots(
        &self,
    ) -> Result<Vec<String>, DispatchError>;

    /// Enumerate `playground_*`-prefixed postgres schemas.
    async fn list_postgres_playground_schemas(&self) -> Result<Vec<String>, DispatchError>;

    /// Create an isolated mysql database for a playground session.
    async fn provision_mysql_database(&self, name: &str) -> Result<(), DispatchError>;

    async fn teardown_mysql_database(&self, name: &str) -> Result<(), DispatchError>;

    async fn list_mysql_playground_databases(&self) -> Result<Vec<String>, DispatchError>;

    /// Execute SQL against the clickhouse playground sidecar over HTTP
    /// (default port 8123). ClickHouse is multi-statement-aware via the
    /// `query` URL param, so the dispatcher submits the entire body as
    /// one POST.
    async fn exec_clickhouse(&self, sql: &str) -> Result<(String, String), DispatchError> {
        let _ = sql;
        Err(DispatchError::Unavailable)
    }

    /// Database-scoped clickhouse execute — prepends `USE <db>;` so
    /// unqualified table refs resolve in the session's private DB.
    async fn exec_clickhouse_in_database(
        &self,
        db: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        let _ = (db, sql);
        Err(DispatchError::Unavailable)
    }

    async fn ping_clickhouse(&self) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }

    /// Create an isolated clickhouse database for a playground session.
    async fn provision_clickhouse_database(&self, name: &str) -> Result<(), DispatchError> {
        let _ = name;
        Err(DispatchError::Unavailable)
    }

    async fn teardown_clickhouse_database(&self, name: &str) -> Result<(), DispatchError> {
        let _ = name;
        Err(DispatchError::Unavailable)
    }

    async fn list_clickhouse_playground_databases(&self) -> Result<Vec<String>, DispatchError> {
        Err(DispatchError::Unavailable)
    }

    /// True iff the impl is the cloud-backed TCP dispatcher. Handlers
    /// use this to refuse shell-exec fallbacks in cloud deployments
    /// (Phase 10.B.6 invariant): the cloud build MUST NOT reach a
    /// `docker compose exec` for postgres/mysql. The Noop default
    /// returns `false`; only the concrete TCP impl in
    /// `datashuttle-cloud` overrides to `true`.
    fn is_tcp_backed(&self) -> bool {
        false
    }
}

/// OSS default — every dispatcher method returns
/// [`DispatchError::Unavailable`] so the handler path falls back to
/// its local-shell branch. Zero runtime cost and zero dep-graph cost.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPlaygroundDispatcher;

#[async_trait]
impl PlaygroundDispatcher for NoopPlaygroundDispatcher {
    async fn exec_postgres(&self, _sql: &str) -> Result<(String, String), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn exec_postgres_in_schema(
        &self,
        _schema: &str,
        _sql: &str,
    ) -> Result<(String, String), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn exec_mysql(&self, _sql: &str) -> Result<(String, String), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn exec_mysql_in_database(
        &self,
        _db: &str,
        _sql: &str,
    ) -> Result<(String, String), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn ping_postgres(&self) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn ping_mysql(&self) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn provision_postgres_schema(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn teardown_postgres_schema(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn drop_postgres_publication(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn drop_postgres_replication_slot(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn list_postgres_playground_publications(&self) -> Result<Vec<String>, DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn list_postgres_playground_replication_slots(
        &self,
    ) -> Result<Vec<String>, DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn list_postgres_playground_schemas(&self) -> Result<Vec<String>, DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn provision_mysql_database(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn teardown_mysql_database(&self, _name: &str) -> Result<(), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    async fn list_mysql_playground_databases(&self) -> Result<Vec<String>, DispatchError> {
        Err(DispatchError::Unavailable)
    }
}

/// Validate a playground resource name (schema / database).
/// Matches the shape of `derive_namespace` in `playground_sessions.rs`
/// plus a hard length cap; anything that would escape the identifier
/// position fails closed.
pub fn is_safe_resource_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.starts_with("playground_")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate that `s` matches the shape of a playground-session-owned
/// postgres artifact (publication or replication slot). Names follow
/// the pattern `pg_<8hex>_<8hex>[_(pub|slot)]` — see
/// `SessionManager::new` in `playground_sessions.rs`.
pub fn is_safe_playground_shuttle_artifact(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 || !s.starts_with("pg_") {
        return false;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    // pg_<hex>_<hex> with optional _pub/_slot suffix — 3 or 4 parts.
    let parts: Vec<&str> = s.split('_').collect();
    match parts.as_slice() {
        ["pg", a, b] => !a.is_empty() && !b.is_empty(),
        ["pg", a, b, tail] => !a.is_empty() && !b.is_empty() && matches!(*tail, "pub" | "slot"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_error_messages_are_stable() {
        // Lock the surface so handler-side string matching doesn't drift
        // silently. The handler maps these to ActionResponse.error.
        assert_eq!(
            DispatchError::DuplicateKey("k=1".into()).to_string(),
            "duplicate key: k=1"
        );
        assert_eq!(
            DispatchError::SchemaMismatch("no col".into()).to_string(),
            "schema mismatch: no col"
        );
        assert_eq!(
            DispatchError::Timeout(30).to_string(),
            "query timed out after 30s"
        );
        assert_eq!(
            DispatchError::Unavailable.to_string(),
            "playground dispatcher unavailable in this build (saas feature off)"
        );
    }

    #[test]
    fn shuttle_artifact_validator_accepts_derived_names() {
        assert!(is_safe_playground_shuttle_artifact("pg_abcdef12_34567890"));
        assert!(is_safe_playground_shuttle_artifact(
            "pg_abcdef12_34567890_pub"
        ));
        assert!(is_safe_playground_shuttle_artifact(
            "pg_abcdef12_34567890_slot"
        ));
    }

    #[test]
    fn shuttle_artifact_validator_rejects_injection_attempts() {
        for bad in [
            "",
            "datashuttle_pub",            // legacy shared name
            "operator_own_pub",           // unrelated operator pub
            "pg_abc\"; DROP PUBLICATION", // quote injection
            "pg_abc_; SELECT 1",          // semicolon injection
            "pg_abc def",                 // whitespace
            "pg_abc_xyz_other",           // bad suffix
            "abcdef12_34567890_pub",      // missing `pg_` prefix
        ] {
            assert!(
                !is_safe_playground_shuttle_artifact(bad),
                "must reject `{bad}`"
            );
        }
    }

    #[test]
    fn is_safe_resource_name_smoke() {
        assert!(is_safe_resource_name("playground_deadbeef_00001"));
        assert!(!is_safe_resource_name(""));
        assert!(!is_safe_resource_name("no_prefix"));
        assert!(!is_safe_resource_name("playground_; DROP"));
    }

    #[tokio::test]
    async fn noop_dispatcher_returns_unavailable_for_every_method() {
        let d = NoopPlaygroundDispatcher;
        assert!(matches!(
            d.exec_postgres("SELECT 1").await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(matches!(
            d.exec_mysql("SELECT 1").await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(matches!(
            d.ping_postgres().await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(matches!(
            d.ping_mysql().await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(matches!(
            d.list_postgres_playground_schemas().await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(matches!(
            d.list_mysql_playground_databases().await.unwrap_err(),
            DispatchError::Unavailable
        ));
        assert!(!d.is_tcp_backed());
    }
}
