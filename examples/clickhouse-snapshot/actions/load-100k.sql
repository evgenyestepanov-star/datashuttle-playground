-- Playground action: load 100k synthetic web analytics events into the
-- source table so the clickhouse-high-cardinality scenario has enough
-- rows to show meaningful clustering improvements.
--
-- Column list matches `analytics.events` in init.sql (10 cols, last one
-- has DEFAULT now64(3) so we can skip it). Sidecar devices/regions go
-- into `properties` as JSON since the table schema doesn't expose them
-- as first-class columns.

INSERT INTO analytics.events
    (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    number                                                      AS id,
    -- 2M session ids, ~200 events per session on average
    (number * 17) % 2000000                                     AS session_id,
    -- 10k unique users distributed by zipf-ish modulo
    toUInt32(number % 10000)                                    AS user_id,
    arrayElement(
        ['page_view', 'click', 'scroll', 'submit', 'hover', 'purchase'],
        1 + (number % 6)
    )                                                           AS event_type,
    concat('evt_', toString(number % 1000))                     AS event_name,
    arrayElement(
        ['/home', '/product', '/cart', '/checkout', '/search', '/profile', '/blog'],
        1 + (number % 7)
    )                                                           AS page_url,
    -- Half the rows have a referrer, mimicking real clickstream sparsity
    if(
        number % 2 = 0,
        toNullable(arrayElement(['google', 'direct', 'twitter', 'reddit'], 1 + (number % 4))),
        NULL
    )                                                           AS referrer,
    concat(
        '{"device":"',
        arrayElement(['desktop', 'mobile', 'tablet'], 1 + (number % 3)),
        '","region":"',
        arrayElement(['us-east', 'us-west', 'eu-west', 'ap-south', 'sa-east'], 1 + (number % 5)),
        '","duration_ms":',
        toString((number * 31) % 1500),
        '}'
    )                                                           AS properties,
    now64(3) - toIntervalSecond(number % 2592000)               AS event_time
FROM numbers(100000);
