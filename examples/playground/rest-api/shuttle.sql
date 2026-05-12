-- Playground Tier-3 scenario: poll a WireMock REST endpoint every 5s.

CREATE CONNECTION IF NOT EXISTS playground_rest
  TYPE REST_API
  PROPERTIES (
    base_url      = 'http://localhost:8888',
    path          = '/api/users',
    method        = 'GET',
    poll_interval = '5s',
    pagination    = 'offset',
    page_size     = '100',
    response_root = '$.data'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE playground_rest
  TARGET warehouse.{namespace}
  WITH (
    schedule        = 'continuous',
    commit_interval = '10 seconds',
    batch_size      = '200'
  );

RESUME SHUTTLE {shuttle};
