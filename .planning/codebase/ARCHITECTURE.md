<!-- refreshed: 2026-05-17 -->
# Architecture

**Analysis Date:** 2026-05-17

## System Overview

```text
┌──────────────────────────────────────────────────────────────────────┐
│                      OSS DataShuttle API                             │
│             (datashuttle-api-core, external to this repo)            │
│  Reverse-proxy: strips /api/v1/playground prefix, injects            │
│  X-Datashuttle-* identity headers, forwards to playground-server     │
└────────────────────────────┬─────────────────────────────────────────┘
                             │  HTTP  Bearer=PLAYGROUND_TOKEN
                             │  X-Datashuttle-User-Id / Tenant-Id
                             ▼
┌──────────────────────────────────────────────────────────────────────┐
│              datashuttle-playground-server  (binary)                 │
│  `crates/datashuttle-playground-server/src/`                        │
│                                                                      │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────────┐  │
│  │ auth_mid     │  │ identity_mid │  │  handlers.rs              │  │
│  │ router.rs    │  │ identity.rs  │  │  (session lifecycle,       │  │
│  │              │  │              │  │   action dispatch)         │  │
│  └──────────────┘  └──────────────┘  └──────────┬────────────────┘  │
│                                                  │                   │
│  ┌───────────────────┐     ┌────────────────────┐│                   │
│  │  api_client.rs    │◄────┤  ServerState        ││                   │
│  │  (OSS api calls)  │     │  (config, sessions, ││                   │
│  └───────────────────┘     │   quota, metrics,   ││                   │
│                            │   dispatcher,       ││                   │
│  ┌───────────────────┐     │   api_client)       ││                   │
│  │  dispatcher.rs    │◄────┤                     ││                   │
│  │  TcpPlayground-   │     └────────────────────┘│                   │
│  │  Dispatcher       │                            │                   │
│  └──────┬────────────┘                            │                   │
└─────────┼───────────────────────────────────────────────────────────┘
          │                                          │
          │  TCP                                     │  HTTP callbacks
          ▼                                          ▼
┌─────────────────────┐            ┌─────────────────────────────────┐
│  Source Sidecars     │            │  OSS api /api/v1/shuttles       │
│  (postgres, mysql,   │            │          /api/v1/connections    │
│   clickhouse, redis) │            │          /api/v1/catalog        │
│  (Docker containers) │            └─────────────────────────────────┘
└─────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│              datashuttle-playground  (foundation library)            │
│  `crates/datashuttle-playground/src/`                               │
│  sessions.rs | manifest.rs | tcp.rs | quota.rs | metrics.rs         │
└──────────────────────────────────────────────────────────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| `router.rs` | Builds axum router, wires auth + identity middleware, owns `ServerState` struct | `crates/datashuttle-playground-server/src/router.rs` |
| `handlers.rs` | Session lifecycle HTTP handlers (create, get, list, end, reset, extend, execute action), orphan sweeper, session reaper | `crates/datashuttle-playground-server/src/handlers.rs` |
| `identity.rs` | Middleware that extracts `Identity` from `X-Datashuttle-*` headers; rejects non-exempt paths with 401 | `crates/datashuttle-playground-server/src/identity.rs` |
| `api_client.rs` | HTTP client for OSS api callbacks; client-side dispatches playground SQL to typed api endpoints | `crates/datashuttle-playground-server/src/api_client.rs` |
| `dispatcher.rs` | `TcpPlaygroundDispatcher` — concrete pool-backed impl of `PlaygroundDispatcher`; Postgres/MySQL/ClickHouse/Redis | `crates/datashuttle-playground-server/src/dispatcher.rs` |
| `config.rs` | Env-var-only config loader; all playground knobs | `crates/datashuttle-playground-server/src/config.rs` |
| `main.rs` | Boot sequence: load manifest, wire state, spawn reaper + orphan sweeper, start axum listener | `crates/datashuttle-playground-server/src/main.rs` |
| `sessions.rs` | In-memory `SessionManager` with optional JSON persistence, TTL enforcement, per-user one-session limit, action rate-limit | `crates/datashuttle-playground/src/sessions.rs` |
| `manifest.rs` | `Manifest` + `Scenario` + `Action` types, JSON loader, validation, action whitelist | `crates/datashuttle-playground/src/manifest.rs` |
| `tcp.rs` | `PlaygroundDispatcher` trait definition, `NoopPlaygroundDispatcher`, input-safety validators (`is_safe_resource_name`) | `crates/datashuttle-playground/src/tcp.rs` |
| `quota.rs` | Per-tenant per-UTC-day session-creation cap | `crates/datashuttle-playground/src/quota.rs` |
| `metrics.rs` | Prometheus counters/histograms/gauges for session, action, teardown, orphan, smoke events | `crates/datashuttle-playground/src/metrics.rs` |

## Pattern Overview

**Overall:** Two-crate Rust workspace. A foundation library (`datashuttle-playground`) contains trait definitions, session manager, manifest types, quota, and metrics. The server binary crate (`datashuttle-playground-server`) contains the concrete HTTP surface, dispatcher implementation, and boot logic. All state is shared via `Arc<ServerState>`.

**Key Characteristics:**
- Scenarios live as data (`examples/manifest.json`) and are loaded at boot; the server holds no hard-coded scenario logic
- Session isolation is enforced by per-session Iceberg namespace (`playground_<uhash>_<sid>`) and per-session source-DB schema/database
- The playground-server never calls the OSS api directly for engine operations — it uses `ApiClient` to call back to the OSS api process's REST endpoints
- The OSS api's reverse-proxy is the only public entry point; direct access to the playground-server is not intended and requires the shared `PLAYGROUND_TOKEN` bearer
- Action execution is whitelisted through the manifest — no free-form SQL from users is accepted

## Layers

**Foundation Library (`datashuttle-playground`):**
- Purpose: Self-contained runtime primitives with no coupling to private OSS internals
- Location: `crates/datashuttle-playground/src/`
- Contains: Session manager, manifest types and loader, dispatcher trait + noop impl, quota tracker, Prometheus metrics
- Depends on: `serde`, `tokio`, `uuid`, `chrono`, `sha2`, `prometheus`, `async-trait`, `thiserror`
- Used by: `datashuttle-playground-server` (server binary)

**Server Binary (`datashuttle-playground-server`):**
- Purpose: HTTP surface, concrete TCP dispatcher, OSS api callback client, boot logic
- Location: `crates/datashuttle-playground-server/src/`
- Contains: Axum router, auth + identity middleware, session lifecycle handlers, action execution, orphan sweeper, `TcpPlaygroundDispatcher`
- Depends on: `datashuttle-playground` (foundation lib), `axum`, `sqlx`, `mysql_async`, `reqwest`, `redis`, `tower-http`
- Used by: Deployed as the `datashuttle-playground-server` binary; integration tests in `tests/`

**Scenario Data Layer:**
- Purpose: All scenario definitions, source seed SQL, and curated action files
- Location: `examples/playground/` (playground-specific) and `examples/manifest.json`
- Contains: `shuttle.sql` (CREATE PIPELINE / CREATE SHUTTLE statements), `init.sql` (source DB seed), `actions/*.sql` / `actions/*.sh` (whitelisted user actions)
- Depends on: Nothing at runtime — loaded from filesystem via `Config.examples_dir`
- Used by: `handlers.rs` reads action file content; `manifest.rs` validates action references

## Data Flow

### Session Creation

1. Browser hits `POST /api/v1/playground/sessions` on OSS api (`datashuttle-api-core`)
2. OSS api reverse-proxy strips `/playground` prefix, injects `X-Datashuttle-*` headers, forwards to `POST /api/v1/sessions` on playground-server
3. `auth_middleware` validates `Authorization: Bearer <PLAYGROUND_TOKEN>` (`crates/datashuttle-playground-server/src/router.rs`)
4. `identity_middleware` extracts `Identity` from `X-Datashuttle-User-Id` header (`crates/datashuttle-playground-server/src/identity.rs`)
5. `create_session` handler validates scenario id against manifest whitelist; calls `SessionManager::create`; charges quota via `PlaygroundQuotaTracker::try_consume` (`crates/datashuttle-playground-server/src/handlers.rs`)
6. `SessionManager` allocates `playground_<uhash>_<sid>` namespace + shuttle/connection names; persists to `<data_dir>/playground/sessions.json` (`crates/datashuttle-playground/src/sessions.rs`)
7. Handler calls `TcpPlaygroundDispatcher::provision_postgres_schema` (or mysql/clickhouse) to create isolated sidecar schema (`crates/datashuttle-playground-server/src/dispatcher.rs`)
8. Handler executes `init_sql` against the sidecar via the dispatcher (seeds source DB)
9. Handler calls `ApiClient::exec_sql` for each statement in `shuttle_sql` — dispatches `CREATE CONNECTION` / `CREATE SHUTTLE` to OSS api (`crates/datashuttle-playground-server/src/api_client.rs`)
10. Session status advances: `Provisioning` → `Active`; `SessionView` returned as HTTP 201

### Action Execution

1. Browser hits `POST /api/v1/playground/sessions/:id/actions/:action_id`
2. After auth + identity middleware, `execute_action` handler looks up the session and action from the manifest whitelist
3. Rate-limit enforced via `SessionManager::touch_action` (1-second cooldown)
4. Depending on `Action.kind`: `Sql` → dispatcher `exec_*` call; `Http` → `ApiClient.request`; `Shell` → `tokio::process::Command` (or dispatcher for TCP-backed deployments); `ProduceKafka` → base64 payload write
5. Result recorded as `SessionEvent` on the session
6. `ActionResponse` returned (stdout / stderr / error)

### Session Teardown

1. User sends `DELETE /api/v1/playground/sessions/:id` OR background reaper sweeps expired sessions every 60s
2. `teardown_session` function calls `ApiClient::exec_sql("DROP SHUTTLE ...")` then `DROP CONNECTION`
3. Dispatcher tears down the sidecar namespace (`teardown_postgres_schema` / `teardown_mysql_database` etc.)
4. `SessionManager::end` removes session from in-memory map and persists updated state
5. Metrics: `teardown_duration_seconds{kind="session"}` observed

**State Management:**
- All live session state is held in `SessionManager`'s `RwLock<HashMap<Uuid, Session>>` (in-memory, single pod)
- Persisted to `<data_dir>/playground/sessions.json` on every mutation (temp-file + rename for atomicity)
- On restart, hydrate drops expired entries; orphan sweeper then cleans up any sidecar artifacts not represented by live sessions

## Key Abstractions

**`PlaygroundDispatcher` trait:**
- Purpose: Abstracts source-database operations (exec SQL, provision/teardown schema, list playground artifacts)
- Examples: `crates/datashuttle-playground/src/tcp.rs` (trait + noop), `crates/datashuttle-playground-server/src/dispatcher.rs` (TCP impl)
- Pattern: `Arc<dyn PlaygroundDispatcher>` on `ServerState`; `NoopPlaygroundDispatcher` returns `Unavailable` for OSS builds; `TcpPlaygroundDispatcher` uses `OnceCell` lazy-init pools

**`Manifest` / `Scenario` / `Action`:**
- Purpose: Type-safe representation of `examples/manifest.json`; drives the scenario gallery, action whitelist, deployment visibility filters
- Examples: `crates/datashuttle-playground/src/manifest.rs`
- Pattern: Loaded once at boot via `Manifest::load`; held as `Arc<Manifest>` on `SessionManager`; `Scenario::allowed_action_ids()` is the authoritative whitelist

**`Identity`:**
- Purpose: Authenticated user + tenant forwarded by the OSS reverse-proxy through headers
- Examples: `crates/datashuttle-playground-server/src/identity.rs`
- Pattern: Built in `identity_middleware`, inserted into `Request::extensions`, extracted by handlers via `require_identity(&request)`

**`ApiClient`:**
- Purpose: Outbound HTTP client for OSS api callbacks; parses playground SQL into typed api endpoint calls
- Examples: `crates/datashuttle-playground-server/src/api_client.rs`
- Pattern: `Option<Arc<ApiClient>>` on `ServerState`; handlers call `require_api_client(&state)` which returns 503 when absent (partial deploy)

## Entry Points

**Binary entrypoint:**
- Location: `crates/datashuttle-playground-server/src/main.rs`
- Triggers: `datashuttle-playground-server` process start
- Responsibilities: Load config, load manifest, init `SessionManager` with persistence, build `TcpPlaygroundDispatcher`, optionally build `ApiClient`, spawn background reaper + orphan sweeper, start axum server on `PLAYGROUND_BIND_ADDR` (default `0.0.0.0:8081`)

**HTTP routes (all under `/api/v1` after proxy prefix strip):**
- `GET /health` — unauthenticated liveness probe
- `GET /metrics` — Prometheus exposition, unauthenticated
- `GET /api/v1/health` — unauthenticated playground health
- `GET /api/v1/manifest` — scenario list, unauthenticated
- `POST /api/v1/sessions` — create session (authenticated)
- `GET /api/v1/sessions` — list user's sessions (authenticated)
- `GET /api/v1/sessions/:id` — get session (authenticated, ownership checked)
- `DELETE /api/v1/sessions/:id` — end session (authenticated, ownership checked)
- `POST /api/v1/sessions/:id/reset` — reset session (authenticated)
- `POST /api/v1/sessions/:id/extend` — extend TTL (authenticated)
- `POST /api/v1/sessions/:id/actions/:action_id` — execute whitelisted action (authenticated)

## Architectural Constraints

- **Threading:** Single-process tokio async runtime (`#[tokio::main]`). `SessionManager` uses `RwLock` internally. `PlaygroundQuotaTracker` uses `std::sync::Mutex` (sync, not async — contention is minimal).
- **Global state:** No module-level singletons. All state flows through `Arc<ServerState>`. `TcpPlaygroundDispatcher` uses `OnceCell` for lazy pool init per-instance.
- **Circular imports:** None detected. The foundation crate (`datashuttle-playground`) has no dependency on the server crate.
- **Proxy dependency:** The server is not designed to be internet-facing alone. It trusts `X-Datashuttle-*` headers injected by the OSS api's reverse-proxy. The `PLAYGROUND_TOKEN` bearer is the only inbound auth mechanism.
- **Single-pod quota:** `PlaygroundQuotaTracker` is per-pod in-memory; multi-pod deployments allow up to `MAX * pods` session creations per tenant per day.
- **No OSS core dependency:** The foundation library explicitly avoids importing `datashuttle-core` to keep the dep graph (and thus DataFusion/Arrow) out of the playground binary.

## Anti-Patterns

### Calling the OSS api `/api/v1/sql` generic endpoint

**What happens:** Early code called `POST /api/v1/sql` with raw SQL; `api_client.rs` now documents this endpoint was retired in commit 9e925433.
**Why it's wrong:** The generic SQL endpoint no longer exists in the OSS api.
**Do this instead:** Use `ApiClient::exec_sql`, which client-side dispatches to the correct typed endpoint (`POST /api/v1/connections`, `POST /api/v1/shuttles`, etc.) based on statement prefix-matching (`crates/datashuttle-playground-server/src/api_client.rs`).

### Free-form SQL in action definitions

**What happens:** An action without a pre-reviewed `sql` / `sql_file` reference cannot be added.
**Why it's wrong:** The manifest validator (`Manifest::validate`) rejects actions of kind `sql` that lack `sql` or `sql_file`; handlers reject any action_id not present in the manifest whitelist.
**Do this instead:** Define SQL in a file under `examples/playground/<scenario>/` (or inline in `manifest.json`) and reference it via `sql_file` in the action. The file path is resolved against `Config.examples_dir` at execution time.

### Unsafe resource names in SQL strings

**What happens:** Splicing a session namespace directly into a DDL string without validation.
**Why it's wrong:** Would allow SQL injection via crafted namespace strings.
**Do this instead:** Always pass names through `is_safe_resource_name` or `is_safe_playground_shuttle_artifact` (`crates/datashuttle-playground/src/tcp.rs`) before use; the dispatcher methods do this automatically.

## Error Handling

**Strategy:** Typed errors per layer. `DispatchError` for source-DB failures; `SessionError` for session-manager failures; `ApiCallError` for OSS api callback failures; `ManifestError` for manifest load/validate failures; `QuotaError` for quota exhaustion.

**Patterns:**
- Handlers map typed errors to HTTP status codes via `map_session_err` (e.g., `SessionError::UserLimit` → 409, `SessionError::Cooldown` → 429)
- `DispatchError::Unavailable` triggers the OSS-build fallback path (shell exec); `is_tcp_backed()` guard prevents this fallback on cloud deployments
- `ApiCallError::Status` surfaces the upstream HTTP body to the caller for debugging

## Cross-Cutting Concerns

**Logging:** `tracing` crate, JSON format output (`tracing_subscriber::fmt().json()`), filtered by `RUST_LOG` env var. All session lifecycle events logged with structured fields.
**Validation:** Input sanitised at two boundaries — `is_safe_resource_name` / `is_safe_playground_shuttle_artifact` for resource names; `Manifest::validate` for scenario/action definitions at load time.
**Authentication:** Two-layer: shared bearer token (`PLAYGROUND_TOKEN`) at the transport boundary; per-request `Identity` extracted from OSS-proxy-injected headers. Public probe paths (`/health`, `/metrics`, `/api/v1/manifest`) bypass both.

---

*Architecture analysis: 2026-05-17*
