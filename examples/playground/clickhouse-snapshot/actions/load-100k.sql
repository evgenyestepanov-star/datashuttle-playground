-- Playground action: load 100k synthetic web analytics events.
-- Variant of `examples/clickhouse-snapshot/actions/load-100k.sql` with
-- the `analytics.` prefix removed — the playground dispatcher injects
-- `USE {namespace};` so the unqualified table resolves to the
-- session's private DB.

INSERT INTO events
    (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    1000000 + number                                            AS id,
    (number * 17) % 2000000                                     AS session_id,
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
