-- DataShuttle shuttle for MySQL IoT demo.
-- Run: datashuttle sql -f examples/mysql-cdc/shuttle.sql

CREATE CONNECTION iot_mysql
  TYPE MYSQL
  PROPERTIES (
    host = 'localhost',
    port = '3306',
    database = 'iot',
    username = 'datashuttle',
    password = 'datashuttle'
  );

CREATE SHUTTLE iot_cdc
  SOURCE iot_mysql
  TABLES (devices, readings, alerts, device_configs)
  TARGET warehouse.iot
  WITH (
    mode = 'SNAPSHOT_THEN_CDC',
    commit_interval = '10 seconds',
    delete_mode = 'deletion_vectors',
    batch_size = '10000'
  );
