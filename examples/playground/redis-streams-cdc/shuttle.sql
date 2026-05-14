-- Playground template for redis-streams-cdc.
--
-- `schedule = 'continuous'` puts the api in Stream mode → the
-- connector's `plan(ReadMode::Stream)` emits cdc-* shards whose
-- `open_shard` runs the long-lived XREADGROUP loop in
-- `crates/datashuttle-connector-redis/src/cdc.rs`.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE REDIS
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{source_db}',
    username = '{source_user}',
    password = '{source_password}'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (
    'redis.{namespace}:events'
  )
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds',
    batch_size = '5000'
  );

RESUME SHUTTLE {shuttle};
