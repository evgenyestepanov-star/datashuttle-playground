-- Playground Tier-4 scenario: large-payload mysql replication.
-- {source_*} placeholders resolve to the cloud-local
-- `mysql-playground:3306` sidecar; demo bundle falls back to
-- `localhost:3306` with root creds.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE MYSQL
  PROPERTIES (
    host     = '{source_host}',
    port     = '{source_port}',
    database = '{namespace}',
    username = '{source_user}',
    password = '{source_password}'
  );

-- MySQL treats `schema` as database; SCHEMA '{namespace}' makes the
-- OOP connector's table discovery look in the session's private DB
-- instead of falling back to its default ("public"). See
-- [[file-connector-empty-source-blocker-2026-05-13]] /
-- [[clickhouse-cloud-2026-05-13]] for the same pattern.
CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  SCHEMA '{namespace}' TABLE blobs
  TARGET warehouse.{namespace}
  WITH (
    schedule           = 'continuous',
    commit_interval    = '30 seconds',
    batch_size         = '10',
    max_payload_bytes  = '62914560'
  );

RESUME SHUTTLE {shuttle};
