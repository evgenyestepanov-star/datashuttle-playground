-- Playground Tier-3 scenario: DynamoDB Streams → Iceberg.

CREATE CONNECTION IF NOT EXISTS playground_dynamo
  TYPE DYNAMODB
  PROPERTIES (
    endpoint         = 'http://localhost:8000',
    region           = 'us-east-1',
    access_key       = 'local',
    secret_key       = 'local',
    table            = 'playground_items',
    stream_view_type = 'NEW_AND_OLD_IMAGES'
  );

-- `SCHEDULE CONTINUOUS` at the top-level replaces the pre-2025
-- `schedule = 'continuous'` WITH option; the parser now rejects
-- `MODE`/`SCHEDULE` inside WITH.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_dynamo
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '5 seconds',
    batch_size      = '100'
  );

RESUME SHUTTLE {shuttle};
