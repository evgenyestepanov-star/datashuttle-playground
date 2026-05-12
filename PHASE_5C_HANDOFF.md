# Phase 5.C — playground extraction handoff

> Status as of 2026-05-12.

## What landed today (4 commits, 3 repos)

| Repo | SHA | Purpose |
|------|-----|---------|
| `datashuttle` | [`2fd6a81c`](https://github.com/evgenyestepanov-star/datashuttle/commit/2fd6a81c) | Unbreak `docker/Dockerfile` (Phase 6 retired the cli's `saas` umbrella; build now uses `--features postgres-registry,datashuttle-api-core/saas`). Drop the orphan `playground_postgres_cdc_cloud.rs` test. |
| `datashuttle-playground` | [`7747b3a`](https://github.com/evgenyestepanov-star/datashuttle-playground/commit/7747b3a) | Relocate `TcpPlaygroundDispatcher` (the sqlx + mysql_async concrete impl) from `datashuttle-cloud::playground` into `datashuttle-playground-server::dispatcher`. Add workspace deps. Gate under `#[allow(dead_code)]` until handler wiring lands. |
| `datashuttle-cloud` | [`88a451d`](https://github.com/evgenyestepanov-star/datashuttle-cloud/commit/88a451d) | Drop `src/playground/` (mod.rs + tcp.rs, 812 LOC), the matching integration test, and the now-orphaned deps (`mysql_async`, `testcontainers`, `testcontainers-modules`). Update api-core features (drop retired `saas-aws`) and control features (drop retired `saas`). Refresh OSS git pin to current main. Net: −2305 / +177 LOC. |
| `datashuttle` | [`a7b05db6`](https://github.com/evgenyestepanov-star/datashuttle/commit/a7b05db6) | The OSS reverse-proxy at `/api/v1/playground/*` now forwards authenticated identity as four trusted headers (`X-Datashuttle-User-Id`, `X-Datashuttle-Tenant-Id`, `X-Datashuttle-Actor-Id`, `X-Datashuttle-Auth-Method`). Inbound copies are stripped before forward so a malicious client can't spoof. |

Verified: cargo check green in all three repos, cargo test green in each.

## What is **not** done (deploy blockers)

### 1. Session-lifecycle HTTP handlers in `playground-server` (the big one)

Currently the standalone playground-server exposes only:

| Method | Path | Status |
|--------|------|--------|
| GET    | `/health` | ✅ implemented |
| GET    | `/metrics` | ✅ implemented |
| GET    | `/api/v1/playground/manifest` | ✅ implemented |
| GET    | `/api/v1/playground/health` | ✅ implemented |
| POST   | `/api/v1/playground/sessions` | ❌ NOT implemented |
| GET    | `/api/v1/playground/sessions` | ❌ NOT implemented |
| GET    | `/api/v1/playground/sessions/:id` | ❌ NOT implemented |
| DELETE | `/api/v1/playground/sessions/:id` | ❌ NOT implemented |
| POST   | `/api/v1/playground/sessions/:id/actions/:action_id` | ❌ NOT implemented |
| POST   | `/api/v1/playground/sessions/:id/reset` | ❌ NOT implemented |

The original handlers (2616 LOC) live in OSS git history before
commit [`c8959ae6`](https://github.com/evgenyestepanov-star/datashuttle/commit/c8959ae6)
("refactor(api,cli): remove legacy inline playground modules", 8 May).

Recover the source verbatim:

```sh
cd ~/git/datashuttle
git show c8959ae6^:crates/datashuttle-api-core/src/playground/handlers.rs \
    > ~/git/datashuttle-playground/crates/datashuttle-playground-server/src/handlers.rs
git show c8959ae6^:crates/datashuttle-api-core/src/playground/runtime.rs \
    > ~/git/datashuttle-playground/crates/datashuttle-playground-server/src/runtime.rs
```

Port surgery required (mechanical):

| Old | New | Why |
|-----|-----|-----|
| `use crate::auth::{AuthContext, effective_user_id, effective_tenant_id, ANONYMOUS_USER}` | A new `crate::identity` module that reads `X-Datashuttle-User-Id` / `X-Datashuttle-Tenant-Id` from headers (see [Identity model](#identity-model)) | Out-of-process: no JWT to parse, no `AuthContext` extension |
| `use crate::state::AppState` | `use crate::router::ServerState` | The playground-server has its own state struct |
| `use crate::handlers::shuttles::ErrorResponse` | Local `ErrorResponse` (re-define — it's a 2-field struct) | api-core's type isn't reachable |
| `state.playground_dispatcher` | Replace with the new `state.dispatcher` field (see [Wiring](#wiring)) | Different field name + type-erased trait |
| `playground_runtime(&state)` | **DEFER** — see [Shuttle runtime gap](#shuttle-runtime-gap) | Playground-server has no `ShuttleManager` |

### 2. Identity model

The OSS reverse-proxy forwards these as trusted headers:

```
X-Datashuttle-User-Id:    "<uuid>"
X-Datashuttle-Tenant-Id:  "<tenant-uuid>"     // optional
X-Datashuttle-Actor-Id:   "<admin-user-uuid>" // optional, impersonation
X-Datashuttle-Auth-Method: "oidc" | "api_key" | "basic"
```

Build a small middleware (`crates/datashuttle-playground-server/src/identity.rs`) that:

1. Reads `X-Datashuttle-User-Id` from the request. If absent, return 401 (the reverse-proxy doesn't set it unless the inbound request was authenticated).
2. Reads `X-Datashuttle-Tenant-Id` (optional).
3. Inserts an `Identity { user_id, tenant_id, actor_id, auth_method }` struct into request extensions.
4. Skips the check for `/health`, `/metrics`, `/api/v1/playground/health`, `/api/v1/playground/manifest`.

### 3. Wiring

`ServerState` (currently in `src/router.rs`) needs to gain:

```rust
pub struct ServerState {
    // ...existing fields...
    pub dispatcher: Arc<dyn PlaygroundDispatcher>,
}
```

And `main.rs` constructs it:

```rust
let dispatcher: Arc<dyn PlaygroundDispatcher> =
    Arc::new(dispatcher::build_dispatcher());
let state = Arc::new(ServerState { ..., dispatcher });
```

Then drop the `#[allow(dead_code)]` on `mod dispatcher;` once the
handlers use it.

### 4. Shuttle runtime gap

The original `playground_runtime(&state)` in api-core delegated shuttle
creation / start / teardown to the in-process `ShuttleManager`. The
playground-server doesn't have that — it lives outside api.

Two options, listed in increasing scope:

**(a) Defer shuttle integration** — port the handlers, no-op the shuttle
runtime calls (`session.shuttle_id = None`, "ok" responses for
start/teardown), only the source-side DDL/DML actions actually execute.
The Postgres CDC scenario won't be end-to-end without the shuttle, but
the UI loads, manifest renders, source actions run. Half-deploy, useful
for showcasing.

**(b) Reach back to api over HTTP** — playground-server creates shuttles
via the api's existing `POST /api/v1/shuttles` endpoint, authenticated
via the same shared bearer it receives requests through. Re-shapes the
extraction into a proper edge service. ~2-3 more days of work but
matches the architecture target.

Pick before re-implementing — (b) is correct, (a) is faster to
something deployable.

### 5. Quota scope

Phase 5.C agreed on **per-tenant** quotas
([conversation 2026-05-12]). `PlaygroundQuotaTracker` currently keys on
user_id. Two-line change: `key_by_tenant: bool` constructor arg, or
swap the map key from user_id to `tenant_id.unwrap_or(user_id)`.

### 6. Source-sidecar coverage

The dispatcher currently supports Postgres + MySQL only. The compose
that runs in [Task 11](#deploy) brings up sidecars for all of
postgres / mysql / mongo / clickhouse / cassandra / dynamodb-local /
cockroach / redpanda — but scenarios touching the 6 non-pg/mysql
sources will be `deployment-filtered` by the manifest until
`TcpPlaygroundDispatcher` learns more `exec_*` branches. That's a
separate phase, **not blocking deploy**.

## Deploy tasks (independent of Task 7)

### 7. Build images

```sh
# OSS api
cd ~/git/datashuttle
docker buildx build --platform linux/amd64 \
    -f docker/Dockerfile -t datashuttle:beta . 2>&1 | tee build.log

# Playground server
cd ~/git/datashuttle-playground
docker buildx build --platform linux/amd64 \
    -f docker/Dockerfile -t datashuttle-playground:dev . 2>&1 | tee build.log
```

Both should now succeed cleanly. First OSS build is ~15-25 min on a
warm cargo cache.

### 8. Local cloud compose

`deploy/jarvis-cloud-local/` in `datashuttle` repo (new directory):

* Strip Caddy from `deploy/jarvis-cloud/docker-compose.yaml`, publish
  `api:8080` on the host.
* Add `playground` service running `datashuttle-playground:dev`,
  `expose: 8081`, env: `PLAYGROUND_BIND_ADDR=0.0.0.0:8081`,
  `PLAYGROUND_TOKEN=<shared bearer with api>`, plus the
  `DS_PG_PLAYGROUND_*` / `DS_MYSQL_PLAYGROUND_*` envs and a volume
  mount for `<data_dir>/playground/sessions.json`.
* api gets `DS_PLAYGROUND_URL=http://playground:8081` and
  `DS_PLAYGROUND_TOKEN=<same shared bearer>`.
* Add source sidecars for `mongo`, `clickhouse`, `cassandra`,
  `dynamodb-local`, `cockroach`, `redpanda`. Pull definitions from
  `examples/docker-compose.yml` (the demo stack — they're already
  hardened).
* `.env.local` + `secrets/` per the existing
  `deploy/jarvis-cloud/README.md` layout.

```sh
cd ~/git/datashuttle
docker compose -f deploy/jarvis-cloud-local/docker-compose.yaml up -d --wait
docker compose -f deploy/jarvis-cloud-local/docker-compose.yaml exec api \
    datashuttle db migrate
open http://localhost:8080/ui
```

## Architecture diagram (target state)

```
┌──────────────────────────────────────────────────────────────────┐
│                          Browser / CLI                            │
│   sends:  Authorization: Bearer <user_jwt>                        │
└──────────────────────────────┬───────────────────────────────────┘
                               │  /api/v1/playground/*
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│           datashuttle-api  (OSS, port 8080)                       │
│                                                                   │
│   auth middleware → AuthContext (user_id, tenant_id, …)           │
│                          │                                        │
│                          ▼                                        │
│   handlers/playground_proxy.rs                                    │
│     • strip Authorization                                         │
│     • set Authorization: Bearer <PLAYGROUND_TOKEN>                │
│     • set X-Datashuttle-User-Id / Tenant-Id / Actor-Id / Auth-M.  │
│     • POST to playground.url + path                               │
└──────────────────────────────┬───────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│   datashuttle-playground-server  (port 8081, internal-only)       │
│                                                                   │
│   middleware: identity.rs reads X-Datashuttle-* → Identity ext.   │
│                          │                                        │
│                          ▼                                        │
│   handlers.rs (PORT FROM `c8959ae6^`)                             │
│     • SessionManager (from datashuttle-playground lib)            │
│     • PlaygroundQuotaTracker  (per-tenant key)                    │
│     • PlaygroundMetrics                                           │
│     • dispatcher.rs → TcpPlaygroundDispatcher                     │
│                          │                                        │
│                          ▼                                        │
│   sqlx::PgPool / mysql_async::Pool                                │
└──────────────┬─────────────────────────┬────────────────────────┘
               │                          │
               ▼                          ▼
   postgres-playground:5432    mysql-playground:3306
   (DS_PG_PLAYGROUND_*)        (DS_MYSQL_PLAYGROUND_*)
```

## Open questions

1. **Shuttle runtime gap (#4 above)** — pick (a) defer / (b) reach back
   over HTTP. (b) is correct; (a) is faster to a half-demo.
2. **Persistence file path** — `<data_dir>/playground/sessions.json`
   today (foundation lib default). Should `data_dir` be configurable
   via `PLAYGROUND_DATA_DIR` env? Probably yes.
3. **Manifest reload** — currently loaded once at boot. Manifest
   updates need a server restart. Hot-reload (signal-based or
   filesystem-watcher) is a future enhancement, not blocking.
4. **Telemetry** — the OSS api auto-instruments all routes with
   `tracing` spans. The playground-server should do the same;
   already uses `tracing_subscriber::fmt().json()` but no per-request
   middleware. Add `tower-http::trace::TraceLayer`.

## How to pick this up

Next session, start with:

```sh
cd ~/git/datashuttle
git log --oneline -10
# Verify a7b05db6 is on origin/main

cd ~/git/datashuttle-playground
git log --oneline -5
# Verify 7747b3a is on origin/main

cd ~/git/datashuttle-cloud
git log --oneline -5
# Verify 88a451d is on origin/main
```

Then either:

A. Port the handlers (Task 7 above). Largest chunk; ~3-5 hours
   focused, depends on the shuttle-gap decision.
B. Build the deploy infrastructure (Tasks 7-8) **without handlers**
   and accept that session-create returns 501 in the UI. Lets you
   demo manifest + config endpoints; useful for stakeholder showcase.

— Phase 5.C handoff written 2026-05-12.
