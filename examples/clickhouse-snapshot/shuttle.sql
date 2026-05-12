-- DataShuttle ClickHouse Parallel Read Demo
--
-- Creates a connection to a 3-shard ClickHouse cluster and a snapshot shuttle.
-- The `cluster` option enables MPP parallel reads — DataShuttle discovers shards
-- via system.clusters and reads each shard directly.

CREATE CONNECTION clickhouse_cluster
  TYPE CLICKHOUSE
  WITH (
    host = 'localhost',
    port = '8123',
    database = 'analytics',
    cluster = 'analytics_cluster'
  );

-- Parallel snapshot: reads events from all 3 shards simultaneously
CREATE SHUTTLE ch_parallel_events
  SOURCE clickhouse_cluster
  SCHEMA analytics TABLE events
  TARGET warehouse.clickhouse
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '50000'
  );

-- Parallel snapshot: sessions
CREATE SHUTTLE ch_parallel_sessions
  SOURCE clickhouse_cluster
  SCHEMA analytics TABLE sessions
  TARGET warehouse.clickhouse
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '10000'
  );
