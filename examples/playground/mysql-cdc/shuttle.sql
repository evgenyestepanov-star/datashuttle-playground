-- Playground template for MySQL IoT CDC scenario. See
-- `examples/playground/postgres-cdc/shuttle.sql` for the substitution
-- model — {connection}/{shuttle}/{namespace} are rewritten per session.

-- Source coords are substituted from `DS_MYSQL_PLAYGROUND_*` env
-- (cloud sidecar) or the in-code defaults (`localhost:3306`, root /
-- rootpass on the examples docker-compose). `database` uses
-- `{namespace}` instead of a shared `iot` so each session is
-- isolated at the database level — matches what `exec_mysql_in_database`
-- provisions on cloud and what the shell-fallback's prepended
-- `CREATE DATABASE IF NOT EXISTS <ns>; USE <ns>;` creates locally.
-- `server_id` is intentionally not set — the connector's default
-- (9000001) is fine for single-session local-dev. Cloud deployments
-- with concurrent sessions need a per-session numeric server_id; the
-- playground server should derive one from the namespace (TODO).
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE MYSQL
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{namespace}',
    username = '{source_user}',
    password = '{source_password}'
  );

-- TABLES entries are single-quoted "schema.table" so the connector's
-- OOP wrapper doesn't fall back to its trait-default schema ("public"
-- — same fallback that bit postgres-cdc-ecommerce pre-fix). The
-- parser stores these literally; `ShuttleRecord::from_create`
-- splits on the first dot via `splitn(2, '.')` so the connector
-- gets {schema, table} pairs that resolve in our session DB.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (
    '{namespace}.devices',
    '{namespace}.readings',
    '{namespace}.alerts',
    '{namespace}.device_configs'
  )
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds',
    delete_mode = 'deletion_vectors',
    batch_size = '10000'
  );

RESUME SHUTTLE {shuttle};
