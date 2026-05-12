CREATE CONNECTION IF NOT EXISTS playground_large
  TYPE MYSQL
  PROPERTIES (
    host     = 'localhost',
    port     = '3306',
    database = 'large_payload',
    username = 'root',
    password = 'rootpass'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_large
  TABLE blobs
  TARGET warehouse.{namespace}
  WITH (
    schedule           = 'continuous',
    commit_interval    = '30 seconds',
    batch_size         = '10',
    max_payload_bytes  = '62914560'
  );

RESUME SHUTTLE {shuttle};
