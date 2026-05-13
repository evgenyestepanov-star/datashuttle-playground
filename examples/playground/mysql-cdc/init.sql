-- DataShuttle MySQL CDC playground: IoT Telemetry Schema (cloud-safe).
-- The dispatcher injects `USE {namespace};` so unqualified table names
-- land in the session's private DB. Stored procedures and `DELIMITER`
-- blocks are not safe through the dispatcher's statement splitter
-- (splits on `;` outside quotes), so this version uses recursive CTE
-- INSERTs that ship as one statement each.

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

-- 50 devices via recursive CTE — MySQL 8.0+ has WITH RECURSIVE.
INSERT INTO devices (device_id, name, device_type, location, firmware_version)
WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n < 50)
SELECT
    CONCAT('DEV-', LPAD(n, 4, '0')),
    CONCAT(ELT(1 + (n % 5), 'TempSense', 'HumidiTrack', 'BaroMeter', 'AirQual', 'MotionEye'), '-', n),
    ELT(1 + (n % 5), 'temperature_sensor', 'humidity_sensor', 'pressure_sensor', 'air_quality', 'motion_detector'),
    ELT(1 + (n % 8), 'Building A Floor 1', 'Building A Floor 2', 'Building A Floor 3',
        'Building B Floor 1', 'Building B Floor 2', 'Warehouse North', 'Warehouse South', 'Outdoor'),
    CONCAT('2.', (n % 5), '.', (n % 10))
FROM seq;

-- 800 readings — mysql's default `cte_max_recursion_depth` caps at
-- 1000; staying under it avoids ER_CTE_MAX_RECURSION_DEPTH (3636).
-- The catchup action adds another batch live to demonstrate
-- post-pause replication.
INSERT INTO readings (device_id, metric_type, value, unit, quality, recorded_at)
WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n < 800)
SELECT
    CONCAT('DEV-', LPAD(1 + (n % 50), 4, '0')),
    ELT(1 + (n % 5), 'temperature', 'humidity', 'pressure', 'co2', 'motion'),
    CASE (n % 5)
        WHEN 0 THEN 15.0 + (RAND() * 25.0)
        WHEN 1 THEN 20.0 + (RAND() * 60.0)
        WHEN 2 THEN 990.0 + (RAND() * 40.0)
        WHEN 3 THEN 300.0 + (RAND() * 1200.0)
        ELSE ROUND(RAND())
    END,
    ELT(1 + (n % 5), '°C', '%', 'hPa', 'ppm', 'bool'),
    GREATEST(50, FLOOR(80 + RAND() * 20)),
    TIMESTAMPADD(SECOND, -n * 6, NOW())
FROM seq;

INSERT INTO alerts (device_id, severity, alert_type, message, is_acknowledged, triggered_at)
WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n < 200)
SELECT
    CONCAT('DEV-', LPAD(1 + (n % 50), 4, '0')),
    ELT(1 + (n % 3), 'info', 'warning', 'critical'),
    ELT(1 + (n % 5), 'high_temperature', 'low_battery', 'offline', 'threshold_exceeded', 'calibration_needed'),
    ELT(1 + (n % 5),
        CONCAT('Temperature exceeded threshold: ', ROUND(38 + RAND() * 5, 1), '°C'),
        CONCAT('Battery level critical: ', FLOOR(5 + RAND() * 10), '%'),
        'Device has not reported for 15 minutes',
        'Metric value exceeded configured threshold',
        'Sensor calibration overdue by 30 days'
    ),
    (n % 3 = 0),
    TIMESTAMPADD(HOUR, -n, NOW())
FROM seq;

-- 50 devices × 3 config cfg_keys = 150 rows via cross-join with a CTE.
INSERT INTO device_configs (device_id, config_key, config_value)
WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n < 50),
cfg_keys AS (
    SELECT 'report_interval' AS k UNION ALL
    SELECT 'threshold_high'  UNION ALL
    SELECT 'threshold_low'
)
SELECT
    CONCAT('DEV-', LPAD(seq.n, 4, '0')),
    cfg_keys.k,
    CASE cfg_keys.k
        WHEN 'report_interval' THEN CONCAT(FLOOR(10 + RAND() * 50), 's')
        WHEN 'threshold_high'  THEN CAST(ROUND(35 + RAND() * 10, 1) AS CHAR)
        ELSE CAST(ROUND(5 + RAND() * 10, 1) AS CHAR)
    END
FROM seq CROSS JOIN cfg_keys;
