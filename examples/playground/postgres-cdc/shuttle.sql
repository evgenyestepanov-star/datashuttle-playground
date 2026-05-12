-- Playground scenario template for PostgreSQL CDC (#752.10).
--
-- This file is applied by the Playground session manager through
-- `substitute_placeholders` — every `{shuttle}`/`{connection}`/
-- `{namespace}` token is rewritten to a session-unique string before
-- the SQL lands on /api/v1/sql. Two concurrent sessions therefore get
-- their OWN connection, shuttle, and Postgres replication slot
-- without colliding.
--
-- init.sql (run per-session on cloud, once-at-boot locally) creates
-- seed data + a session-scoped publication named `{shuttle}_pub`
-- that covers only this session's schema. Previous versions consumed
-- a shared cluster-wide `datashuttle_pub`; that renamed in 2026-04
-- to fix cloud-side collisions between concurrent sessions.
--
-- `CREATE ... IF NOT EXISTS` keeps retries idempotent — a resumed
-- session lands back on the same shuttle+connection pair without
-- a 409.

-- Source coordinates (`{source_host}`, `{source_port}`, `{source_db}`,
-- `{source_user}`, `{source_password}`) are substituted at dispatch
-- time from the api's `DS_PG_PLAYGROUND_*` env block — the same pool
-- the playground TCP dispatcher already uses for
-- `exec_postgres_in_schema`. Local demo bundle falls back to the
-- in-code defaults (`localhost:5432/ecommerce` as `postgres`);
-- on jarvis-cloud the substitution resolves to the
-- `postgres-playground` sidecar. Implementation:
-- `substitute_source_coords` in
-- `crates/datashuttle-api/src/handlers/playground.rs`.
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE POSTGRES
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{source_db}',
    username = '{source_user}',
    password = '{source_password}',
    replication_slot = '{shuttle}_slot',
    publication = '{shuttle}_pub'
  );

-- `SCHEDULE CONTINUOUS` replaces the pre-2025 `mode = 'SNAPSHOT_THEN_CDC'`
-- option — CDC source + continuous schedule is how the parser now
-- represents a streaming shuttle. Snapshot-then-follow is the
-- default for a postgres logical-replication source.
--
-- Each table is fully-qualified with the session's postgres schema.
-- Single-quoted strings are required because the DataShuttle SQL
-- parser's `expect_identifier` rejects `schema.table` as a bare
-- identifier (a dot terminates the identifier, leaving
-- `.customers` unparsable) — but it accepts single-quoted strings
-- verbatim, preserving the dot for downstream splitting.
-- `ShuttleRecord::from_create` stores TABLES entries literally, and
-- `shuttle_manager::tables_to_process` splits on the first dot via
-- `splitn(2, '.')`. Without qualification tables without a dot fall
-- back to the connector's `default_schema` (`public` for postgres),
-- but init.sql seeds the demo tables into `playground_{uuid}` on the
-- shared sidecar via the TCP dispatcher's `SET LOCAL search_path`,
-- so we must target the qualified names for discovery + replication.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (
    '{namespace}.customers',
    '{namespace}.products',
    '{namespace}.orders',
    '{namespace}.order_items',
    '{namespace}.payments'
  )
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds',
    delete_mode = 'deletion_vectors',
    schema_evolution = 'compatible',
    parallelism = '4',
    batch_size = '5000',
    -- 11.A.2 — realtime=true populates the Arrow Flight hot buffer
    -- so the Playground FlightMonitor widget actually shows rows as
    -- they arrive from the source, not just the "Waiting for first
    -- row…" empty state. Playground data is small (ecommerce seed
    -- ~ hundreds of rows), so the default 100k-row buffer cap is
    -- well-oversized. Iceberg commit behavior is unchanged —
    -- commit_interval = '10 seconds' still drives flushes.
    realtime = 'true'
  );

RESUME SHUTTLE {shuttle};
