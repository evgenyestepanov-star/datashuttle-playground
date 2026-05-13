-- Playground action: after the binlog-restart scenario pauses + resumes
-- the shuttle, this writes a sentinel row whose appearance in Iceberg
-- confirms the shuttle caught up from the retained binlog. Column
-- names match the playground init.sql's `readings` table — the demo
-- bundle's catchup.sql targeted the OSS schema (device_id INT,
-- reading_type, reading_time) which doesn't exist in the playground
-- variant.

INSERT INTO readings (device_id, metric_type, value, unit, quality, recorded_at)
VALUES ('DEV-9999', 'temperature', 42.0, '°C', 100, NOW());

SELECT id, device_id, metric_type, value, recorded_at
FROM readings
ORDER BY id DESC
LIMIT 3;
