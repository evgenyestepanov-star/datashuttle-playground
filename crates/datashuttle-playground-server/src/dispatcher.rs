//! Concrete playground TCP dispatcher.
//!
//! Cloud-only impl of the [`PlaygroundDispatcher`] trait. The trait +
//! `DispatchError` + safety validators + pool consts live next door in
//! `datashuttle_playground::tcp`; this module is the bridge to the live
//! source databases (Postgres + MySQL playground sidecars) and keeps
//! the sqlx + mysql_async dep graph isolated from the foundation lib.
//!
//! Connection inputs (host/port/user/db) come from environment
//! variables injected by the cloud compose:
//!
//!   * `DS_PG_PLAYGROUND_HOST`, `DS_PG_PLAYGROUND_PORT`,
//!     `DS_PG_PLAYGROUND_USER`, `DS_PG_PLAYGROUND_DB`
//!   * `DS_MYSQL_PLAYGROUND_HOST`, `DS_MYSQL_PLAYGROUND_PORT`,
//!     `DS_MYSQL_PLAYGROUND_USER`, `DS_MYSQL_PLAYGROUND_DB`
//!
//! Passwords are loaded from Docker secrets at
//! `/run/secrets/{pg,mysql}_playground_password` with env-var
//! fallback (`DS_PG_PLAYGROUND_PASSWORD` /
//! `DS_MYSQL_PLAYGROUND_PASSWORD`) for local-dev / testcontainers.
//!
//! Pools are bounded at `POOL_MAX_CONNECTIONS` each and lazily
//! initialised on first use — so even a deploy that never invokes a
//! `target=source` action pays zero overhead on boot.
//!
//! Phase 5.C of the architecture-simplification epic moved this module
//! out of `datashuttle-cloud::playground` (where it lived after Phase 1
//! of cloud-extraction, #1051) into the standalone playground-server
//! binary. The cloud crate is now playground-free; the api edge serves
//! `/api/v1/playground/*` strictly through its reverse-proxy.

use std::time::Duration;

use async_trait::async_trait;
use datashuttle_playground::tcp::{
    is_safe_playground_shuttle_artifact, is_safe_resource_name, DispatchError,
    PlaygroundDispatcher, POOL_ACQUIRE_TIMEOUT_SECS, POOL_MAX_CONNECTIONS, STATEMENT_TIMEOUT_SECS,
};
use tokio::sync::OnceCell;

/// Per-process dispatcher with one cached pool per protocol.
///
/// Construct once during boot (via [`build_dispatcher`]) and share via
/// `Arc<dyn PlaygroundDispatcher>` on `ServerState`. `OnceCell` gives
/// us "init exactly once on first use" without further locking —
/// callers don't pay the connection cost until the first
/// `exec_postgres` / `exec_mysql`.
#[derive(Debug, Default)]
pub struct TcpPlaygroundDispatcher {
    pg: OnceCell<sqlx::PgPool>,
    mysql: OnceCell<mysql_async::Pool>,
    clickhouse: OnceCell<ClickhouseConfig>,
    redis: OnceCell<RedisConfig>,
}

#[derive(Debug, Clone)]
struct ClickhouseConfig {
    base_url: String,
    user: String,
    password: String,
    default_db: String,
    client: reqwest::Client,
}

/// Cached redis playground client. `MultiplexedConnection` shares one
/// underlying TCP socket across concurrent users — we mint per-exec
/// clones via `clone()` rather than holding a Mutex.
#[derive(Debug, Clone)]
struct RedisConfig {
    /// AUTH password. Empty = no AUTH.
    password: String,
    /// Cached client. Held by Arc so clones are cheap.
    client: std::sync::Arc<redis::Client>,
}

impl TcpPlaygroundDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    async fn exec_postgres_inner(
        &self,
        schema: Option<&str>,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        use futures::TryStreamExt;
        use sqlx::Executor;
        let pool = self.pg.get_or_try_init(build_pg_pool).await?;
        let mut tx = pool.begin().await.map_err(map_pg_error)?;
        tx.execute(sqlx::query(&format!(
            "SET LOCAL statement_timeout = '{}s'",
            STATEMENT_TIMEOUT_SECS
        )))
        .await
        .map_err(map_pg_error)?;
        if let Some(schema) = schema {
            // Schema already validated by the caller via
            // is_safe_resource_name, so splicing is safe.
            let stmt = format!("SET LOCAL search_path = \"{}\", public", schema);
            tx.execute(sqlx::query(&stmt))
                .await
                .map_err(map_pg_error)?;
        }
        let mut stream = sqlx::raw_sql(sql).execute_many(&mut *tx);
        let mut rows: u64 = 0;
        while let Some(result) = stream.try_next().await.map_err(map_pg_error)? {
            rows = rows.saturating_add(result.rows_affected());
        }
        drop(stream);
        tx.commit().await.map_err(map_pg_error)?;
        Ok((format!("OK ({rows} rows)"), String::new()))
    }

    async fn exec_mysql_inner(
        &self,
        db: Option<&str>,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        use mysql_async::prelude::Queryable;
        let pool = self.mysql.get_or_try_init(build_mysql_pool).await?;
        let mut conn = pool.get_conn().await.map_err(map_mysql_error)?;
        conn.query_drop(format!(
            "SET SESSION MAX_EXECUTION_TIME = {}",
            STATEMENT_TIMEOUT_SECS * 1000
        ))
        .await
        .map_err(map_mysql_error)?;
        if let Some(db) = db {
            let stmt = format!("USE `{}`", db);
            conn.query_drop(stmt).await.map_err(map_mysql_error)?;
        }
        conn.query_drop(sql).await.map_err(map_mysql_error)?;
        let rows = conn.affected_rows();
        drop(conn);
        Ok((format!("OK ({rows} rows)"), String::new()))
    }

    async fn exec_clickhouse_inner(
        &self,
        db: Option<&str>,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        let cfg = self
            .clickhouse
            .get_or_try_init(build_clickhouse_config)
            .await?;
        // ClickHouse 24.x forbids multi-statement HTTP bodies by default
        // ("Multi-statements are not allowed"). Both the `?multiquery=1`
        // URL param and the `multi_statements` setting got renamed or
        // restricted across versions — the portable fix is to split the
        // body on `;` and POST each statement separately. Init scripts /
        // action SQL files routinely have 5–20 statements so the round-trip
        // cost is fine for a playground workload.
        // `db` arrived through `is_safe_resource_name` and `default_db`
        // is sourced from env — both are restricted to ASCII alnum + `_`,
        // which are URL-safe so no escaping needed.
        let target_db = db.unwrap_or(cfg.default_db.as_str());
        let url = format!(
            "{}/?max_execution_time={}&database={}",
            cfg.base_url, STATEMENT_TIMEOUT_SECS, target_db
        );
        let mut last_body = String::new();
        let mut stmt_count = 0usize;
        for stmt in split_clickhouse_statements(sql) {
            stmt_count += 1;
            let resp = cfg
                .client
                .post(&url)
                .basic_auth(&cfg.user, Some(&cfg.password))
                .body(stmt.clone())
                .send()
                .await
                .map_err(map_reqwest_error)?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(map_clickhouse_status(status, body));
            }
            last_body = body;
        }
        if stmt_count == 0 {
            return Ok((String::new(), String::new()));
        }
        Ok((last_body, String::new()))
    }

    /// Execute a Redis command script. One command per non-blank,
    /// non-`#`-prefixed line; whitespace-split into argv. Per-session
    /// key isolation is the scenario author's responsibility — bake
    /// `{namespace}` into every key reference in your script and the
    /// handler's placeholder substitution resolves it before we see
    /// the body. No magic prefixing here so EVAL/Lua and MULTI/EXEC
    /// behave intuitively.
    async fn exec_redis_inner(
        &self,
        namespace: Option<&str>,
        script: &str,
    ) -> Result<(String, String), DispatchError> {
        // `namespace` is informational only — kept on the signature so
        // future commands that legitimately need to know the session
        // boundary (XINFO listing, namespace teardown sweep) can read
        // it without a trait churn. Today it's just an attribution
        // hint we log.
        let _ = namespace;
        let cfg = self.redis.get_or_try_init(build_redis_config).await?;
        let mut conn = cfg
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(map_redis_error)?;
        if !cfg.password.is_empty() {
            let _: redis::RedisResult<()> = redis::cmd("AUTH")
                .arg(&cfg.password)
                .query_async(&mut conn)
                .await;
        }
        let mut count = 0usize;
        let mut last_reply = String::new();
        for raw in script.lines() {
            let line = raw.split('#').next().unwrap_or(raw).trim();
            if line.is_empty() {
                continue;
            }
            let tokens = tokenize_redis_line(line);
            if tokens.is_empty() {
                continue;
            }
            let mut cmd = redis::cmd(&tokens[0]);
            for tok in tokens.iter().skip(1) {
                cmd.arg(tok.as_str());
            }
            let reply: redis::Value =
                cmd.query_async(&mut conn).await.map_err(map_redis_error)?;
            count += 1;
            last_reply = format!("{reply:?}");
        }
        Ok((format!("OK ({count} commands)\n{last_reply}"), String::new()))
    }
}

/// Cheap shell-style tokenizer for Redis scripts. Honours single + double
/// quotes so `XADD events * type "purchase order" amount 1.50` keeps
/// the spaced value intact. Anything fancier (escapes, nested quotes)
/// is out of scope — playground scripts are authored by us.
fn tokenize_redis_line(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for c in line.chars() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ws if ws.is_whitespace() && !in_single && !in_double => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            other => buf.push(other),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Split a SQL body on top-level `;` boundaries while honouring `'…'`
/// and `"…"` string literals and `--` line comments. ClickHouse seed
/// files use `'…'` extensively for arrayElement string lists — naive
/// `split(';')` would split inside those.
fn split_clickhouse_statements(sql: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        // Skip `-- ...` line comments outside quotes.
        if !in_single && !in_double && c == '-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                current.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(c);
            }
            ';' if !in_single && !in_double => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    out.push(trimmed);
                }
                current.clear();
            }
            other => current.push(other),
        }
        i += 1;
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

#[async_trait]
impl PlaygroundDispatcher for TcpPlaygroundDispatcher {
    async fn exec_postgres(&self, sql: &str) -> Result<(String, String), DispatchError> {
        self.exec_postgres_inner(None, sql).await
    }

    async fn exec_postgres_in_schema(
        &self,
        schema: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        if !is_safe_resource_name(schema) {
            return Err(DispatchError::Config(format!(
                "unsafe postgres schema name: {schema}"
            )));
        }
        self.exec_postgres_inner(Some(schema), sql).await
    }

    async fn exec_mysql(&self, sql: &str) -> Result<(String, String), DispatchError> {
        self.exec_mysql_inner(None, sql).await
    }

    async fn exec_mysql_in_database(
        &self,
        db: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        if !is_safe_resource_name(db) {
            return Err(DispatchError::Config(format!(
                "unsafe mysql database name: {db}"
            )));
        }
        self.exec_mysql_inner(Some(db), sql).await
    }

    async fn ping_postgres(&self) -> Result<(), DispatchError> {
        let _ = self.exec_postgres("SELECT 1").await?;
        Ok(())
    }

    async fn ping_mysql(&self) -> Result<(), DispatchError> {
        let _ = self.exec_mysql("SELECT 1").await?;
        Ok(())
    }

    async fn provision_postgres_schema(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe postgres schema name: {name}"
            )));
        }
        let ddl = format!("CREATE SCHEMA IF NOT EXISTS \"{name}\"");
        self.exec_postgres(&ddl).await.map(|_| ())
    }

    async fn teardown_postgres_schema(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe postgres schema name: {name}"
            )));
        }
        let ddl = format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE");
        self.exec_postgres(&ddl).await.map(|_| ())
    }

    async fn drop_postgres_publication(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_playground_shuttle_artifact(name) {
            return Err(DispatchError::Config(format!(
                "unsafe postgres publication name: {name}"
            )));
        }
        let ddl = format!("DROP PUBLICATION IF EXISTS \"{name}\"");
        self.exec_postgres(&ddl).await.map(|_| ())
    }

    async fn drop_postgres_replication_slot(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_playground_shuttle_artifact(name) {
            return Err(DispatchError::Config(format!(
                "unsafe postgres replication slot name: {name}"
            )));
        }
        // `pg_drop_replication_slot` errors on a missing slot, so
        // guard with a SELECT first. Two roundtrips but teardown is
        // not hot.
        let sql = format!(
            "DO $$ BEGIN \
                 IF EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{name}') THEN \
                     PERFORM pg_drop_replication_slot('{name}'); \
                 END IF; \
             END $$"
        );
        self.exec_postgres(&sql).await.map(|_| ())
    }

    async fn list_postgres_playground_publications(&self) -> Result<Vec<String>, DispatchError> {
        use sqlx::Row;
        let pool = self.pg.get_or_try_init(build_pg_pool).await?;
        let rows = sqlx::query(
            "SELECT pubname FROM pg_publication \
             WHERE pubname LIKE 'pg\\_%\\_pub' ESCAPE '\\'",
        )
        .fetch_all(pool)
        .await
        .map_err(map_pg_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn list_postgres_playground_replication_slots(
        &self,
    ) -> Result<Vec<String>, DispatchError> {
        use sqlx::Row;
        let pool = self.pg.get_or_try_init(build_pg_pool).await?;
        let rows = sqlx::query(
            "SELECT slot_name FROM pg_replication_slots \
             WHERE slot_name LIKE 'pg\\_%\\_slot' ESCAPE '\\'",
        )
        .fetch_all(pool)
        .await
        .map_err(map_pg_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn list_postgres_playground_schemas(&self) -> Result<Vec<String>, DispatchError> {
        use sqlx::Row;
        let pool = self.pg.get_or_try_init(build_pg_pool).await?;
        let rows = sqlx::query(
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name LIKE 'playground\\_%' ESCAPE '\\'",
        )
        .fetch_all(pool)
        .await
        .map_err(map_pg_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn provision_mysql_database(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe mysql database name: {name}"
            )));
        }
        let ddl = format!("CREATE DATABASE IF NOT EXISTS `{name}`");
        self.exec_mysql(&ddl).await.map(|_| ())
    }

    async fn teardown_mysql_database(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe mysql database name: {name}"
            )));
        }
        let ddl = format!("DROP DATABASE IF EXISTS `{name}`");
        self.exec_mysql(&ddl).await.map(|_| ())
    }

    async fn list_mysql_playground_databases(&self) -> Result<Vec<String>, DispatchError> {
        use mysql_async::prelude::Queryable;
        let pool = self.mysql.get_or_try_init(build_mysql_pool).await?;
        let mut conn = pool.get_conn().await.map_err(map_mysql_error)?;
        let rows: Vec<String> = conn
            .query(
                "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA \
                 WHERE SCHEMA_NAME LIKE 'playground\\_%'",
            )
            .await
            .map_err(map_mysql_error)?;
        drop(conn);
        Ok(rows)
    }

    async fn exec_clickhouse(&self, sql: &str) -> Result<(String, String), DispatchError> {
        self.exec_clickhouse_inner(None, sql).await
    }

    async fn exec_clickhouse_in_database(
        &self,
        db: &str,
        sql: &str,
    ) -> Result<(String, String), DispatchError> {
        if !is_safe_resource_name(db) {
            return Err(DispatchError::Config(format!(
                "unsafe clickhouse database name: {db}"
            )));
        }
        self.exec_clickhouse_inner(Some(db), sql).await
    }

    async fn ping_clickhouse(&self) -> Result<(), DispatchError> {
        let _ = self.exec_clickhouse("SELECT 1").await?;
        Ok(())
    }

    async fn provision_clickhouse_database(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe clickhouse database name: {name}"
            )));
        }
        let ddl = format!("CREATE DATABASE IF NOT EXISTS `{name}`");
        self.exec_clickhouse(&ddl).await.map(|_| ())
    }

    async fn teardown_clickhouse_database(&self, name: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(name) {
            return Err(DispatchError::Config(format!(
                "unsafe clickhouse database name: {name}"
            )));
        }
        let ddl = format!("DROP DATABASE IF EXISTS `{name}`");
        self.exec_clickhouse(&ddl).await.map(|_| ())
    }

    async fn list_clickhouse_playground_databases(&self) -> Result<Vec<String>, DispatchError> {
        let (body, _) = self
            .exec_clickhouse(
                "SELECT name FROM system.databases \
                 WHERE name LIKE 'playground\\_%' FORMAT TabSeparated",
            )
            .await?;
        Ok(body
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    async fn exec_redis(&self, script: &str) -> Result<(String, String), DispatchError> {
        self.exec_redis_inner(None, script).await
    }

    async fn exec_redis_in_namespace(
        &self,
        namespace: &str,
        script: &str,
    ) -> Result<(String, String), DispatchError> {
        if !is_safe_resource_name(namespace) {
            return Err(DispatchError::Config(format!(
                "unsafe redis namespace: {namespace}"
            )));
        }
        self.exec_redis_inner(Some(namespace), script).await
    }

    async fn ping_redis(&self) -> Result<(), DispatchError> {
        let cfg = self.redis.get_or_try_init(build_redis_config).await?;
        let mut conn = cfg
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(map_redis_error)?;
        let _: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(map_redis_error)?;
        Ok(())
    }

    async fn teardown_redis_namespace(&self, namespace: &str) -> Result<(), DispatchError> {
        if !is_safe_resource_name(namespace) {
            return Err(DispatchError::Config(format!(
                "unsafe redis namespace: {namespace}"
            )));
        }
        let cfg = self.redis.get_or_try_init(build_redis_config).await?;
        let mut conn = cfg
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(map_redis_error)?;
        // SCAN through the prefix and DEL in chunks of 100. Avoids
        // KEYS which would block the server for many keys, and avoids
        // FLUSHDB which would wipe other sessions.
        let pattern = format!("{namespace}:*");
        let mut cursor: u64 = 0;
        loop {
            let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(200)
                .query_async(&mut conn)
                .await
                .map_err(map_redis_error)?;
            if !keys.is_empty() {
                let _: i64 = redis::cmd("DEL")
                    .arg(&keys)
                    .query_async(&mut conn)
                    .await
                    .map_err(map_redis_error)?;
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
        Ok(())
    }

    fn is_tcp_backed(&self) -> bool {
        true
    }
}

/// Build the dispatcher stashed on `ServerState` at boot.
///
/// Zero-cost at construction — the pools inside lazily initialise on
/// first use.
pub fn build_dispatcher() -> TcpPlaygroundDispatcher {
    TcpPlaygroundDispatcher::new()
}

// ── Pool builders + secret / env helpers ────────────────────────────────────

async fn build_pg_pool() -> Result<sqlx::PgPool, DispatchError> {
    let host = env_or("DS_PG_PLAYGROUND_HOST", "postgres-playground");
    let port = env_u16("DS_PG_PLAYGROUND_PORT", 5432)?;
    let user = env_or("DS_PG_PLAYGROUND_USER", "playground_admin");
    let db = env_or("DS_PG_PLAYGROUND_DB", "playground");
    let password = load_secret(
        "DS_PG_PLAYGROUND_PASSWORD",
        "/run/secrets/pg_playground_password",
    )?;
    let password = url_encode(&password);
    let url = format!("postgres://{user}:{password}@{host}:{port}/{db}?sslmode=disable");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(POOL_MAX_CONNECTIONS)
        .acquire_timeout(Duration::from_secs(POOL_ACQUIRE_TIMEOUT_SECS))
        .connect(&url)
        .await
        .map_err(map_pg_error)
}

async fn build_clickhouse_config() -> Result<ClickhouseConfig, DispatchError> {
    let host = env_or("DS_CLICKHOUSE_PLAYGROUND_HOST", "clickhouse-playground");
    let port = env_u16("DS_CLICKHOUSE_PLAYGROUND_PORT", 8123)?;
    let user = env_or("DS_CLICKHOUSE_PLAYGROUND_USER", "playground");
    let default_db = env_or("DS_CLICKHOUSE_PLAYGROUND_DB", "playground");
    let password = load_secret(
        "DS_CLICKHOUSE_PLAYGROUND_PASSWORD",
        "/run/secrets/clickhouse_playground_password",
    )?;
    let base_url = format!("http://{host}:{port}");
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(POOL_ACQUIRE_TIMEOUT_SECS))
        .timeout(Duration::from_secs(STATEMENT_TIMEOUT_SECS + 5))
        .pool_max_idle_per_host(POOL_MAX_CONNECTIONS as usize)
        .build()
        .map_err(|e| DispatchError::Config(format!("reqwest builder: {e}")))?;
    Ok(ClickhouseConfig {
        base_url,
        user,
        password,
        default_db,
        client,
    })
}

async fn build_redis_config() -> Result<RedisConfig, DispatchError> {
    let host = env_or("DS_REDIS_PLAYGROUND_HOST", "redis-playground");
    let port = env_u16("DS_REDIS_PLAYGROUND_PORT", 6379)?;
    let db = env_u16("DS_REDIS_PLAYGROUND_DB", 0)?;
    // Redis playground is optional + auth-less by default; tolerate
    // missing secret file + env.
    let password = match load_secret(
        "DS_REDIS_PLAYGROUND_PASSWORD",
        "/run/secrets/redis_playground_password",
    ) {
        Ok(p) => p,
        Err(_) => String::new(),
    };
    let url = format!("redis://{host}:{port}/{db}");
    let client = redis::Client::open(url)
        .map_err(|e| DispatchError::Config(format!("redis client: {e}")))?;
    Ok(RedisConfig {
        password,
        client: std::sync::Arc::new(client),
    })
}

async fn build_mysql_pool() -> Result<mysql_async::Pool, DispatchError> {
    let host = env_or("DS_MYSQL_PLAYGROUND_HOST", "mysql-playground");
    let port = env_u16("DS_MYSQL_PLAYGROUND_PORT", 3306)?;
    let user = env_or("DS_MYSQL_PLAYGROUND_USER", "playground_admin");
    let db = env_or("DS_MYSQL_PLAYGROUND_DB", "playground");
    let password = load_secret(
        "DS_MYSQL_PLAYGROUND_PASSWORD",
        "/run/secrets/mysql_playground_password",
    )?;
    let opts = mysql_async::OptsBuilder::default()
        .ip_or_hostname(host)
        .tcp_port(port)
        .user(Some(user))
        .pass(Some(password))
        .db_name(Some(db));
    let constraints = mysql_async::PoolConstraints::new(0, POOL_MAX_CONNECTIONS as usize)
        .ok_or_else(|| DispatchError::Config("invalid pool constraints".into()))?;
    let pool_opts = mysql_async::PoolOpts::default().with_constraints(constraints);
    Ok(mysql_async::Pool::new(opts.pool_opts(pool_opts)))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_u16(key: &str, default: u16) -> Result<u16, DispatchError> {
    match std::env::var(key) {
        Ok(v) => v
            .parse::<u16>()
            .map_err(|e| DispatchError::Config(format!("{key}={v}: {e}"))),
        Err(_) => Ok(default),
    }
}

/// Load a secret from `/run/secrets/...` first; fall back to env var
/// if the file is absent. Docker secrets win because the file mode is
/// `0400` and only the api process can read it, while env vars leak
/// via `docker inspect` and `/proc/<pid>/environ`.
fn load_secret(env_key: &str, secret_path: &str) -> Result<String, DispatchError> {
    if std::path::Path::new(secret_path).exists() {
        let raw = std::fs::read_to_string(secret_path)
            .map_err(|e| DispatchError::Config(format!("read {secret_path}: {e}")))?;
        return Ok(raw.trim().to_string());
    }
    std::env::var(env_key).map_err(|_| {
        DispatchError::Config(format!(
            "playground password not found at {secret_path} and ${env_key} unset"
        ))
    })
}

/// Percent-encode characters that would break the `postgres://` URI
/// parser. Covers the printable ASCII the openssl password generator
/// produces plus any future symbol-bearing rotation.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for &b in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

fn map_pg_error(e: sqlx::Error) -> DispatchError {
    use sqlx::Error as E;
    match e {
        E::Database(db_err) => {
            // Postgres SQLSTATEs we care about for clean playground UX.
            // 23505 unique_violation, 42P01 undefined_table, 42703
            // undefined_column, 42601 syntax_error.
            let code = db_err.code().map(|c| c.into_owned()).unwrap_or_default();
            let msg = db_err.message().to_string();
            match code.as_str() {
                "23505" => DispatchError::DuplicateKey(msg),
                "42P01" | "42703" => DispatchError::SchemaMismatch(msg),
                "28P01" | "28000" => DispatchError::Auth(msg),
                "57014" => DispatchError::Timeout(STATEMENT_TIMEOUT_SECS),
                _ => DispatchError::Protocol(format!("[{code}] {msg}")),
            }
        }
        E::Io(io_err) => DispatchError::Connect(io_err.to_string()),
        E::PoolTimedOut => DispatchError::Timeout(POOL_ACQUIRE_TIMEOUT_SECS),
        E::PoolClosed => DispatchError::Connect("pool closed".into()),
        other => DispatchError::Protocol(other.to_string()),
    }
}

fn map_reqwest_error(e: reqwest::Error) -> DispatchError {
    if e.is_timeout() {
        DispatchError::Timeout(STATEMENT_TIMEOUT_SECS)
    } else if e.is_connect() {
        DispatchError::Connect(e.to_string())
    } else {
        DispatchError::Protocol(e.to_string())
    }
}

/// Map a non-2xx clickhouse HTTP response to a [`DispatchError`].
/// ClickHouse returns the error code as a `X-ClickHouse-Exception-Code`
/// header but the response body also leads with `Code: NNN. DB::Exception:`
/// — we parse the body since reqwest already gives it back to us.
fn map_clickhouse_status(status: reqwest::StatusCode, body: String) -> DispatchError {
    // Examples:
    //   Code: 60. DB::Exception: Table x doesn't exist.
    //   Code: 81. DB::Exception: Database y doesn't exist.
    //   Code: 62. DB::Exception: Syntax error.
    let code = body
        .strip_prefix("Code: ")
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.trim().parse::<u32>().ok());
    let msg = body
        .lines()
        .next()
        .map(|l| l.to_string())
        .unwrap_or_else(|| status.to_string());
    match code {
        Some(60) | Some(81) | Some(47) => DispatchError::SchemaMismatch(msg),
        Some(62) | Some(63) => DispatchError::Protocol(format!("syntax: {msg}")),
        Some(159) => DispatchError::Timeout(STATEMENT_TIMEOUT_SECS),
        Some(516) | Some(192) => DispatchError::Auth(msg),
        _ => {
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                DispatchError::Auth(msg)
            } else {
                DispatchError::Protocol(format!("[{}] {msg}", status.as_u16()))
            }
        }
    }
}

fn map_redis_error(e: redis::RedisError) -> DispatchError {
    use redis::ErrorKind as K;
    let msg = e.to_string();
    match e.kind() {
        K::AuthenticationFailed => DispatchError::Auth(msg),
        K::IoError | K::ClientError => DispatchError::Connect(msg),
        K::TypeError | K::ResponseError => DispatchError::Protocol(msg),
        K::ExtensionError => DispatchError::Protocol(msg),
        _ => DispatchError::Protocol(msg),
    }
}

fn map_mysql_error(e: mysql_async::Error) -> DispatchError {
    use mysql_async::Error as E;
    match e {
        E::Server(srv) => {
            let msg = srv.message.clone();
            match srv.code {
                1062 => DispatchError::DuplicateKey(msg), // ER_DUP_ENTRY
                1146 | 1054 => DispatchError::SchemaMismatch(msg), // no such table / column
                1045 | 1044 | 1698 => DispatchError::Auth(msg), // access denied
                1064 => DispatchError::Protocol(format!("syntax: {msg}")),
                1969 | 3024 => DispatchError::Timeout(STATEMENT_TIMEOUT_SECS),
                code => DispatchError::Protocol(format!("[{code}] {msg}")),
            }
        }
        E::Driver(drv) => DispatchError::Protocol(drv.to_string()),
        E::Io(io) => DispatchError::Connect(io.to_string()),
        E::Url(u) => DispatchError::Config(u.to_string()),
        other => DispatchError::Protocol(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_is_safe_for_random_hex() {
        // openssl rand -hex 24 produces only 0-9a-f — must be
        // untouched.
        let pw = "deadbeefcafebabe1234";
        assert_eq!(url_encode(pw), pw);
    }

    #[test]
    fn url_encode_escapes_uri_specials() {
        assert_eq!(url_encode("p@ss:wor/d"), "p%40ss%3Awor%2Fd");
    }

    #[tokio::test]
    async fn tcp_dispatcher_is_tcp_backed() {
        let d = TcpPlaygroundDispatcher::new();
        assert!(d.is_tcp_backed());
    }
}
