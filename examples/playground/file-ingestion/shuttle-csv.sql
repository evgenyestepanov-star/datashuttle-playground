-- Playground template for file-ingestion scenario — reads CSVs out of
-- the shared MinIO bucket. Connection is shared across sessions
-- (S3 credentials don't care about identity); shuttle is per-session.

CREATE CONNECTION IF NOT EXISTS playground_files_s3
  TYPE S3
  PROPERTIES (
    endpoint = 'http://localhost:9000',
    region = 'us-east-1',
    access_key = 'minioadmin',
    secret_key = 'minioadmin'
  );

-- S3 file-ingestion is poll-based (no change stream) — `SCHEDULE EVERY`
-- re-scans the prefix at the given interval. `mode = 'APPEND'` was the
-- pre-2025 opt that the parser now rejects; append semantics are the
-- default for S3 sources so dropping the key is safe.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_files_s3 PATH 's3://file-ingestion/csv/'
  TARGET warehouse.{namespace}
  SCHEDULE EVERY '30 seconds'
  WITH (
    file_pattern = '*.csv',
    commit_interval = '30 seconds'
  );

RESUME SHUTTLE {shuttle};
