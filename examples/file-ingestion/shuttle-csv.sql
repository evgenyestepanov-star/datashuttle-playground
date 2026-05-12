-- Shuttles for file ingestion demo.
-- Run: datashuttle sql -f examples/file-ingestion/shuttle-csv.sql

CREATE CONNECTION demo_s3
  TYPE S3
  PROPERTIES (
    endpoint = 'http://localhost:9000',
    region = 'us-east-1',
    access_key = 'minioadmin',
    secret_key = 'minioadmin'
  );

CREATE SHUTTLE csv_events
  SOURCE demo_s3 PATH 's3://file-ingestion/csv/'
  TARGET warehouse.analytics
  WITH (
    mode = 'APPEND',
    file_pattern = '*.csv',
    commit_interval = '30 seconds'
  );
