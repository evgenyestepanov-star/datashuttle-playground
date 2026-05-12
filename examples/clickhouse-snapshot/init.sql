-- DataShuttle ClickHouse Snapshot Demo: Web Analytics Schema
-- Initializes: events, sessions, users, page_views
-- Designed for snapshot + incremental demo: realistic clickstream data, sorting keys, DateTime64.

-- ── Schema ────────────────────────────────────────────

CREATE DATABASE IF NOT EXISTS analytics;

CREATE TABLE IF NOT EXISTS analytics.users
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

CREATE TABLE IF NOT EXISTS analytics.sessions
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

CREATE TABLE IF NOT EXISTS analytics.events
(
    id            UInt64,
    session_id    UInt64,
    user_id       UInt32,
    event_type    LowCardinality(String),
    event_name    String,
    page_url      String,
    referrer      Nullable(String),
    properties    String,  -- JSON string
    event_time    DateTime64(3),
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (user_id, event_time)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS analytics.page_views
(
    id            UInt64,
    session_id    UInt64,
    user_id       UInt32,
    page_url      String,
    page_title    String,
    load_time_ms  UInt32,
    scroll_depth  UInt8,
    time_on_page  UInt32,
    is_exit       Bool,
    viewed_at     DateTime64(3),
    created_at    DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (user_id, viewed_at)
SETTINGS index_granularity = 8192;

-- ── Seed data: Users (500) ────────────────────────────

INSERT INTO analytics.users (id, username, email, full_name, country, city, plan, signup_date, last_login_at, is_active)
SELECT
    number + 1 AS id,
    concat('user_', toString(number + 1)) AS username,
    concat('user', toString(number + 1), '@example.com') AS email,
    concat(
        arrayElement(['James','Mary','John','Patricia','Robert','Jennifer','Michael','Linda',
                       'David','Elizabeth','William','Barbara','Richard','Susan','Joseph','Jessica',
                       'Thomas','Sarah','Charles','Karen','Emma','Oliver','Ava','Liam'], (number % 24) + 1),
        ' ',
        arrayElement(['Smith','Johnson','Williams','Brown','Jones','Garcia','Miller','Davis',
                       'Rodriguez','Martinez','Hernandez','Lopez','Gonzalez','Wilson','Anderson',
                       'Thomas','Taylor','Moore','Jackson','Martin','Lee','Perez','White','Harris'], (number % 24) + 1)
    ) AS full_name,
    arrayElement(['US','US','US','GB','DE','FR','CA','AU','JP','BR'], (number % 10) + 1) AS country,
    arrayElement(['New York','Los Angeles','Chicago','London','Berlin','Paris','Toronto','Sydney',
                  'Tokyo','São Paulo','Seattle','Boston','Miami','Denver'], (number % 14) + 1) AS city,
    arrayElement(['free','starter','pro','enterprise','free','free','starter','pro'], (number % 8) + 1) AS plan,
    toDate('2023-01-01') + toIntervalDay(number % 730) AS signup_date,
    now64(3) - toIntervalSecond(number * 120) AS last_login_at,
    (number % 10) != 0 AS is_active
FROM numbers(500);

-- ── Seed data: Sessions (2,000) ───────────────────────

INSERT INTO analytics.sessions (id, user_id, session_start, session_end, duration_sec, page_count, device_type, browser, os, utm_source, utm_medium, utm_campaign, country, is_bounce)
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
    arrayElement(['US','US','US','GB','DE','FR','CA','AU','JP','BR'], (number % 10) + 1) AS country,
    (number % 8) = 0 AS is_bounce
FROM numbers(2000);

-- ── Seed data: Events (10,000) ────────────────────────

INSERT INTO analytics.events (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    number + 1 AS id,
    (number % 2000) + 1 AS session_id,
    (number % 500) + 1 AS user_id,
    arrayElement(['page_view','click','search','purchase','signup','add_to_cart','remove_from_cart','checkout'],
                 (number % 8) + 1) AS event_type,
    concat(
        arrayElement(['page_view','click','search','purchase','signup','add_to_cart','remove_from_cart','checkout'],
                     (number % 8) + 1),
        '_',
        toString((number % 100) + 1)
    ) AS event_name,
    concat('https://example.com/',
           arrayElement(['home','products','pricing','about','blog','docs','signup','checkout','search','profile'],
                        (number % 10) + 1)) AS page_url,
    if(number % 4 = 0,
       arrayElement(['https://google.com','https://facebook.com','https://twitter.com','https://linkedin.com',''],
                    (number % 5) + 1),
       NULL) AS referrer,
    concat('{"button_id":"btn_', toString(number % 50), '","value":', toString((number * 13) % 1000), '}') AS properties,
    now64(3) - toIntervalSecond(number * 60) AS event_time
FROM numbers(10000);

-- ── Seed data: Page Views (8,000) ─────────────────────

INSERT INTO analytics.page_views (id, session_id, user_id, page_url, page_title, load_time_ms, scroll_depth, time_on_page, is_exit, viewed_at)
SELECT
    number + 1 AS id,
    (number % 2000) + 1 AS session_id,
    (number % 500) + 1 AS user_id,
    concat('https://example.com/',
           arrayElement(['home','products','pricing','about','blog','docs','signup','checkout','search','profile'],
                        (number % 10) + 1)) AS page_url,
    arrayElement(['Home','Products','Pricing','About Us','Blog','Documentation','Sign Up','Checkout','Search','Profile'],
                 (number % 10) + 1) AS page_title,
    200 + (number * 17) % 3000 AS load_time_ms,
    toUInt8((number * 7) % 101) AS scroll_depth,
    5 + (number * 3) % 300 AS time_on_page,
    (number % 5) = 0 AS is_exit,
    now64(3) - toIntervalSecond(number * 75) AS viewed_at
FROM numbers(8000);

-- ── Summary ───────────────────────────────────────────

SELECT '── DataShuttle ClickHouse demo data loaded ──' AS info;
SELECT 'users' AS tbl, count() AS cnt FROM analytics.users
UNION ALL SELECT 'sessions', count() FROM analytics.sessions
UNION ALL SELECT 'events', count() FROM analytics.events
UNION ALL SELECT 'page_views', count() FROM analytics.page_views;
