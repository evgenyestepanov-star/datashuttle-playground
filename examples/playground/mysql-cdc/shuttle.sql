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
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE MYSQL
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{namespace}',
    username = '{source_user}',
    password = '{source_password}',
    server_id = '{shuttle}'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (devices, readings, alerts, device_configs)
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds',
    delete_mode = 'deletion_vectors',
    batch_size = '10000'
  );

RESUME SHUTTLE {shuttle};
