-- Distributed tables (created on coordinator, routes to shards)

CREATE TABLE IF NOT EXISTS events
(
    id            UInt64,
    session_id    UInt64,
    user_id       UInt32,
    event_type    LowCardinality(String),
    event_name    String,
    page_url      String,
    referrer      Nullable(String),
    properties    String,
    region        LowCardinality(String),
    device_type   LowCardinality(String),
    event_time    DateTime64(3),
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = Distributed('analytics_cluster', 'analytics', 'events_local', rand());

CREATE TABLE IF NOT EXISTS sessions
(
    id            UInt64,
    user_id       UInt32,
    session_start DateTime64(3),
    duration_sec  UInt32,
    page_count    UInt16,
    device_type   LowCardinality(String),
    country       LowCardinality(String),
    is_bounce     Bool,
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = Distributed('analytics_cluster', 'analytics', 'sessions_local', rand());
