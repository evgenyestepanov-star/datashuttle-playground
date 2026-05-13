-- Playground template for file-ingestion scenario.
--
-- Each session uploads CSVs into its own prefix under the shared
-- MinIO bucket (`file-ingestion/{namespace}/`). The shuttle polls
-- only that prefix so concurrent sessions don't see each other's
-- files.
--
-- TYPE FILE is the registered connector name — `TYPE S3` is the
-- URI scheme used in PATH, not a connector type.

CREATE CONNECTION IF NOT EXISTS playground_files_s3
  TYPE FILE
  PROPERTIES (
    endpoint = 'http://minio:9000',
    region = 'us-east-1',
    access_key = 'minioadmin',
    secret_key = 'minioadmin'
  );

-- S3 file-ingestion is poll-based — the file connector polls the
-- prefix continuously (SCHEDULE EVERY is parsed but the executor
-- treats every shuttle as continuous; see SPEC §2065).
--
-- `path_pattern` is intentionally omitted: driver::matches_file uses
-- substring matching for path_pattern (not globset), so `*.csv`
-- would filter out every real path. The format=csv filter on
-- extension is sufficient to pick up only CSV files.
--
-- NOTE: the file connector errors at create-time if the prefix has
-- no matching files (schema can't be inferred). Sessions are
-- expected to upload at least one CSV before the snapshot poll
-- runs. Until then the shuttle sits in `error` state and recovers
-- on the next poll cycle once a file lands.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_files_s3 PATH 's3://file-ingestion/{namespace}/'
  TARGET warehouse.{namespace}
  SCHEDULE EVERY '30 seconds'
  WITH (
    format = 'csv',
    commit_interval = '30 seconds'
  );

RESUME SHUTTLE {shuttle};
