-- Playground Tier-1 scenario: Kafka throughput fast-forward (10k burst).
--
-- Shuttle is identical in structure to the JSON scenario but with a
-- larger hot-buffer ceiling so the burst action can fit in memory and
-- render a sub-millisecond latency in the dashboard's Arrow Flight
-- hot-buffer panel.

-- Source coordinates substituted from `DS_KAFKA_PLAYGROUND_*` —
-- see the sibling kafka-json scenario for the resolution chain.
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE KAFKA
  PROPERTIES (
    bootstrap_servers = '{source_host}:{source_port}',
    database          = '{shuttle}',
    topic             = '{shuttle}_src',
    group_id          = '{shuttle}_ds',
    value_format      = 'json'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TARGET warehouse.{namespace}
  WITH (
    schedule            = 'continuous',
    realtime            = 'true',
    commit_interval     = '5 seconds',
    batch_size          = '2000',
    hot_buffer_max_rows = '20000'
  );

RESUME SHUTTLE {shuttle};
