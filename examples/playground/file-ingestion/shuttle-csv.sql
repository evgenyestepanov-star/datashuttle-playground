-- Playground template for file-ingestion scenario.
--
-- Each session uploads CSVs into its own prefix under the shared
-- MinIO bucket (`file-ingestion/{namespace}/`). The shuttle polls
-- only that prefix so concurrent sessions don't see each other's
-- files.
--
-- TYPE FILE is the registered connector name — `TYPE S3` is the
-- URI scheme used in PATH, not a connector type.

-- `format` lives on the CONNECTION rather than the SHUTTLE WITH clause
-- because run_loop_chunks/setup.rs only propagates connection.options
-- into the connector's `properties` map. Shuttle WITH options stay on
-- record.options and don't reach the file connector's config (only
-- `source_path` → `base_uri` is special-cased). Putting `format` here
-- keeps the file sidecar's FileConfig getting the right value.
CREATE CONNECTION IF NOT EXISTS playground_files_s3
  TYPE FILE
  PROPERTIES (
    endpoint = '{minio_endpoint}',
    region = 'us-east-1',
    access_key = '{minio_access_key}',
    secret_key = '{minio_secret_key}',
    format = 'csv'
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
-- The file connector's get_schema polls the prefix for up to 60s
-- waiting for the first matching file, so it's fine to create the
-- session first and have the user click upload-csv afterwards.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_files_s3 PATH 's3://file-ingestion/{namespace}/'
  TARGET warehouse.{namespace}
  SCHEDULE EVERY '30 seconds'
  WITH (
    format = 'csv',
    commit_interval = '30 seconds'
  );

RESUME SHUTTLE {shuttle};
