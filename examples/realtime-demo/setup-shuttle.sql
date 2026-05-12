-- DataShuttle Realtime Demo — Shuttle Setup
--
-- Run via DataShuttle SQL console:
--   curl -X POST http://localhost:8080/api/v1/sql -H 'Content-Type: application/json' -d @setup-shuttle.sql
--
-- Or paste into the Web UI SQL Console at http://localhost:8080/sql

-- 1. Create Kafka connection (uses Panda Proxy REST API)
CREATE CONNECTION kafka_demo
  TYPE KAFKA
  OPTIONS (
    bootstrap_servers = 'redpanda:9092',
    group_id = 'datashuttle-demo'
  );

-- 2. Create realtime shuttle: Kafka clickstream → Iceberg + Flight buffer
CREATE SHUTTLE clickstream_rt
  SOURCE kafka_demo
  TABLE clickstream
  TARGET warehouse.realtime
  WITH (
    schedule = 'continuous',
    realtime = 'true',
    commit_interval = '5 seconds',
    batch_size = '1000',
    hot_buffer_max_rows = '10000'
  );

-- 3. Create realtime shuttle: Kafka transactions → Iceberg + Flight buffer
CREATE SHUTTLE transactions_rt
  SOURCE kafka_demo
  TABLE transactions
  TARGET warehouse.realtime
  WITH (
    schedule = 'continuous',
    realtime = 'true',
    commit_interval = '5 seconds',
    batch_size = '500',
    hot_buffer_max_rows = '10000'
  );
