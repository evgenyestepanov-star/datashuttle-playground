-- Playground action: simulate a "bad write" with sentinel markers
-- so the clickhouse-time-travel scenario can roll back to a clean
-- snapshot. Unqualified table — the dispatcher injects `USE {namespace};`.

INSERT INTO events
    (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    9000000 + number                       AS id,
    0                                      AS session_id,
    999999                                 AS user_id,
    'bad_event'                            AS event_type,
    'bad_write'                            AS event_name,
    '/bad'                                 AS page_url,
    NULL                                   AS referrer,
    '{"device":"broken","region":"unknown"}' AS properties,
    now64(3)                               AS event_time
FROM numbers(5000);
