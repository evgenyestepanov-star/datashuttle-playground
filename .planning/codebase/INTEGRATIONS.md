# External Integrations

**Analysis Date:** 2026-05-17

## APIs & External Services

**OSS DataShuttle API (primary integration):**
- The playground server is a sidecar to the OSS DataShuttle api. It never talks to the Iceberg catalog, warehouses, or DataFusion engine directly.
- All pipeline operations are dispatched as HTTP callbacks over `reqwest` to the OSS api.
- Client: `crates/datashuttle-playground-server/src/api_client.rs` — `ApiClient`
- Base URL env var: `PLAYGROUND_API_BASE_URL` (e.g. `http://api:8080`)
- Auth: `Authorization: Bearer <PLAYGROUND_SERVICE_TOKEN>` + impersonation headers `X-Datashuttle-Impersonate-User-Id` / `X-Datashuttle-Impersonate-Tenant-Id`
- Supported statement types dispatched to typed api endpoints:
  - `CREATE CONNECTION ...` → `POST /api/v1/connections`
  - `CREATE SHUTTLE ...` → `POST /api/v1/shuttles`
  - `RESUME SHUTTLE <n>` → `POST /api/v1/shuttles/<n>/resume`
  - `PAUSE SHUTTLE <n>` → `POST /api/v1/shuttles/<n>/pause`
  - `DROP SHUTTLE <n>` → `DELETE /api/v1/shuttles/<n>`
  - `DROP CONNECTION <n>` → `DELETE /api/v1/connections/<n>`
- Timeout: `PLAYGROUND_API_TIMEOUT_SECS` (default 30s)
- Degraded-mode design: if `PLAYGROUND_API_BASE_URL` or `PLAYGROUND_SERVICE_TOKEN` is unset, `api_client` is `None` and only `/health`, `/metrics`, `/api/v1/health`, `/api/v1/manifest` remain usable; session/action endpoints return 503.

**OSS api reverse-proxy (inbound side):**
- The OSS api reverse-proxies `POST /api/v1/playground/*` to this binary after stripping the `/playground` path prefix. Playground server therefore sees paths like `/api/v1/sessions`, `/api/v1/manifest`, not `/api/v1/playground/sessions`.
- The OSS proxy injects `X-Datashuttle-User-Id`, `X-Datashuttle-Tenant-Id`, `X-Datashuttle-Actor-Id`, `X-Datashuttle-Auth-Method` headers; identity middleware (`src/identity.rs`) parses these.
- Inbound protection: shared bearer token `PLAYGROUND_TOKEN` (constant-time comparison in `router.rs`).

**Redpanda (Kafka) — `rpk` CLI integration:**
- For `produce-kafka` action kind, the handler shells out to `rpk` (Redpanda CLI v24.2.7, bundled in Docker image at `/usr/local/bin/rpk`) to create topics before the first Pandaproxy REST produce call.
- Pandaproxy REST endpoint: `http://redpanda:8082` (internal Docker network name)
- Relevant scenario examples: `kafka-json-poison`, `kafka-throughput`, `slow-consumer`

**MinIO (S3-compatible object store) — `mc` CLI integration:**
- For `upload-file` action kind, the handler shells out to `mc` (MinIO client, bundled in Docker image at `/usr/local/bin/mc`) to copy payload files into the cloud stack's MinIO bucket.
- Direct S3 sigv4 PUT via `reqwest` was deliberately avoided to prevent pulling `aws-sdk-s3` into the build graph.
- Relevant scenario: `file-s3-mixed-formats`, `file-bad-encoding`

**Apache Polaris (Iceberg REST catalog):**
- Not connected directly from playground server.
- Polaris is in the examples `docker-compose.yml` (`examples/docker-compose.yml`) as the Iceberg catalog backing DataShuttle. Playground sessions write Iceberg data into Polaris via the OSS api callback path.
- Connection config example: `DS_CATALOG_URI=http://polaris:8181/api/catalog`, `DS_CATALOG_CLIENT_ID=root`, `DS_CATALOG_CLIENT_SECRET=s3cr3t`

## Data Storage

**Databases (source sidecars — write-path, playground actions mutate these):**

| Database | Role | Connection crate |
|---|---|---|
| PostgreSQL 16 (`postgres-playground` sidecar) | CDC source — logical replication | `sqlx 0.8` via `PgPool` |
| MySQL 8.4 (`mysql-playground` sidecar) | CDC source — binlog replication | `mysql_async 0.34` |
| ClickHouse 24.8 (`clickhouse-playground` sidecar) | Snapshot source | `reqwest` HTTP to port 8123 |
| Redis 7 (`redis-playground` sidecar) | Streams source | `redis 0.27` (MultiplexedConnection) |

Pool settings (defined in `crates/datashuttle-playground/src/tcp.rs`):
- `POOL_MAX_CONNECTIONS = 8` per protocol
- `POOL_ACQUIRE_TIMEOUT_SECS = 10`
- `STATEMENT_TIMEOUT_SECS = 30`

All pools are lazily initialised via `tokio::sync::OnceCell` — unused protocol pays zero connection overhead at boot.

**Session persistence (local filesystem):**
- Path: `<PLAYGROUND_DATA_DIR>/playground/sessions.json` (default `/var/lib/datashuttle-playground/playground/sessions.json`)
- Written atomically (temp file + rename) after every create/update/end
- Hydrated at startup; expired sessions dropped during hydrate
- Exists in `crates/datashuttle-playground/src/sessions.rs` — `SessionManager::new_with_persistence`

**Iceberg warehouse (write destination — indirect):**
- MinIO S3 bucket `s3://warehouse/` backing Apache Polaris
- Playground sessions get isolated Iceberg namespaces: `playground_<sha256_user[:4]>_<uuid[:4]>`
- Namespace teardown on session end: playground server calls OSS api to DROP shuttle + connection; OSS api tears down the Iceberg namespace

**File Storage:**
- MinIO `file-ingestion` bucket for S3 file-ingestion scenarios
- Accessed via `mc` CLI shell-out (not direct SDK call)

**Caching:**
- None — quota tracker is in-process `Mutex<HashMap>` (per-pod, not distributed)

## Authentication & Identity

**Auth Provider:**
- Custom bearer token (shared secret) — no OAuth/OIDC in playground server itself
- Inbound: `Authorization: Bearer <PLAYGROUND_TOKEN>` checked in `auth_middleware` (`src/router.rs`)
- Outbound to OSS api: `Authorization: Bearer <PLAYGROUND_SERVICE_TOKEN>` + impersonation headers
- Identity extraction middleware: `src/identity.rs` — reads `X-Datashuttle-*` headers from OSS reverse-proxy
- Exempt paths (no auth required): `/health`, `/metrics`, `/api/v1/health`, `/api/v1/manifest`

## Monitoring & Observability

**Error Tracking:**
- None (no Sentry/Bugsnag integration detected)

**Prometheus Metrics** (exposed at `GET /metrics`):
- Registry: per-process `prometheus::Registry`, passed to `PlaygroundMetrics::new`
- Metrics defined in `crates/datashuttle-playground/src/metrics.rs`:
  - `datashuttle_playground_session_started_total{scenario, outcome}` — counter
  - `datashuttle_playground_session_active{tenant}` — gauge
  - `datashuttle_playground_action_duration_seconds{scenario, action, outcome}` — histogram (buckets: 10ms–30s)
  - `datashuttle_playground_action_error_total{scenario, action, error_kind}` — counter
  - `datashuttle_playground_teardown_duration_seconds{kind}` — histogram
  - `datashuttle_playground_orphan_resources_reaped_total{protocol}` — counter
  - `datashuttle_playground_smoke_run_total{scenario, result}` — counter

**Logs:**
- Structured JSON via `tracing-subscriber` with `env-filter`
- Configured by `RUST_LOG` env var (default `info`)
- Key events: startup config, manifest load, api client status, session create/end/reap, orphan sweep results

## CI/CD & Deployment

**Hosting:**
- Docker container (`datashuttle/playground:<tag>`) on port 8081
- Kubernetes via Helm chart (`helm/`) — ClusterIP service, single replica
- Dev: `docker/docker-compose.yml` — simple single-service compose

**CI Pipeline:**
- `release-please` configured (`release-please-config.json`, `.release-please-manifest.json`) for automated versioning/changelog

**Container image tools:**
- `.dockerignore` present
- Build: `FROM rust:1.94-bookworm AS builder` → `FROM debian:bookworm-slim`

## Webhooks & Callbacks

**Incoming:**
- None. Playground server is a request-response service; no webhook receivers.

**Outgoing:**
- OSS api callbacks on session create/reset/end and on every action execution — `POST/DELETE /api/v1/connections`, `POST/DELETE /api/v1/shuttles`, `POST /api/v1/shuttles/:name/resume`, `POST /api/v1/shuttles/:name/pause`
- Orphan sweep: at boot, a background task calls the OSS api to enumerate live shuttles/connections and drop any whose sessions are no longer in the local session map

## Environment Configuration

**Required env vars for full functionality:**
- `PLAYGROUND_API_BASE_URL` — OSS api URL (e.g. `http://api:8080`)
- `PLAYGROUND_SERVICE_TOKEN` — service bearer for OSS api callbacks
- `PLAYGROUND_TOKEN` — inbound bearer; unset = unauthenticated (dev only)
- Source sidecar passwords via Docker secrets files or `DS_*_PLAYGROUND_PASSWORD` env vars

**Optional env vars:**
- `PLAYGROUND_MANIFEST`, `PLAYGROUND_TTL_SECS`, `PLAYGROUND_QUOTA_PER_DAY`, `PLAYGROUND_API_TIMEOUT_SECS`, `PLAYGROUND_DEPLOYMENT`, `PLAYGROUND_EXAMPLES_DIR`, `PLAYGROUND_DATA_DIR`, `PLAYGROUND_BIND_ADDR`, `RUST_LOG`

**Secrets location:**
- Docker secrets: `/run/secrets/pg_playground_password`, `/run/secrets/mysql_playground_password`, `/run/secrets/clickhouse_playground_password`, `/run/secrets/redis_playground_password`
- Kubernetes: `authToken.existingSecret` in Helm values for `PLAYGROUND_TOKEN`
- No `.env` file present in repo

---

*Integration audit: 2026-05-17*
