-- Playground template for ClickHouse parallel snapshot scenario. Two
-- shuttles — one per table — both share the same session connection
-- but get distinct shuttle names so the UI can report them
-- independently.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE CLICKHOUSE
  WITH (
    host = 'localhost',
    port = '8123',
    database = 'analytics',
    cluster = 'analytics_cluster'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  SCHEMA analytics TABLE events
  TARGET warehouse.{namespace}
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '50000'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}_sessions
  SOURCE {connection}
  SCHEMA analytics TABLE sessions
  TARGET warehouse.{namespace}
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '10000'
  );

RESUME SHUTTLE {shuttle};
RESUME SHUTTLE {shuttle}_sessions;
