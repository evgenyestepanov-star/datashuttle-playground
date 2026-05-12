-- Seed 100K events across 3 shards via Distributed table
-- Data is automatically distributed by rand() sharding key

INSERT INTO events (id, session_id, user_id, event_type, event_name, page_url, referrer, properties, region, device_type, event_time)
SELECT
    number + 1 AS id,
    (number % 20000) + 1 AS session_id,
    (number % 5000) + 1 AS user_id,
    arrayElement(['page_view','click','search','purchase','signup','add_to_cart','remove_from_cart','checkout'],
                 (number % 8) + 1) AS event_type,
    concat(arrayElement(['page_view','click','search','purchase','signup','add_to_cart','remove_from_cart','checkout'],
                        (number % 8) + 1), '_', toString((number % 100) + 1)) AS event_name,
    concat('https://example.com/', arrayElement(['home','products','pricing','about','blog','docs','signup','checkout','search','profile'],
                                                (number % 10) + 1)) AS page_url,
    if(number % 4 = 0, arrayElement(['https://google.com','https://facebook.com','https://twitter.com',''], (number % 4) + 1), NULL) AS referrer,
    concat('{"button":"btn_', toString(number % 50), '","value":', toString((number * 13) % 1000), '}') AS properties,
    arrayElement(['us-east','us-west','eu-west','eu-central','ap-south','ap-east'], (number % 6) + 1) AS region,
    arrayElement(['desktop','mobile','tablet'], (number % 3) + 1) AS device_type,
    now64(3) - toIntervalSecond(number * 3) AS event_time
FROM numbers(100000);

-- Seed 20K sessions
INSERT INTO sessions (id, user_id, session_start, duration_sec, page_count, device_type, country, is_bounce)
SELECT
    number + 1 AS id,
    (number % 5000) + 1 AS user_id,
    now64(3) - toIntervalSecond(number * 30) AS session_start,
    60 + (number * 7) % 1800 AS duration_sec,
    1 + (number % 20) AS page_count,
    arrayElement(['desktop','mobile','tablet'], (number % 3) + 1) AS device_type,
    arrayElement(['US','US','US','GB','DE','FR','CA','AU','JP','BR'], (number % 10) + 1) AS country,
    (number % 8) = 0 AS is_bounce
FROM numbers(20000);

SELECT '── Data seeded: 100K events + 20K sessions across 3 shards ──' AS info;
