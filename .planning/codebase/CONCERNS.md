# Codebase Concerns

**Analysis Date:** 2026-05-17

## R007 — Legacy `mode = 'SNAPSHOT_THEN_CDC'` SQL Syntax

**Files affected:**
- `examples/mysql-cdc/shuttle.sql` (line 19) — live use in `CREATE SHUTTLE iot_cdc`
- `examples/mongodb-cdc/shuttle.sql` (line 15) — live use in `CREATE SHUTTLE social_cdc`
- `examples/postgres-cdc/shuttle.sql` (line 29) — live use in `CREATE SHUTTLE ecommerce_cdc`
- `examples/postgres-cdc/shuttle-backfill.sql` (lines 21–26) — uses `start_mode = 'snapshot_then_cdc'` which the comment says is also silently ignored

**Issue:** The parser rejects `MODE` as a key and treats `start_mode` as an unknown WITH option (silently ignored per comment in `examples/playground/postgres-cdc/shuttle-backfill.sql:18`). The three standalone demo files (`examples/mysql-cdc/`, `examples/mongodb-cdc/`, `examples/postgres-cdc/`) still contain `mode = 'SNAPSHOT_THEN_CDC'`. Users who copy these examples verbatim will get a parse error in prod; the shuttle silently starts without snapshot-then-CDC semantics.

**Impact:** High. These are the primary user-facing example files referenced in README and docs. Silent semantic failure in prod — shuttle starts in CDC-only mode, missing any backfill rows.

**Fix approach:** Replace `mode = 'SNAPSHOT_THEN_CDC'` with `SCHEDULE CONTINUOUS` at the statement level (remove the WITH clause entry). The playground versions under `examples/playground/` already show the correct pattern — mirror that change to the three standalone files. Also remove `start_mode = 'snapshot_then_cdc'` from `examples/postgres-cdc/shuttle-backfill.sql`.

---

## R008 — Multi-Statement init.sql Dispatched as a Single Call

**Files affected:**
- `crates/datashuttle-playground-server/src/handlers.rs` lines 356–400 — `create_session` calls `dispatch_source_sql` with the raw multi-statement init_sql content
- `examples/postgres-cdc/init.sql` — 215 lines, ~8 distinct statements (CREATE TABLE ×5, INSERT ×3, UPDATE, DO $$)
- `examples/playground/mysql-cdc/init.sql` — multi-statement MySQL DDL + seed
- `examples/playground/large-payload/init.sql` — CREATE TABLE + INSERT
- `examples/playground/redis-streams-cdc/init.sql`, `examples/playground/redis-streams-events/init.sql` — Redis scripts

**Issue:** The `dispatch_source_sql` path for postgres passes the entire init_sql body to `exec_postgres_in_schema` which runs it as one sqlx `raw_sql` call. For MySQL it calls `exec_mysql_inner` which calls `conn.query_drop(sql)` on the entire body — `mysql_async` may or may not handle multi-statement bodies depending on client flags (not verified). For ClickHouse the dispatcher already correctly splits on `;` (see `split_clickhouse_statements` in `dispatcher.rs`), so that path is safe. The `shuttle_sql` path is correctly split via `split_ds_sql_statements` before sending to `/api/v1/sql`, but `init_sql` goes through the source dispatcher without splitting.

**Impact:** Medium. For the TCP dispatcher's postgres path, sqlx `execute_many` on `raw_sql` does handle multi-statement bodies. MySQL is the fragile case — `query_drop` with multi-statement content requires the `MULTI_STATEMENTS` client flag which is not explicitly set in `build_mysql_pool` in `dispatcher.rs`. If the MySQL sidecar enforces `multi_statements=0` (the default on strict servers), init.sql will fail silently after the first statement.

**Fix approach:** Mirror the ClickHouse pattern: split init_sql on `;` before dispatching for postgres and mysql source types. Reuse or generalize `split_ds_sql_statements` (already in `handlers.rs` line 1729) in `dispatch_source_sql` for all source types, executing each statement individually. This eliminates driver-specific multi-statement handling differences.

---

## R009 — Postgres Publication Name Convention Violations

**Files affected:**
- `examples/postgres-cdc/docker-init.sql` (line 98) — creates `CREATE PUBLICATION datashuttle_pub FOR ALL TABLES`
- `examples/postgres-cdc/shuttle-backfill.sql` (line 14) — `publication = 'datashuttle_pub'`
- `examples/postgres-cdc/shuttle.sql` (line 21) — `publication = 'datashuttle_pub'`

**Convention required:** `{pipeline}_pub` per R009; the playground session manager uses `{shuttle}_pub` as the per-session name (e.g. `pg_<8hex>_<8hex>_pub`). The validator `is_safe_playground_shuttle_artifact` in `tcp.rs` explicitly rejects `datashuttle_pub` (line 376 in test: `"datashuttle_pub" // legacy shared name`).

**Issue:** Three standalone example files still use the fixed `datashuttle_pub` name. On the shared cloud `postgres-playground` sidecar, two concurrent sessions that both run `CREATE PUBLICATION datashuttle_pub` will collide — the second `CREATE` fails (or the first session's replication breaks when the second session's `DROP PUBLICATION IF EXISTS datashuttle_pub` runs as part of init.sql cleanup, line 97 of `postgres-cdc/init.sql`).

**Additionally:** `examples/postgres-cdc/init.sql` line 97 issues a `DROP PUBLICATION IF EXISTS datashuttle_pub` as a one-time legacy cleanup. In cloud environments with concurrent sessions this DROP can silently destroy the replication publication belonging to another session that is still using the old name pattern.

**Impact:** High on cloud/shared sidecar. Session B's init tears down Session A's publication mid-stream, causing silent CDC gap.

**Fix approach:** Update `examples/postgres-cdc/shuttle.sql`, `examples/postgres-cdc/shuttle-backfill.sql`, and `examples/postgres-cdc/docker-init.sql` to use session-scoped publication names (`{shuttle}_pub` template in the playground variants). Remove the unconditional `DROP PUBLICATION IF EXISTS datashuttle_pub` from `examples/postgres-cdc/init.sql` now that the legacy transition period noted in the comment (`safe to remove once jarvis-cloud has rolled past 2026-05`) has passed.

---

## R010 — Smoke Validation Checks HTTP 200 Only, Not Row Counts

**Files affected:**
- `crates/datashuttle-playground-server/tests/handlers_smoke.rs` — all four tests only assert `StatusCode`
- `crates/datashuttle-playground-server/src/api_client.rs` — `exec_sql` returns `Value`, HTTP status captured but row counts are not extracted or returned to callers
- `crates/datashuttle-playground/src/manifest.rs` lines 98, 200–206 — `ExpectedOutcome` struct is defined and populated in `manifest.json` but never enforced at runtime

**Issue:** `ExpectedOutcome` entries in `examples/manifest.json` (e.g. `rows_landed >= 9600` for `postgres-cdc-ecommerce`, `dlq_count == 0` for fan-out, `dedup_gap_offsets == 0` for backfill) are declared but never evaluated. There is no code path in `handlers.rs` or `sessions.rs` that reads `scenario.expected_outcomes`, computes the metric, and compares it against the assertion. The smoke tests in `handlers_smoke.rs` use a `NoopPlaygroundDispatcher` (no sidecar) and only verify HTTP 200/201/401 status codes. The standalone `examples/*/verify.sh` scripts do query row counts but they are manual, not wired into any automated gate.

**Impact:** Critical. This is the primary correctness guardrail. A shuttle that silently drops all rows, misses the snapshot phase (R007), or fails schema evolution still returns HTTP 200 for every action. Regressions go undetected until a human runs a verify script manually.

**Fix approach:**
1. At session end (or as a background step after shuttle reaches `Running` status), query the metrics endpoint for `datashuttle_shuttle_rows_total` for the session's shuttle name and compare against the scenario's `expected_outcomes[metric=rows_landed]` entry.
2. Expose an `outcome_check` event on the session log that records pass/fail with the observed vs expected value.
3. Add at least one integration test that exercises the outcome check with a mock that returns a known row count response, asserting the check fails on mismatch.

---

## R006 — Sidecar Secret File Permission Mismatch

**Files affected:**
- `crates/datashuttle-playground-server/src/dispatcher.rs` lines 740–755 — `load_secret` function; comment at line 742 states the file mode is `0400`
- `docker/Dockerfile` — playground-server runs as `USER playground:playground` (non-root UID)
- No compose file in this repo defines Docker secrets with an explicit `mode` override

**Issue:** Docker secrets mounted at `/run/secrets/` default to `0400` (owner-readable only) with owner being the **container entrypoint UID** — which is `root` for most Docker secret implementations unless `mode` is set. The playground-server runs as the non-root `playground` user (created in `Dockerfile` lines 41–43). If the secret files are mounted `0400 root:root`, the `playground` user cannot read them. The code comment in `dispatcher.rs` says "the file mode is `0400` and only the api process can read it" — this is correct only when the secret is mounted with the correct UID or with mode `0640`/`0644`.

**No Docker compose file in this repo defines the secrets block with `file_mode`:** the standalone `docker/docker-compose.yml` has no secrets section at all; the cloud compose (not in this repo) must set `mode: 0644` or run the playground container as root for this to work.

**Impact:** High on cloud deployments. If mode is `0400 root:root`, every pool initialisation call returns `DispatchError::Config("playground password not found at /run/secrets/... and $DS_*_PLAYGROUND_PASSWORD unset")` and falls back to env-var. If env-vars are also absent the dispatcher fails hard on first use, breaking all source-SQL actions silently after session creation appears to succeed.

**Fix approach:** In the cloud compose (jarvis-cloud), add `mode: 0644` to each secret definition (or `uid: "<playground-uid>"`). Add a boot-time check in `build_pg_pool` / `build_mysql_pool` that logs a warning when the secret file exists but is unreadable (catching permission mismatches at pool init time rather than on first action). Document the required `mode: 0644` requirement in `docker/docker-compose.yml` as a comment.

---

## Tech Debt: MySQL `server_id` Collision for Concurrent Sessions

**Files affected:**
- `examples/playground/mysql-cdc/shuttle.sql` line 15 comment: `"the playground server should derive one from the namespace (TODO)"`

**Issue:** MySQL binlog replication requires each replication client to have a unique `server_id`. The playground MySQL shuttle template does not set `server_id`. The connector defaults to `9000001`. Two concurrent sessions on the same MySQL sidecar will both use `server_id=9000001`, causing the second connection to silently steal the binlog stream from the first. The first session's CDC feed stops without error.

**Impact:** Medium. Only manifests when two users run the `mysql-binlog-restart` or `large-payload` scenario at the same time on the cloud sidecar. Low probability in current playground traffic but deterministic failure when it occurs.

**Fix approach:** Derive a numeric `server_id` from the session namespace hash in `substitute_placeholders` (e.g. `u32` from the first 4 bytes of the SHA256 of `session.namespace`, bounded to `9000001..9999999` range to avoid conflicts with the production MySQL replication topology). Add a `{source_server_id}` placeholder and expand it in `substitute_source_coords`.

---

## Tech Debt: Quota Tracker is Per-Pod, Not Distributed

**Files affected:**
- `crates/datashuttle-playground/src/quota.rs` lines 1–15

**Issue:** `PlaygroundQuotaTracker` uses a per-process `Mutex<HashMap>`. The comment acknowledges that in a multi-pod deployment, a tenant can create `MAX_SESSIONS_PER_TENANT_PER_DAY * pods` sessions. The KV-backed migration is tracked for 9.4 but not yet landed.

**Impact:** Low in current single-pod deployment, medium in future horizontal scale.

**Fix approach:** When 9.4 KV store is available, replace `Mutex<HashMap>` with an atomic increment in the distributed KV, keyed by `(tenant, UTC date)`.

---

## Tech Debt: `session.status` Set to `Active` Before Shuttle Provisioning Completes

**Files affected:**
- `crates/datashuttle-playground-server/src/handlers.rs` lines 466–483

**Issue:** `create_session` sets `session.status = SessionStatus::Active` after the shuttle_sql loop, regardless of whether the shuttle provision succeeded or failed. If `shuttle-provision-failed` is recorded in the session events, the session is still `Active`. The UI renders the session as ready but actions will fail because the shuttle doesn't exist.

**Impact:** Medium. UX confusion — session appears live but all source actions return errors until the user notices the event log entry.

**Fix approach:** Set status to `Active` only when `provision_outcome` is `Ok`. On failure, set status to a new `ProvisionFailed` state (or remain `Provisioning`) so the UI can show a meaningful error state and offer a retry/reset button rather than presenting broken actions.

---

## Test Coverage Gaps

**Outcome enforcement (R010):**
- What's not tested: `ExpectedOutcome` evaluation against actual shuttle metrics
- Files: `crates/datashuttle-playground-server/tests/handlers_smoke.rs`, `crates/datashuttle-playground/src/manifest.rs`
- Risk: Scenarios with `expected_outcomes` silently pass even with zero rows landed
- Priority: High

**Multi-statement init_sql dispatch (R008):**
- What's not tested: MySQL multi-statement dispatch through `exec_mysql_in_database`; no test verifies that a two-statement init.sql body executes both statements correctly
- Files: `crates/datashuttle-playground-server/src/dispatcher.rs`
- Risk: MySQL init silently stops after first statement on strict servers
- Priority: Medium

**Publication name collision (R009):**
- What's not tested: Concurrent session creation with postgres source, asserting that two sessions get distinct publications
- Files: `crates/datashuttle-playground/src/sessions.rs`
- Risk: Silent CDC gap in cloud concurrent-session scenarios
- Priority: High

**Secret file permissions (R006):**
- What's not tested: `load_secret` behavior when file exists but is not readable (permission denied vs not found)
- Files: `crates/datashuttle-playground-server/src/dispatcher.rs`
- Risk: Production sidecar pool init fails silently, falls through to missing env-var error
- Priority: Medium

---

*Concerns audit: 2026-05-17*
