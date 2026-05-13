-- Playground template for ClickHouse snapshot scenarios. ClickHouse
-- treats "database" as the schema; the per-session isolation injects
-- `USE {namespace}` so init.sql / actions land in the session's
-- private database, and we set the connection's `database` field +
-- the shuttle's SCHEMA to `{namespace}` so the connector reads from
-- the same isolated DB.
--
-- `{source_host}` / `{source_port}` / `{source_user}` / `{source_password}`
-- are populated by `substitute_source_coords` from the api's
-- `DS_CLICKHOUSE_PLAYGROUND_*` env block. On the local demo bundle
-- they fall through to `clickhouse-playground:8123` as `playground`.
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE CLICKHOUSE
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{namespace}',
    username = '{source_user}',
    password = '{source_password}'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  SCHEMA '{namespace}' TABLE events
  TARGET warehouse.{namespace}
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '50000'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}_sessions
  SOURCE {connection}
  SCHEMA '{namespace}' TABLE sessions
  TARGET warehouse.{namespace}
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '10000'
  );

RESUME SHUTTLE {shuttle};
RESUME SHUTTLE {shuttle}_sessions;
