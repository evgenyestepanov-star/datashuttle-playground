-- Generate IoT CDC changes: new readings, config updates, alerts.
-- Run: mysql -h localhost -u datashuttle -pdatashuttle iot < examples/mysql-cdc/generate-changes.sql

-- ── New readings (burst of 100) ───────────────────────

INSERT INTO readings (device_id, metric_type, value, unit, quality)
SELECT
    CONCAT('DEV-', LPAD(1 + (seq % 50), 4, '0')),
    ELT(1 + (seq % 3), 'temperature', 'humidity', 'pressure'),
    CASE (seq % 3)
        WHEN 0 THEN 20.0 + (RAND() * 15.0)
        WHEN 1 THEN 40.0 + (RAND() * 30.0)
        ELSE 1005.0 + (RAND() * 20.0)
    END,
    ELT(1 + (seq % 3), '°C', '%', 'hPa'),
    GREATEST(70, FLOOR(90 + RAND() * 10))
FROM (
    SELECT @row := @row + 1 AS seq
    FROM information_schema.columns, (SELECT @row := 0) r
    LIMIT 100
) AS nums;

-- ── Update device configs ─────────────────────────────

UPDATE device_configs SET config_value = '15s'
WHERE config_key = 'report_interval'
AND device_id IN ('DEV-0001', 'DEV-0002', 'DEV-0003');

UPDATE device_configs SET config_value = '42.0'
WHERE config_key = 'threshold_high'
AND device_id IN ('DEV-0010', 'DEV-0011');

-- ── New critical alerts ───────────────────────────────

INSERT INTO alerts (device_id, severity, alert_type, message) VALUES
('DEV-0005', 'critical', 'high_temperature', 'Temperature spike: 45.2°C — immediate attention required'),
('DEV-0012', 'critical', 'offline', 'Device unreachable for 30 minutes'),
('DEV-0033', 'warning', 'calibration_needed', 'Sensor drift detected — calibration recommended');

-- ── Acknowledge old alerts ────────────────────────────

UPDATE alerts SET is_acknowledged = true, acknowledged_at = NOW()
WHERE is_acknowledged = false AND severity = 'info'
LIMIT 30;

-- ── Devices go offline/online ─────────────────────────

UPDATE devices SET is_online = false WHERE device_id IN ('DEV-0012', 'DEV-0048');
UPDATE devices SET is_online = true, firmware_version = '2.5.1' WHERE device_id = 'DEV-0025';

SELECT 'MySQL CDC changes generated' AS result;
