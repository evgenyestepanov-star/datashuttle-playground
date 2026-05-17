# Codebase Structure

**Analysis Date:** 2026-05-17

## Directory Layout

```
playground/                            # Workspace root
├── Cargo.toml                         # Workspace manifest (members, shared deps)
├── Cargo.lock                         # Locked dependency versions
├── clippy.toml                        # Clippy lint configuration
├── rustfmt.toml                       # Rustfmt formatting config
├── deny.toml                          # cargo-deny license + advisory config
│
├── crates/                            # Rust crates
│   ├── datashuttle-playground/        # Foundation library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                 # Public re-exports
│   │       ├── manifest.rs            # Manifest/Scenario/Action types + loader
│   │       ├── metrics.rs             # Prometheus metrics definitions
│   │       ├── quota.rs               # Per-tenant daily session quota
│   │       ├── sessions.rs            # SessionManager + Session types
│   │       └── tcp.rs                 # PlaygroundDispatcher trait + NoopDispatcher
│   │
│   └── datashuttle-playground-server/ # HTTP server binary crate
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs                # Binary entry point + boot sequence
│       │   ├── lib.rs                 # Re-export modules for integration tests
│       │   ├── router.rs              # Axum router, ServerState, auth middleware
│       │   ├── handlers.rs            # Session lifecycle HTTP handlers
│       │   ├── identity.rs            # Identity extraction middleware
│       │   ├── api_client.rs          # OSS api callback client
│       │   ├── config.rs              # Env-var config loader
│       │   └── dispatcher.rs          # TcpPlaygroundDispatcher implementation
│       └── tests/
│           └── handlers_smoke.rs      # Integration smoke test (wired router)
│
├── examples/                          # Scenario data (loaded at runtime)
│   ├── manifest.json                  # Authoritative scenario + source catalog
│   ├── manifest.schema.json           # JSON Schema (draft 2020-12) for manifest
│   │
│   ├── playground/                    # Scenarios used by the cloud playground
│   │   ├── cassandra/                 # Cassandra CDC scenario
│   │   ├── clickhouse-snapshot/       # ClickHouse snapshot scenario
│   │   ├── dynamodb/                  # DynamoDB scenario
│   │   ├── file-ingestion/            # File (CSV) ingestion scenario
│   │   ├── kinesis/                   # Kinesis streaming scenario
│   │   ├── large-payload/             # Large payload stress scenario
│   │   ├── mongodb-cdc/               # MongoDB CDC scenario
│   │   ├── mysql-cdc/                 # MySQL CDC scenario
│   │   ├── postgres-cdc/              # Postgres CDC scenario
│   │   ├── redis-streams-cdc/         # Redis Streams CDC scenario
│   │   ├── redis-streams-events/      # Redis Streams events scenario
│   │   ├── rest-api/                  # REST API source scenario
│   │   ├── chaos/                     # Toxiproxy chaos injection helpers
│   │   └── wiremock/                  # WireMock HTTP mock mappings
│   │
│   ├── postgres-cdc/                  # Standalone (non-playground) example
│   ├── mysql-cdc/                     # Standalone example
│   ├── mongodb-cdc/                   # Standalone example
│   ├── clickhouse-snapshot/           # Standalone example (incl. k8s/)
│   ├── file-ingestion/                # Standalone example
│   ├── realtime-demo/                 # Standalone demo (Kafka, Python dashboard)
│   ├── full-demo/                     # Standalone multi-source demo
│   ├── polaris-config/                # Apache Polaris/Iceberg config example
│   └── docker-compose.yml             # Compose for standalone examples
│
├── docker/
│   ├── Dockerfile                     # Production container build
│   └── docker-compose.yml             # Dev run (playground service only)
│
├── helm/                              # Kubernetes Helm chart
│   ├── Chart.yaml
│   ├── values.yaml                    # Configurable values (image, env, auth)
│   └── templates/
│       ├── deployment.yaml
│       ├── service.yaml
│       └── _helpers.tpl
│
├── docs/
│   └── playground.md                  # mdbook documentation
│
└── .github/
    └── workflows/
        ├── ci.yml                     # Build + test CI
        ├── commitlint.yml             # Commit message lint
        ├── release-please.yml         # Release PR automation
        └── release.yml                # Container publish on tag
```

## Directory Purposes

**`crates/datashuttle-playground/src/`:**
- Purpose: Foundation library — types and runtime primitives shared across all consumers. No HTTP, no networking, no sqlx.
- Contains: Session manager, manifest types, dispatcher trait, quota tracker, metrics
- Key files: `sessions.rs` (session lifecycle), `manifest.rs` (scenario data types), `tcp.rs` (dispatcher trait)

**`crates/datashuttle-playground-server/src/`:**
- Purpose: The deployable HTTP server. All axum, sqlx, mysql_async, reqwest code lives here.
- Contains: Router, middleware, handlers, concrete TCP dispatcher, API callback client, config
- Key files: `main.rs` (boot sequence), `handlers.rs` (all HTTP endpoints), `dispatcher.rs` (DB pool management)

**`crates/datashuttle-playground-server/tests/`:**
- Purpose: Integration tests that wire a real axum router with in-memory state and execute HTTP requests
- Key files: `handlers_smoke.rs` (session create/get/delete cycle)

**`examples/playground/`:**
- Purpose: Scenario asset tree for cloud playground scenarios. Each subdirectory corresponds to one scenario declared in `manifest.json`.
- Contains: `shuttle.sql` (CREATE SHUTTLE/CONNECTION statements), `init.sql` (source DB seed data), `actions/` (whitelisted SQL/shell action files)
- Key convention: Scenario directory name matches `scenario.id` in `manifest.json`

**`examples/<scenario-name>/` (top-level non-playground examples):**
- Purpose: Self-contained developer examples with their own docker-compose files and READMEs. Not consumed by the playground server at runtime.
- Contains: Full demo setups, k8s manifests, generate/verify scripts

**`docker/`:**
- Purpose: Container build artifacts for the playground server binary
- Key files: `Dockerfile` (multi-stage Rust build), `docker-compose.yml` (dev run)

**`helm/`:**
- Purpose: Kubernetes deployment chart for the playground server
- Key files: `values.yaml` (configures image, port, auth secret reference)

## Key File Locations

**Entry Points:**
- `crates/datashuttle-playground-server/src/main.rs`: Binary boot sequence — config, manifest load, state assembly, background tasks, axum listener
- `crates/datashuttle-playground-server/src/router.rs`: `ServerState` struct definition and `router()` function

**Configuration:**
- `crates/datashuttle-playground-server/src/config.rs`: All env-var knobs with defaults; `Config::load()` is called once at boot
- `Cargo.toml` (workspace root): Shared dependency versions and workspace metadata

**Core Logic:**
- `crates/datashuttle-playground/src/sessions.rs`: `SessionManager` — all session CRUD, TTL, persistence, sweep
- `crates/datashuttle-playground/src/manifest.rs`: `Manifest::load` + `Manifest::validate` + whitelist lookups
- `crates/datashuttle-playground-server/src/handlers.rs`: All HTTP handlers — the primary business logic file
- `crates/datashuttle-playground-server/src/dispatcher.rs`: `TcpPlaygroundDispatcher` — all source DB operations
- `crates/datashuttle-playground-server/src/api_client.rs`: `ApiClient::exec_sql` — outbound OSS api calls

**Scenario Data:**
- `examples/manifest.json`: The single source of truth for all scenarios and sources surfaced in the UI
- `examples/manifest.schema.json`: JSON Schema for manifest validation
- `examples/playground/<scenario>/shuttle.sql`: CREATE SHUTTLE statement(s) for the scenario
- `examples/playground/<scenario>/init.sql` (or `init.sh`, `init.cql`, `init.js`): Source DB seed script
- `examples/playground/<scenario>/actions/`: Whitelisted action payloads referenced by `manifest.json`

**Testing:**
- `crates/datashuttle-playground-server/tests/handlers_smoke.rs`: Integration smoke test
- Inline `#[cfg(test)]` modules throughout all source files (unit tests)

## Naming Conventions

**Files:**
- Snake-case Rust source files: `session_manager` pattern would be `sessions.rs`, `api_client.rs`
- SQL scenario files: `shuttle.sql`, `init.sql`, descriptive action files like `catchup.sql`, `load-100k.sql`
- Shell action files: kebab-case `.sh`, e.g. `load-wide.sh`, `insert-50mb.sh`

**Directories:**
- Scenario directories: kebab-case matching scenario `id` in manifest (`postgres-cdc`, `clickhouse-snapshot`)
- Rust crates: kebab-case with `datashuttle-playground` prefix

**Rust:**
- Structs and enums: `PascalCase` (`SessionManager`, `TcpPlaygroundDispatcher`, `PlaygroundDispatcher`)
- Functions and methods: `snake_case` (`create_session`, `exec_postgres_in_schema`)
- Constants: `SCREAMING_SNAKE_CASE` (`DEFAULT_TTL`, `POOL_MAX_CONNECTIONS`, `STATEMENT_TIMEOUT_SECS`)
- Error types: `PascalCase` with `Error` suffix or `thiserror::Error` derive (`SessionError`, `DispatchError`)

## Where to Add New Code

**New scenario:**
1. Create directory `examples/playground/<scenario-id>/`
2. Add `shuttle.sql` and `init.sql` (or source-appropriate init script)
3. Add any action files to `examples/playground/<scenario-id>/actions/`
4. Declare source (if new) and scenario in `examples/manifest.json`, referencing the correct `source_id` and action `sql_file` / `shell_cmd` paths
5. No Rust code changes needed unless a new `ActionKind` is required

**New action kind:**
1. Add variant to `ActionKind` enum in `crates/datashuttle-playground/src/manifest.rs`
2. Add handling in the `run_action` / `execute_action` handler in `crates/datashuttle-playground-server/src/handlers.rs`
3. Add validation in `validate_action` in `crates/datashuttle-playground/src/manifest.rs`

**New dispatcher method (new source DB protocol):**
1. Add trait method(s) to `PlaygroundDispatcher` in `crates/datashuttle-playground/src/tcp.rs` (with default `Err(Unavailable)` so `NoopPlaygroundDispatcher` auto-inherits)
2. Implement in `TcpPlaygroundDispatcher` in `crates/datashuttle-playground-server/src/dispatcher.rs`
3. Add pool builder function and env-var defaults in `dispatcher.rs`

**New configuration knob:**
1. Add field to `Config` struct in `crates/datashuttle-playground-server/src/config.rs`
2. Read from env var in `config::load()` with a sensible default constant
3. Update `helm/values.yaml` env block and `docker/docker-compose.yml` if the knob should be exposed

**New HTTP endpoint:**
1. Add route to `handlers::routes()` in `crates/datashuttle-playground-server/src/handlers.rs`
2. Implement handler function in the same file, following the `require_identity` / `require_sessions` / `require_api_client` guard pattern
3. Add integration test in `crates/datashuttle-playground-server/tests/handlers_smoke.rs`

**Utilities / shared helpers:**
- Session-scoped helpers: `crates/datashuttle-playground/src/sessions.rs` (e.g., namespace derivation functions)
- Server-local helpers: inline in the relevant `src/` module

## Special Directories

**`examples/playground/`:**
- Purpose: Runtime scenario asset tree read by the playground server
- Generated: No — hand-authored
- Committed: Yes — deployed in the container image at `/opt/datashuttle/examples/`

**`examples/` (top-level non-playground subdirs):**
- Purpose: Standalone developer examples for docs and demos — NOT loaded by the playground server
- Generated: No
- Committed: Yes

**`.planning/codebase/`:**
- Purpose: Architecture + conventions documents consumed by GSD planning and execution agents
- Generated: Yes (by GSD mapper)
- Committed: Yes

---

*Structure analysis: 2026-05-17*
