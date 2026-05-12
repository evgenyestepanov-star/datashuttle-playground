-- Parquet shuttle.
-- Run: datashuttle sql -f examples/file-ingestion/shuttle-parquet.sql

CREATE SHUTTLE parquet_transactions
  SOURCE demo_s3 PATH 's3://file-ingestion/parquet/'
  TARGET warehouse.analytics
  WITH (
    mode = 'APPEND',
    file_pattern = '*.parquet',
    commit_interval = '30 seconds'
  );
