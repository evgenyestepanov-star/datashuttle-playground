-- Playground action: after the binlog recovery scenario pauses MySQL
-- for 30 seconds, this inserts a checkpoint row whose appearance in
-- Iceberg confirms the shuttle caught up from the retained binlog.

INSERT INTO readings (device_id, reading_type, value, reading_time)
VALUES (1, 'temperature', 42.0, NOW());

SELECT id, reading_type, value, reading_time
FROM readings
ORDER BY id DESC
LIMIT 5;
