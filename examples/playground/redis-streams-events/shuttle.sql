-- Playground template for the redis-streams-events scenario.
--
-- Source coords are substituted from `DS_REDIS_PLAYGROUND_*` env (cloud
-- sidecar) or the registry defaults (`redis-playground:6379`, db 0,
-- no auth). The connector itself reads stream entries via XRANGE
-- pagination and emits each entry as one Arrow row with the fixed
-- shape `(stream_key, stream_id, fields_json)`.
--
-- Per-session isolation is via key prefix: init.sql XADDs into
-- `{namespace}:events`, and the SHUTTLE TABLES list points at the
-- same key so the connector knows where to read from.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE REDIS
  PROPERTIES (
    host = '{source_host}',
    port = '{source_port}',
    database = '{source_db}',
    username = '{source_user}',
    password = '{source_password}'
  );

-- TABLES entries are `schema.name`; the OopGenericConnectorFactory's
-- splitn(2, '.') sends `{schema: "redis", name: "<namespace>:events"}`
-- to the sidecar, which uses the name verbatim as the Redis stream
-- key. `redis` is purely a label here — Redis has no schema concept.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (
    'redis.{namespace}:events'
  )
  TARGET warehouse.{namespace}
  WITH (
    schedule = 'every 5 minutes',
    batch_size = '5000'
  );

RESUME SHUTTLE {shuttle};
