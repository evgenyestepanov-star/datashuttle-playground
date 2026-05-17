# Technology Stack

**Analysis Date:** 2026-05-17

## Languages

**Primary:**
- Rust 2021 edition (MSRV 1.82, builder image pins 1.94) — entire codebase

## Runtime

**Environment:**
- Tokio async runtime (full feature set) — `tokio = "1"` workspace dep
- Single binary: `datashuttle-playground-server`

**Package Manager:**
- Cargo (workspace with Cargo.lock present)
- Lockfile: present (`Cargo.lock` committed, version 3)

## Frameworks

**HTTP server:**
- Axum 0.7 (with WebSocket feature) — `axum = { version = "0.7", features = ["ws"] }`
  - Tower middleware stack: `tower-http = "0.5"` (trace + request-size limit)
  - `tower = "0.5"` for integration test service wrappers

**Testing:**
- Tokio test utils: `tokio = { features = ["test-util", "macros"] }` in dev-dependencies
- `tempfile = "3"` — used in `sessions` persistence round-trip tests

**Build/Dev:**
- Multi-stage Docker build: `rust:1.94-bookworm` builder → `debian:bookworm-slim` runtime
- `tini` as PID-1 init in the container
- Release profile: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`

## Key Dependencies

**Critical:**
- `axum 0.7` — HTTP surface (session lifecycle, manifest, health, metrics endpoints)
- `tokio 1` (full features) — async runtime, TCP listener, `OnceCell` for lazy pool init
- `sqlx 0.8` — PostgreSQL connection pool; features: `runtime-tokio`, `tls-rustls`, `postgres`, `macros`, `chrono`, `uuid`, `json`, `migrate`
- `mysql_async 0.34` — MySQL async connection pool; features: `default-rustls`
- `redis 0.27` — Redis Streams dispatcher; features: `tokio-comp`, `aio`, `streams` (MultiplexedConnection)
- `reqwest 0.12` — outbound HTTP to OSS api callbacks + ClickHouse HTTP dispatch; features: `json`, `rustls-tls-native-roots`
- `prometheus 0.13` (with `process` feature) — Prometheus metrics exposition at `/metrics`
- `serde 1` + `serde_json 1` — all serialisation (sessions persistence, manifest, API payloads)
- `tracing 0.1` + `tracing-subscriber 0.3` (env-filter + json) — structured JSON logging

**Infrastructure:**
- `chrono 0.4` (serde feature) — session TTL wall-clock timestamps
- `uuid 1` (v4 + serde) — session IDs
- `sha2 0.10` — deterministic namespace derivation (`playground_<sha256[:4]>_<uuid[:4]>`)
- `base64 0.22` — SQL shell dispatch encoding (source-side action payloads)
- `async-trait 0.1` — `PlaygroundDispatcher` trait object
- `anyhow 1` + `thiserror 2` — error handling

**Supply-chain audit:**
- `cargo-deny` configured in `deny.toml`: multiple crate versions = warn, wildcards = deny, licenses allow-list covers Apache-2.0/MIT/BSD/ISC/MPL-2.0

## Configuration

**Environment (all read at startup in `config.rs`):**
- `PLAYGROUND_BIND_ADDR` — default `0.0.0.0:8081`
- `PLAYGROUND_MANIFEST` — path to `manifest.json`; auto-discovered from `/opt/datashuttle/examples/manifest.json` if unset
- `PLAYGROUND_TOKEN` — inbound bearer token; server starts without auth if unset (dev mode warned in logs)
- `PLAYGROUND_TTL_SECS` — session TTL in seconds; default 7200 (2h); clamped 5m–8h
- `PLAYGROUND_QUOTA_PER_DAY` — per-tenant daily session creation limit; default 20
- `PLAYGROUND_API_BASE_URL` — OSS api callback target (e.g. `http://api:8080`); 503 returned for session/action endpoints if unset
- `PLAYGROUND_SERVICE_TOKEN` — bearer used for outbound OSS api callbacks
- `PLAYGROUND_API_TIMEOUT_SECS` — per-callback timeout; default 30s
- `PLAYGROUND_DEPLOYMENT` — deployment classification (`cloud` default); drives manifest visibility filters
- `PLAYGROUND_EXAMPLES_DIR` — scenario asset root; default `/opt/datashuttle/examples`
- `PLAYGROUND_DATA_DIR` — writable state dir for session persistence; default `/var/lib/datashuttle-playground`

**Source sidecar connections (all in `dispatcher.rs`):**

| Variable | Default | Purpose |
|---|---|---|
| `DS_PG_PLAYGROUND_HOST` | `postgres-playground` | PostgreSQL sidecar |
| `DS_PG_PLAYGROUND_PORT` | `5432` | |
| `DS_PG_PLAYGROUND_USER` | `playground_admin` | |
| `DS_PG_PLAYGROUND_DB` | `playground` | |
| `DS_PG_PLAYGROUND_PASSWORD` | `/run/secrets/pg_playground_password` | File wins over env |
| `DS_MYSQL_PLAYGROUND_HOST` | `mysql-playground` | MySQL sidecar |
| `DS_MYSQL_PLAYGROUND_PORT` | `3306` | |
| `DS_MYSQL_PLAYGROUND_USER` | `playground_admin` | |
| `DS_MYSQL_PLAYGROUND_DB` | `playground` | |
| `DS_MYSQL_PLAYGROUND_PASSWORD` | `/run/secrets/mysql_playground_password` | File wins over env |
| `DS_CLICKHOUSE_PLAYGROUND_HOST` | `clickhouse-playground` | ClickHouse sidecar |
| `DS_CLICKHOUSE_PLAYGROUND_PORT` | `8123` | HTTP interface |
| `DS_CLICKHOUSE_PLAYGROUND_USER` | `playground` | |
| `DS_CLICKHOUSE_PLAYGROUND_DB` | `playground` | |
| `DS_CLICKHOUSE_PLAYGROUND_PASSWORD` | `/run/secrets/clickhouse_playground_password` | File wins over env |
| `DS_REDIS_PLAYGROUND_HOST` | `redis-playground` | Redis sidecar |
| `DS_REDIS_PLAYGROUND_PORT` | `6379` | |
| `DS_REDIS_PLAYGROUND_DB` | `0` | Redis logical db |
| `DS_REDIS_PLAYGROUND_PASSWORD` | `/run/secrets/redis_playground_password` | Optional; empty = no AUTH |

**Secrets loading strategy:** Docker secrets file at `/run/secrets/<name>` takes priority over env var, preventing secret leakage via `docker inspect` / `/proc/<pid>/environ`.

**Build:**
- Workspace: `Cargo.toml` at project root
- Build config files: `Cargo.toml`, `Cargo.lock`, `clippy.toml` (`avoid-breaking-exported-api = false`), `rustfmt.toml` (`edition = "2021"`), `deny.toml`

## Platform Requirements

**Development:**
- Rust 1.82+ (MSRV); builder uses 1.94
- Docker Compose for source sidecars (postgres, mysql, clickhouse, redis, redpanda, cassandra, dynamodb-local, localstack, wiremock, toxiproxy)
- No `.env` file detected; all config via environment variables

**Production:**
- Containerised: single Docker image `datashuttle/playground:<tag>`
- Exposed port: `8081`
- Runtime user: `playground:playground` (non-root)
- Third-party CLIs bundled in image: `rpk` (Redpanda, v24.2.7) for Kafka topic creation; `mc` (MinIO client) for S3 file upload
- Helm chart available: `helm/` (ClusterIP service, liveness/readiness at `/health`)
- Kubernetes Secrets mount for `PLAYGROUND_TOKEN`
- Session state persisted to `<PLAYGROUND_DATA_DIR>/playground/sessions.json` (atomic write via temp+rename)

---

*Stack analysis: 2026-05-17*
