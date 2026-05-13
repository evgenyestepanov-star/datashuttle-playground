-- Playground init.sql for the ClickHouse snapshot scenarios.
-- Variant of `examples/clickhouse-snapshot/init.sql` with the
-- hard-coded `analytics` database removed: the playground dispatcher
-- prepends `CREATE DATABASE IF NOT EXISTS {namespace}; USE {namespace};`
-- to every source action, so tables land in the session's private DB
-- and the shuttle's `SCHEMA '{namespace}' TABLE events` reads from
-- the same place.

CREATE TABLE IF NOT EXISTS users
(
    id            UInt32,
    username      String,
    email         String,
    full_name     String,
    country       LowCardinality(String),
    city          String,
    plan          Enum8('free' = 1, 'starter' = 2, 'pro' = 3, 'enterprise' = 4),
    signup_date   Date,
    last_login_at DateTime64(3),
    is_active     Bool,
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (id)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS sessions
(
    id            UInt64,
    user_id       UInt32,
    session_start DateTime64(3),
    session_end   Nullable(DateTime64(3)),
    duration_sec  UInt32,
    page_count    UInt16,
    device_type   LowCardinality(String),
    browser       LowCardinality(String),
    os            LowCardinality(String),
    utm_source    Nullable(String),
    utm_medium    Nullable(String),
    utm_campaign  Nullable(String),
    country       LowCardinality(String),
    is_bounce     Bool,
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (user_id, session_start)
SETTINGS index_granularity = 8192;

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
    event_time    DateTime64(3),
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (user_id, event_time)
SETTINGS index_granularity = 8192;

INSERT INTO users (id, username, email, full_name, country, city, plan, signup_date, last_login_at, is_active)
SELECT
    number + 1 AS id,
    concat('user_', toString(number + 1)) AS username,
    concat('user', toString(number + 1), '@example.com') AS email,
    concat('User ', toString(number + 1)) AS full_name,
    arrayElement(['US','GB','DE','FR','CA','AU','JP','BR'], (number % 8) + 1) AS country,
    arrayElement(['NYC','LA','Chicago','London','Berlin','Paris','Toronto','Sydney'], (number % 8) + 1) AS city,
    arrayElement(['free','starter','pro','enterprise'], (number % 4) + 1) AS plan,
    toDate('2024-01-01') + toIntervalDay(number % 365) AS signup_date,
    now64(3) - toIntervalSecond(number * 120) AS last_login_at,
    (number % 10) != 0 AS is_active
FROM numbers(500);

INSERT INTO sessions (id, user_id, session_start, session_end, duration_sec, page_count, device_type, browser, os, utm_source, utm_medium, utm_campaign, country, is_bounce)
SELECT
    number + 1 AS id,
    (number % 500) + 1 AS user_id,
    now64(3) - toIntervalSecond(number * 300) AS session_start,
    now64(3) - toIntervalSecond(number * 300) + toIntervalSecond(60 + (number * 7) % 1800) AS session_end,
    60 + (number * 7) % 1800 AS duration_sec,
    1 + (number % 20) AS page_count,
    arrayElement(['desktop','mobile','tablet'], (number % 3) + 1) AS device_type,
    arrayElement(['Chrome','Firefox','Safari','Edge','Opera'], (number % 5) + 1) AS browser,
    arrayElement(['Windows','macOS','Linux','iOS','Android'], (number % 5) + 1) AS os,
    if(number % 3 = 0, arrayElement(['google','facebook','twitter','linkedin','direct'], (number % 5) + 1), NULL) AS utm_source,
    if(number % 3 = 0, arrayElement(['cpc','organic','social','email','referral'], (number % 5) + 1), NULL) AS utm_medium,
    if(number % 5 = 0, concat('campaign_', toString((number % 10) + 1)), NULL) AS utm_campaign,
    arrayElement(['US','GB','DE','FR','CA','AU','JP','BR'], (number % 8) + 1) AS country,
    (number % 8) = 0 AS is_bounce
FROM numbers(2000);

INSERT INTO events (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    number + 1 AS id,
    (number % 2000) + 1 AS session_id,
    (number % 500) + 1 AS user_id,
    arrayElement(['page_view','click','search','purchase','signup','add_to_cart','checkout'],
                 (number % 7) + 1) AS event_type,
    concat(
        arrayElement(['page_view','click','search','purchase','signup','add_to_cart','checkout'],
                     (number % 7) + 1),
        '_',
        toString((number % 100) + 1)
    ) AS event_name,
    concat('/', arrayElement(['home','products','pricing','blog','docs','signup','checkout'],
                              (number % 7) + 1)) AS page_url,
    if(number % 4 = 0,
       arrayElement(['https://google.com','https://facebook.com','https://twitter.com',''],
                    (number % 4) + 1),
       NULL) AS referrer,
    concat('{"button_id":"btn_', toString(number % 50), '","value":', toString((number * 13) % 1000), '}') AS properties,
    now64(3) - toIntervalSecond(number * 60) AS event_time
FROM numbers(10000);
