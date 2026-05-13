-- Playground Tier-3 scenario: poll a WireMock REST endpoint every 5s.
-- `{source_host}` / `{source_port}` resolve to the cloud-local
-- `wiremock-playground:8080` sidecar via DS_WIREMOCK_PLAYGROUND_* env;
-- the demo bundle falls back to localhost:8888 (the OSS docker-compose
-- exposes wiremock at that host port).
--
-- Per-session shuttles use `{connection}` (session-unique) so two
-- sessions don't race on a shared connection name.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE REST_API
  PROPERTIES (
    base_url      = 'http://{source_host}:{source_port}',
    path          = '/api/users',
    method        = 'GET',
    poll_interval = '5s',
    response_root = '$.data'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TARGET warehouse.{namespace}
  WITH (
    schedule        = 'continuous',
    commit_interval = '10 seconds',
    batch_size      = '200'
  );

RESUME SHUTTLE {shuttle};
