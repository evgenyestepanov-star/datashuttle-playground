CREATE CONNECTION IF NOT EXISTS playground_kinesis
  TYPE KINESIS
  PROPERTIES (
    -- `localstack:4566` only resolves inside the docker-compose network;
    -- the playground binary runs on the host, so we point at the
    -- host-exposed port (see docker-compose.yml's `ports: 4566:4566`).
    -- a428543 normalised the rest of the templates the same way.
    endpoint    = 'http://localhost:4566',
    region      = 'us-east-1',
    access_key  = 'local',
    secret_key  = 'local',
    stream_name = 'playground-events'
  );

-- `SCHEDULE CONTINUOUS` at the top level replaces the pre-2025
-- `schedule = 'continuous'` WITH option; the parser now rejects that
-- under WITH with "MODE is no longer supported ..." (same migration
-- b2db7b9 applied to the postgres/mysql/mongodb/file templates).
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_kinesis
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '5 seconds',
    batch_size      = '500'
  );

RESUME SHUTTLE {shuttle};
