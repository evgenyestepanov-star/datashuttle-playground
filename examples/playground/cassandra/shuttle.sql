CREATE CONNECTION IF NOT EXISTS playground_cassandra
  TYPE CASSANDRA
  PROPERTIES (
    contact_points = 'localhost',
    port           = '9042',
    keyspace       = 'playground',
    table          = 'wide_rows'
  );

-- `SCHEDULE CONTINUOUS` at the top-level — the parser rejects the
-- pre-2025 `schedule = 'continuous'` WITH key. Same migration
-- b2db7b9 applied to the postgres/mysql/mongodb/file templates.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_cassandra
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds',
    batch_size      = '200'
  );

RESUME SHUTTLE {shuttle};
