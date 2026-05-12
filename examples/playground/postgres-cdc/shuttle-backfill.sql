-- Playground variant — full-table snapshot then CDC. See
-- `shuttle.sql` in this directory for the placeholder model.

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

-- `SCHEDULE CONTINUOUS` at the top level replaces the pre-2025
-- `mode = 'SNAPSHOT_THEN_CDC'` + `schedule = 'continuous'` options;
-- the parser rejects `MODE` and treats unknown `start_mode` as an
-- extra (silently ignored). Snapshot-then-follow is already the
-- default for a postgres logical-replication source, so SCHEDULE
-- CONTINUOUS alone is enough to get both phases.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (
    '{namespace}.orders',
    '{namespace}.customers',
    '{namespace}.products'
  )
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '5 seconds',
    batch_size = '1000'
  );

RESUME SHUTTLE {shuttle};
