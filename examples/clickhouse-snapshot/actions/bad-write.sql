-- Playground action: simulate a "bad write" by inserting 5000 rows with
-- a deliberately skewed user, then the time-travel scenario uses a
-- rollback to undo these without dropping the whole table.
--
-- Column list matches `analytics.events` in init.sql. Sentinel markers
-- (user_id=999999, event_type='bad_event') let the time-travel demo
-- identify the bad rows when inspecting which partition to roll back.

INSERT INTO analytics.events
    (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, event_time)
SELECT
    1000000 + number                       AS id,       -- high id range = bad-write marker
    0                                      AS session_id,
    999999                                 AS user_id,  -- sentinel bad user
    'bad_event'                            AS event_type,
    'bad_write'                            AS event_name,
    '/bad'                                 AS page_url,
    NULL                                   AS referrer,
    '{"device":"broken","region":"unknown"}' AS properties,
    now64(3)                               AS event_time
FROM numbers(5000);
