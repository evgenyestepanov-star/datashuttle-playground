-- DataShuttle MySQL CDC Demo: IoT Telemetry Schema
-- Initializes: devices, readings, alerts, device_configs

-- ── Schema ────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS devices (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    device_id       VARCHAR(50)  NOT NULL UNIQUE,
    name            VARCHAR(100) NOT NULL,
    device_type     VARCHAR(30)  NOT NULL,
    location        VARCHAR(100),
    firmware_version VARCHAR(20) DEFAULT '1.0.0',
    is_online       BOOLEAN DEFAULT true,
    registered_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_seen_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS readings (
    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
    device_id       VARCHAR(50)  NOT NULL,
    metric_type     VARCHAR(30)  NOT NULL,
    value           DOUBLE       NOT NULL,
    unit            VARCHAR(10)  NOT NULL,
    quality         TINYINT      DEFAULT 100,
    recorded_at     TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_device_time (device_id, recorded_at),
    INDEX idx_metric (metric_type)
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS alerts (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    device_id       VARCHAR(50)  NOT NULL,
    severity        ENUM('info','warning','critical') NOT NULL,
    alert_type      VARCHAR(50)  NOT NULL,
    message         TEXT,
    is_acknowledged BOOLEAN DEFAULT false,
    triggered_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    acknowledged_at TIMESTAMP NULL
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS device_configs (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    device_id       VARCHAR(50)  NOT NULL,
    config_key      VARCHAR(50)  NOT NULL,
    config_value    VARCHAR(200) NOT NULL,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uk_device_key (device_id, config_key)
) ENGINE=InnoDB;

-- ── Grant replication privileges ──────────────────────

GRANT SELECT, REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'datashuttle'@'%';
FLUSH PRIVILEGES;

-- ── Seed data: Devices (50) ───────────────────────────

DELIMITER //
CREATE PROCEDURE seed_data()
BEGIN
    DECLARE i INT DEFAULT 1;
    DECLARE dev_types VARCHAR(200) DEFAULT 'temperature_sensor,humidity_sensor,pressure_sensor,air_quality,motion_detector';

    -- Devices
    WHILE i <= 50 DO
        INSERT INTO devices (device_id, name, device_type, location, firmware_version) VALUES (
            CONCAT('DEV-', LPAD(i, 4, '0')),
            CONCAT(ELT(1 + (i % 5), 'TempSense', 'HumidiTrack', 'BaroMeter', 'AirQual', 'MotionEye'), '-', i),
            ELT(1 + (i % 5), 'temperature_sensor', 'humidity_sensor', 'pressure_sensor', 'air_quality', 'motion_detector'),
            ELT(1 + (i % 8), 'Building A Floor 1', 'Building A Floor 2', 'Building A Floor 3',
                'Building B Floor 1', 'Building B Floor 2', 'Warehouse North', 'Warehouse South', 'Outdoor'),
            CONCAT('2.', (i % 5), '.', (i % 10))
        );
        SET i = i + 1;
    END WHILE;

    -- Readings (10,000)
    SET i = 1;
    WHILE i <= 10000 DO
        INSERT INTO readings (device_id, metric_type, value, unit, quality, recorded_at) VALUES (
            CONCAT('DEV-', LPAD(1 + (i % 50), 4, '0')),
            ELT(1 + (i % 5), 'temperature', 'humidity', 'pressure', 'co2', 'motion'),
            CASE (i % 5)
                WHEN 0 THEN 15.0 + (RAND() * 25.0)          -- temperature: 15-40°C
                WHEN 1 THEN 20.0 + (RAND() * 60.0)           -- humidity: 20-80%
                WHEN 2 THEN 990.0 + (RAND() * 40.0)          -- pressure: 990-1030 hPa
                WHEN 3 THEN 300.0 + (RAND() * 1200.0)        -- co2: 300-1500 ppm
                ELSE ROUND(RAND())                            -- motion: 0 or 1
            END,
            ELT(1 + (i % 5), '°C', '%', 'hPa', 'ppm', 'bool'),
            GREATEST(50, FLOOR(80 + RAND() * 20)),
            TIMESTAMPADD(SECOND, -i * 6, NOW())
        );
        SET i = i + 1;
    END WHILE;

    -- Alerts (200)
    SET i = 1;
    WHILE i <= 200 DO
        INSERT INTO alerts (device_id, severity, alert_type, message, is_acknowledged, triggered_at) VALUES (
            CONCAT('DEV-', LPAD(1 + (i % 50), 4, '0')),
            ELT(1 + (i % 3), 'info', 'warning', 'critical'),
            ELT(1 + (i % 5), 'high_temperature', 'low_battery', 'offline', 'threshold_exceeded', 'calibration_needed'),
            ELT(1 + (i % 5),
                CONCAT('Temperature exceeded threshold: ', ROUND(38 + RAND() * 5, 1), '°C'),
                CONCAT('Battery level critical: ', FLOOR(5 + RAND() * 10), '%'),
                'Device has not reported for 15 minutes',
                'Metric value exceeded configured threshold',
                'Sensor calibration overdue by 30 days'
            ),
            (i % 3 = 0),
            TIMESTAMPADD(HOUR, -i, NOW())
        );
        SET i = i + 1;
    END WHILE;

    -- Device configs
    SET i = 1;
    WHILE i <= 50 DO
        INSERT INTO device_configs (device_id, config_key, config_value) VALUES
            (CONCAT('DEV-', LPAD(i, 4, '0')), 'report_interval', CONCAT(FLOOR(10 + RAND() * 50), 's')),
            (CONCAT('DEV-', LPAD(i, 4, '0')), 'threshold_high', CONCAT(ROUND(35 + RAND() * 10, 1))),
            (CONCAT('DEV-', LPAD(i, 4, '0')), 'threshold_low', CONCAT(ROUND(5 + RAND() * 10, 1)));
        SET i = i + 1;
    END WHILE;

END //
DELIMITER ;

CALL seed_data();
DROP PROCEDURE seed_data;

-- ── Summary ───────────────────────────────────────────

SELECT 'DataShuttle MySQL demo loaded' AS info;
SELECT 'devices' AS tbl, COUNT(*) AS cnt FROM devices
UNION ALL SELECT 'readings', COUNT(*) FROM readings
UNION ALL SELECT 'alerts', COUNT(*) FROM alerts
UNION ALL SELECT 'device_configs', COUNT(*) FROM device_configs;
