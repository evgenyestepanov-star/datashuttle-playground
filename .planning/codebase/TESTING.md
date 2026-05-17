# Testing Patterns

**Analysis Date:** 2026-05-17

## Test Framework

**Runner:**
- Tokio-flavored `cargo test` — no separate test crate (e.g., `nextest`) detected
- Async tests use `#[tokio::test]`; sync tests use `#[test]`

**Assertion Library:**
- Standard `assert!`, `assert_eq!`, `assert!(matches!(...))` — no third-party matcher crate

**Run Commands:**
```bash
cargo test --workspace              # Run all tests
cargo test --workspace -- --nocapture   # With stdout
cargo test -p datashuttle-playground    # Foundation lib only
cargo test -p datashuttle-playground-server  # Server + integration smoke
```

## Test File Organization

**Location:** Tests live in two places:
1. `#[cfg(test)] mod tests { ... }` inline at the bottom of each source file — the predominant pattern
2. `crates/datashuttle-playground-server/tests/handlers_smoke.rs` — integration-level smoke test as a separate file under `tests/`

**Test module naming:** Inline modules are usually `mod tests`, but `handlers.rs` uses named sub-modules to group by tested function:
```rust
#[cfg(test)]
mod splitter_tests { ... }    // crates/datashuttle-playground-server/src/handlers.rs:2222

#[cfg(test)]
mod path_tests { ... }        // crates/datashuttle-playground-server/src/handlers.rs:2276

#[cfg(test)]
mod identifier_tests { ... }  // crates/datashuttle-playground-server/src/handlers.rs:2302
```

## Test Structure

**Suite Organization:**

Inline unit tests in each module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> Manifest { ... }   // shared factory

    #[test]
    fn validates_happy_path() { ... }

    #[tokio::test]
    async fn rejects_when_disabled() { ... }
}
```

Integration smoke test in `tests/handlers_smoke.rs` uses a shared `test_state()` factory:
```rust
fn test_state() -> Arc<ServerState> {
    let manifest_path = workspace_examples_dir().join("manifest.json");
    let manifest = Arc::new(Manifest::load(&manifest_path).expect("examples/manifest.json should parse"));
    // ... build NoopPlaygroundDispatcher, SessionManager, etc.
    Arc::new(ServerState { ... })
}

#[tokio::test]
async fn create_then_delete_session_wires_correctly() {
    let app = router(test_state());
    // Drive axum::Router via tower::ServiceExt::oneshot
}
```

## Mocking

**Framework:** No mocking library — the `NoopPlaygroundDispatcher` pattern is used instead.

**Pattern — Noop trait impl:**
```rust
// crates/datashuttle-playground/src/tcp.rs
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPlaygroundDispatcher;

#[async_trait]
impl PlaygroundDispatcher for NoopPlaygroundDispatcher {
    async fn exec_postgres(&self, _sql: &str) -> Result<(String, String), DispatchError> {
        Err(DispatchError::Unavailable)
    }
    // ... all methods return Err(DispatchError::Unavailable)
}
```

The `NoopPlaygroundDispatcher` is the test double for all tests that don't need live database sidecars. Used in:
- `crates/datashuttle-playground-server/tests/handlers_smoke.rs` via `Arc::new(NoopPlaygroundDispatcher)`
- Anywhere a `PlaygroundDispatcher` is needed without real TCP pools

**What to mock:**
- `PlaygroundDispatcher` — use `NoopPlaygroundDispatcher` when the test exercises session lifecycle or HTTP routing without needing actual SQL execution
- External `ApiClient` — pass `api_client: None` in `ServerState` when testing scenarios that don't call back to the OSS api

**What NOT to mock:**
- `Manifest::load` — tests load the real `examples/manifest.json` via `workspace_examples_dir()` (see `manifest.rs:424: parses_real_manifest`)
- `SessionManager` — always construct a real one; it is in-memory and has no external deps

## Fixtures and Factories

**Test Data:**

Manifest factory — used across `manifest.rs`, `sessions.rs`:
```rust
fn valid_manifest() -> Manifest {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "sources": [{ "id": "postgres", "name": "PostgreSQL", "kind": "cdc", "status": "stable", "free": true }],
        "scenarios": [{
            "id": "s1",
            "source_id": "postgres",
            "title": "t",
            "description": "d",
            "difficulty": "beginner",
            "tier": 1,
            "status": "stable",
            "prerequisites": { "deployment": ["dev"] },
            "actions": [{ "id": "a1", "label": "A1", "kind": "sql", "sql": "SELECT 1" }]
        }]
    }))
    .unwrap()
}
```

Quota tracker with overridden limit (avoids hitting the real per-day cap in tests):
```rust
let q = PlaygroundQuotaTracker::with_limit(2);
```

Time injection for quota day-rollover tests (passes explicit `DateTime<Utc>` rather than `Utc::now()`):
```rust
let day1 = Utc.with_ymd_and_hms(2026, 4, 18, 23, 30, 0).unwrap();
q.try_consume_at(Some("t"), day1).unwrap();
```

Persistence tests use `tempfile::tempdir()`:
```rust
let dir = tempfile::tempdir().unwrap();
let mgr = SessionManager::new_with_persistence(manifest, true, DEFAULT_TTL, dir.path()).await;
```

**Location:** All factories are defined locally inside `mod tests` in the same file — no shared fixture crate.

The `in-memory-generator` scenario in `examples/manifest.json` serves as the HTTP smoke-test fixture because it has no `init_sql`, no `shuttle_sql`, and no `docker_service`, allowing the session-lifecycle handlers to exercise pure in-memory paths without a real sidecar.

## Coverage

**Requirements:** None enforced — no `cargo-llvm-cov` or coverage threshold configuration detected.

**View Coverage:**
```bash
cargo test --workspace   # No coverage report; add cargo-llvm-cov separately if needed
```

## Test Types

**Unit Tests (`#[test]` / `#[tokio::test]` inline):**
Present in all source files:
- `crates/datashuttle-playground/src/manifest.rs` — manifest validation, real manifest parse
- `crates/datashuttle-playground/src/sessions.rs` — session lifecycle, expiry, persistence round-trip
- `crates/datashuttle-playground/src/quota.rs` — daily cap, tenant isolation, day rollover
- `crates/datashuttle-playground/src/tcp.rs` — validator functions, Noop dispatcher behavior, error message stability
- `crates/datashuttle-playground/src/metrics.rs` — error kind classification, double-registration guard
- `crates/datashuttle-playground-server/src/dispatcher.rs` — `url_encode`, `TcpPlaygroundDispatcher::is_tcp_backed`
- `crates/datashuttle-playground-server/src/handlers.rs` — SQL splitter correctness, path validation, identifier safety
- `crates/datashuttle-playground-server/src/identity.rs` — middleware accept/reject on headers
- `crates/datashuttle-playground-server/src/api_client.rs` — URL trimming, error classification, statement classifier

**Integration Smoke Tests (`tests/handlers_smoke.rs`):**
- `create_then_delete_session_wires_correctly` — POST then DELETE session via real `axum::Router`
- `list_sessions_requires_identity` — 401 without `X-Datashuttle-User-Id`
- `health_path_does_not_require_identity` — GET `/health` returns 200 unauthenticated
- `manifest_is_reachable_without_identity` — GET `/api/v1/manifest` returns `{"scenarios": [...]}` unauthenticated

These tests drive a real `Router` with `tower::ServiceExt::oneshot` — no `tokio::net::TcpListener` is bound.

**E2E / Per-Scenario Tests:** Not present in this repository. Per the project context, per-scenario tests that query the target Iceberg table and assert `row_count > 0` (R010) are an OSS AGENTS.md requirement that is not yet implemented here. See "Test Coverage Gaps" in CONCERNS.md.

## Common Patterns

**Async Testing:**
```rust
#[tokio::test]
async fn one_session_per_user() {
    let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
    let first = mgr.create("u1", None, "s1", None).await.unwrap();
    let err = mgr.create("u1", None, "s1", None).await.unwrap_err();
    assert!(matches!(err, SessionError::UserLimit(id) if id == first.id));
}
```

**Error Testing — pattern-matching on error variant:**
```rust
let e = m.validate().unwrap_err();
assert!(matches!(e, ManifestError::Validation(_)));
```

**Error message content assertions:**
```rust
let msg = err.to_string();
assert!(msg.contains("tenant-x"));
assert!(msg.contains("1/1"));
assert!(msg.contains("UTC midnight"));
```

**HTTP Handler Testing via `oneshot`:**
```rust
let req = Request::builder()
    .method("POST")
    .uri("/api/v1/sessions")
    .header("Content-Type", "application/json")
    .header("X-Datashuttle-User-Id", "test-user-1")
    .header("X-Datashuttle-Auth-Method", "oidc")
    .body(Body::from(serde_json::to_vec(&serde_json::json!({
        "scenario_id": "in-memory-generator"
    })).unwrap()))
    .unwrap();
let resp = app.clone().oneshot(req).await.expect("request");
assert_eq!(resp.status(), StatusCode::CREATED);
```

**SQL Splitter Tests (R008 — `split_ds_sql_statements`):**

The `split_ds_sql_statements` function in `crates/datashuttle-playground-server/src/handlers.rs:1729` handles multi-statement DS SQL blocks. Tests cover:
- Three-statement template (CREATE CONNECTION + CREATE SHUTTLE + RESUME SHUTTLE)
- Semicolon inside string literal must not split
- `--` line comment with semicolon must not split
- Trailing whitespace-only segments are dropped
- Escaped single-quote (`''`) inside string must not terminate the string

## Dispatcher Test Patterns

The `TcpPlaygroundDispatcher` unit tests in `crates/datashuttle-playground-server/src/dispatcher.rs:878` test only the pure helpers (no live DB):
- `url_encode_is_safe_for_random_hex` — hex chars pass through untouched
- `url_encode_escapes_uri_specials` — `@`, `:`, `/` are percent-encoded
- `tcp_dispatcher_is_tcp_backed` — confirms `is_tcp_backed()` returns `true`

The `split_clickhouse_statements` function (same file) has no dedicated unit tests — its correctness is assumed via the broader ClickHouse integration path. **New tests for this function should be added when the ClickHouse scenario is extended.**

---

*Testing analysis: 2026-05-17*
