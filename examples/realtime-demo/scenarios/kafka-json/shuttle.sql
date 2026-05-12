-- Playground Tier-1 scenario: Kafka JSON poison message → DLQ.
--
-- Uses the Redpanda broker brought up under the `playground` profile
-- of examples/docker-compose.yml. The UI's "Inject poison message"
-- action publishes an unparseable raw payload; the shuttle's JSON
-- decoder rejects it into the DLQ. "Replay DLQ entries" then replays
-- the entry after the user fixes it through the inspector.

-- Source coordinates resolved at dispatch time via
-- `substitute_source_coords` from `DS_KAFKA_PLAYGROUND_HOST` /
-- `_PORT` (registered against the `redpanda` docker_service).
-- Local demo bundle falls back to `localhost:19092`; cloud
-- compose sets them to `redpanda-playground:9092`. {connection}
-- is also templated so two concurrent sessions don't collide on
-- the same connection name.
CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE KAFKA
  PROPERTIES (
    bootstrap_servers = '{source_host}:{source_port}',
    -- `database` is the kafka connector's logical tenant key
    -- (`kafka_schema.json` flags it required). For shuttles the
    -- `{shuttle}` UUID is a fine isolation-unique label.
    database          = '{shuttle}',
    topic             = '{shuttle}_src',
    group_id          = '{shuttle}_ds',
    value_format      = 'json',
    dlq_enabled       = 'true'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TARGET warehouse.{namespace}
  WITH (
    schedule        = 'continuous',
    realtime        = 'true',
    commit_interval = '5 seconds',
    batch_size      = '500'
  );

RESUME SHUTTLE {shuttle};
