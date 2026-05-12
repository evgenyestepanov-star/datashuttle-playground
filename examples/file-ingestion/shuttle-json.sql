-- JSON Lines shuttle.
-- Run: datashuttle sql -f examples/file-ingestion/shuttle-json.sql

CREATE SHUTTLE json_logs
  SOURCE demo_s3 PATH 's3://file-ingestion/json/'
  TARGET warehouse.analytics
  WITH (
    mode = 'APPEND',
    file_pattern = '*.json',
    commit_interval = '30 seconds'
  );
