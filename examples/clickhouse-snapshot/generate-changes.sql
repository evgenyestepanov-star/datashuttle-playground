-- Generate new data for the ClickHouse analytics demo.
-- Run while the shuttle is active to see incremental ingestion:
--   clickhouse-client --host localhost --port 9009 --database analytics < examples/clickhouse-snapshot/generate-changes.sql
--   OR: docker exec -i datashuttle-clickhouse-1 clickhouse-client --database analytics < examples/clickhouse-snapshot/generate-changes.sql

-- ── New users (10) ────────────────────────────────────

INSERT INTO analytics.users (id, username, email, full_name, country, city, plan, signup_date, last_login_at, is_active)
SELECT
    500 + number + 1 AS id,
    concat('new_user_', toString(number + 1)) AS username,
    concat('new_user_', toString(number + 1), '@example.com') AS email,
    concat(
        arrayElement(['Alice','Boris','Clara','Dmitry','Elena','Frank','Greta','Hugo','Irene','Jan'], number + 1),
        ' ',
        arrayElement(['Nakamura','Petrov','Schmidt','Chen','Santos','O''Brien','Rossi','Müller','Kim','Johansson'], number + 1)
    ) AS full_name,
    arrayElement(['US','RU','DE','CN','BR','IE','IT','AT','KR','SE'], number + 1) AS country,
    arrayElement(['Seattle','Moscow','Berlin','Shanghai','São Paulo','Dublin','Milan','Vienna','Seoul','Stockholm'], number + 1) AS city,
    arrayElement(['pro','enterprise','starter','pro','free','enterprise','pro','starter','enterprise','pro'], number + 1) AS plan,
    today() AS signup_date,
    now64(3) AS last_login_at,
    true AS is_active
FROM numbers(10);

-- ── New sessions (50) ─────────────────────────────────

INSERT INTO analytics.sessions (id, user_id, session_start, session_end, duration_sec, page_count, device_type, browser, os, utm_source, utm_medium, country, is_bounce)
SELECT
    2000 + number + 1 AS id,
    (number % 510) + 1 AS user_id,
    now64(3) - toIntervalSecond(number * 10) AS session_start,
    now64(3) - toIntervalSecond(number * 10) + toIntervalSecond(120 + (number * 11) % 600) AS session_end,
    120 + (number * 11) % 600 AS duration_sec,
    2 + (number % 15) AS page_count,
    arrayElement(['desktop','mobile','tablet'], (number % 3) + 1) AS device_type,
    arrayElement(['Chrome','Firefox','Safari'], (number % 3) + 1) AS browser,
    arrayElement(['Windows','macOS','Android'], (number % 3) + 1) AS os,
    if(number % 2 = 0, 'google', NULL) AS utm_source,
    if(number % 2 = 0, 'cpc', NULL) AS utm_medium,
    arrayElement(['US','GB','DE','FR','JP'], (number % 5) + 1) AS country,
    false AS is_bounce
FROM numbers(50);

-- ── New events (200) ──────────────────────────────────

INSERT INTO analytics.events (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    10000 + number + 1 AS id,
    (number % 50) + 2001 AS session_id,
    (number % 510) + 1 AS user_id,
    arrayElement(['page_view','click','purchase','add_to_cart'], (number % 4) + 1) AS event_type,
    concat(arrayElement(['page_view','click','purchase','add_to_cart'], (number % 4) + 1), '_live_', toString(number + 1)) AS event_name,
    concat('https://example.com/', arrayElement(['products','checkout','pricing','home','blog'], (number % 5) + 1)) AS page_url,
    if(number % 3 = 0, 'https://google.com/search', NULL) AS referrer,
    concat('{"live":true,"value":', toString((number * 7) % 500), '}') AS properties,
    now64(3) - toIntervalSecond(number * 5) AS event_time
FROM numbers(200);

-- ── New page views (150) ──────────────────────────────

INSERT INTO analytics.page_views (id, session_id, user_id, page_url, page_title, load_time_ms, scroll_depth, time_on_page, is_exit, viewed_at)
SELECT
    8000 + number + 1 AS id,
    (number % 50) + 2001 AS session_id,
    (number % 510) + 1 AS user_id,
    concat('https://example.com/', arrayElement(['products','checkout','pricing','home','blog'], (number % 5) + 1)) AS page_url,
    arrayElement(['Products','Checkout','Pricing','Home','Blog'], (number % 5) + 1) AS page_title,
    150 + (number * 13) % 2000 AS load_time_ms,
    toUInt8((number * 11) % 101) AS scroll_depth,
    10 + (number * 3) % 120 AS time_on_page,
    (number % 6) = 0 AS is_exit,
    now64(3) - toIntervalSecond(number * 8) AS viewed_at
FROM numbers(150);

-- ── Report ────────────────────────────────────────────

SELECT '── ClickHouse changes generated ──' AS info;
SELECT 'New users:      10' AS detail
UNION ALL SELECT 'New sessions:   50'
UNION ALL SELECT 'New events:     200'
UNION ALL SELECT 'New page views: 150';
