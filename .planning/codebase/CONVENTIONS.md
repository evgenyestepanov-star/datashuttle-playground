# Coding Conventions

**Analysis Date:** 2026-05-17

## Naming Patterns

**Files:**
- Rust source files: `snake_case.rs` (e.g., `manifest.rs`, `api_client.rs`, `handlers.rs`)
- Scenario SQL asset files: `kebab-case.sql` or `snake_case.sql` (e.g., `insert-order.sql`, `init.sql`, `shuttle.sql`)
- Shell action files: `kebab-case.sh` (e.g., `load-wide.sh`, `setup-proxies.sh`)

**Functions and methods:**
- `snake_case` throughout — Rust convention enforced by `rustfmt`
- Private helper functions at the bottom of a module (e.g., `split_ds_sql_statements`, `validate_example_relative_path`, `substitute_placeholders` at the end of `handlers.rs`)
- Public functions documented with `///` doc comments; private helpers with `//` inline comments
- Test helper factories named `<subject>_with_<qualifier>` or `valid_<subject>` (e.g., `manifest_with_scenario()`, `valid_manifest()`)

**Types and structs:**
- `PascalCase` — `SessionManager`, `TcpPlaygroundDispatcher`, `PlaygroundQuotaTracker`
- Error types: `<Domain>Error` as `thiserror::Error` enums (`SessionError`, `ManifestError`, `DispatchError`, `QuotaError`, `ApiCallError`)
- View/projection types: `<Model>View` suffix (e.g., `SessionView`, `ManifestView`)
- Response types: `<Handler>Response` suffix (e.g., `ActionResponse`, `ErrorResponse`)

**Constants:**
- `SCREAMING_SNAKE_CASE` — `POOL_MAX_CONNECTIONS`, `STATEMENT_TIMEOUT_SECS`, `DEFAULT_TTL`, `MAX_SESSIONS_PER_TENANT_PER_DAY`

**Variables:**
- `snake_case` — `user_id`, `tenant_id`, `session_id`, `shuttle_name`

**Enums:**
- `PascalCase` variants with `#[serde(rename_all = "lowercase")]` or `#[serde(rename_all = "kebab-case")]` for JSON serialization (e.g., `SessionStatus`, `ActionKind`, `Deployment`, `Difficulty`)

## Code Style

**Formatting:**
- `rustfmt` with `edition = "2021"` — enforced in CI via `cargo fmt --check`
- Config: `crates/datashuttle-playground/rustfmt.toml` (workspace root: `/Users/evgeny/datashuttle/playground/rustfmt.toml`)

**Linting:**
- `clippy` with `-D warnings` in `RUSTFLAGS` and `--all-targets -- -D warnings` in CI
- `clippy.toml` sets `avoid-breaking-exported-api = false` — breaking public API changes are not flagged as errors
- `deny.toml` present (crate-level deny rules checked via `cargo deny`)

## Module Structure

**Pattern:** Each logical concern is its own `mod.rs`-free flat file with a module-level doc comment (`//! ...`) at the top.

Every source file opens with a `//!` doc comment explaining:
1. What the module owns
2. Why it exists (often including phase/epic references)
3. Any non-obvious design decisions or historical context

Example from `crates/datashuttle-playground/src/tcp.rs`:
```rust
//! Playground source-database dispatch trait (#829, #817 Phase C).
//!
//! Until Phase C the api crate hosted a concrete `PlaygroundDispatcher`
//! struct holding `sqlx::PgPool` + `mysql_async::Pool` fields behind 19
//! `#[cfg(feature = "saas")]` gates. Phase C splits the surface:
//! ...
```

**Section separators:** Long files use `// ----- section name -----` banners:
```rust
// --------------------------------------------------------------------- routes
// --------------------------------------------------------------------- types
// --------------------------------------------------------------------- error helpers
// --------------------------------------------------------------------- handlers
// --------------------------------------------------------------------- tests
```

## Import Organization

**Order (standard Rust/rustfmt convention):**
1. `std` / `core` imports
2. External crate imports (alphabetical by crate name)
3. Internal (`crate::`) imports

**Pattern example from `handlers.rs`:**
```rust
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
// ...
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api_client::{ApiCallError, ApiClient};
use crate::identity::Identity;
use crate::router::ServerState;
```

**Path Aliases:** None detected; full crate paths used everywhere.

## Error Handling

**Pattern:** `thiserror`-derived enums for domain errors; `anyhow` for configuration loading.

- Domain errors (`SessionError`, `ManifestError`, `DispatchError`, `QuotaError`) use `thiserror::Error` with `#[error("...")]` messages that include context directly in the variant.
- Handler functions return `Result<T, (StatusCode, Json<ErrorResponse>)>` — the error side is a complete HTTP response tuple, not a naked error type.
- `map_*_error` helper functions convert low-level DB/transport errors into the typed `DispatchError` variants (e.g., `map_pg_error`, `map_mysql_error`, `map_redis_error`).
- No `unwrap()` in production paths — use `expect()` with a message or `?` propagation. `unwrap()` is acceptable in test code.
- Best-effort operations log via `warn!` and continue rather than returning errors (e.g., `provision_session_resources` failure in `create_session` records the event and proceeds).

## Logging

**Framework:** `tracing` crate (`tracing = "0.1"`, `tracing-subscriber` with `env-filter` + `json` features)

**Patterns:**
- Use structured fields: `info!(user_id = %identity.user_id, session_id = %session.id, scenario = %session.scenario_id, "playground session created")`
- `%` sigil for `Display` formatting, `?` sigil for `Debug`
- Log at `info!` for lifecycle transitions (session create/delete), `warn!` for recoverable failures, never `error!` unless truly fatal
- Sweep/orphan operations log a summary count, not individual items

## Comments

**When to comment:**
- Every module-level change: update the `//!` doc comment with architectural context
- Non-obvious design decisions get inline `//` comments at the decision point
- Reference issue/PR numbers for historical context: `// Phase 5.C extraction — moved out of datashuttle-cloud::playground`
- Test functions include a brief `// comment` when the test case is non-obvious

**Never annotate temporal state:** Document what IS, not what WAS. Exception: architecture evolution comments referencing specific phase numbers are intentional.

## Function Design

**Size:** Handler functions are large (hundreds of lines) because they contain the full flow inline. Helper extraction is reserved for genuinely reusable logic.

**Parameters:** Functions accepting many related values take a reference to a context struct (`&ServerState`, `&Session`) rather than individual parameters.

**Return Values:**
- Infallible functions return concrete types
- Fallible functions return `Result<T, E>` where `E` is a domain error type
- Handler functions return `Result<(StatusCode, Json<T>), (StatusCode, Json<ErrorResponse>)>`

## Scenario-as-Data Conventions

**Manifest file:** `examples/manifest.json` is the single source of truth. Schema pinned at `examples/manifest.schema.json` (JSON Schema draft 2020-12). Version field must be `1`.

**Scenario id:** `kebab-case` string, globally unique across all scenarios (e.g., `postgres-cdc-ecommerce`, `mysql-cdc-iot`, `in-memory-generator`).

**Source id:** `kebab-case` string, globally unique across all sources (e.g., `postgres`, `mysql`, `clickhouse`, `in-memory`).

**Action id:** `kebab-case` string, unique within its scenario (e.g., `insert-order`, `burst-100`, `drop-column`).

**SQL placeholder tokens in scenario files:** `{shuttle}`, `{namespace}`, `{connection}`, `{session}`, `{source_host}`, `{source_port}`, `{source_db}`, `{source_user}`, `{source_password}`, `{minio_endpoint}`, `{minio_access_key}`, `{minio_secret_key}`. Substitution done by `substitute_placeholders` + `substitute_source_coords` in `crates/datashuttle-playground-server/src/handlers.rs`.

**Per-scenario directory layout** (`examples/playground/<scenario-slug>/`):
- `shuttle.sql` — multi-statement DS SQL template with `{placeholder}` tokens; split by `split_ds_sql_statements` before dispatch
- `init.sql` (or `init.sh` for non-SQL sources) — run once per session during provisioning to seed source data
- `catchup.sql` — optional; used for snapshot scenarios that need a catch-up pass
- `actions/` — one `.sql` or `.sh` file per whitelisted action referenced by `sql_file` in the manifest

**init.sql authoring constraints:**
- Must be safe through the statement splitter (splits on `;` outside single-quoted strings and `--` line comments)
- No `DELIMITER` blocks or stored procedures with compound bodies — these cannot be safely split
- For MySQL: use recursive CTE INSERTs rather than stored procedures (see comment in `examples/playground/mysql-cdc/init.sql`)
- For ClickHouse: each statement is posted as a separate HTTP request (see `split_clickhouse_statements` in `crates/datashuttle-playground-server/src/dispatcher.rs`)

## Module Design

**Exports:** Types needed by tests or integration consumers are `pub`; internal helpers are `pub(crate)` or private.

**Barrel Files:** The `lib.rs` in each crate is a thin re-export barrel:
```rust
// crates/datashuttle-playground/src/lib.rs
pub mod manifest;
pub mod metrics;
pub mod quota;
pub mod sessions;
pub mod tcp;
pub use manifest::Manifest;
```

**Trait objects:** `Arc<dyn PlaygroundDispatcher>` on `ServerState` — trait must be `Send + Sync + Debug` and object-safe (no generic methods).

---

*Convention analysis: 2026-05-17*
