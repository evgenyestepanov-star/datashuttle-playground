-- DataShuttle shuttle definitions for PostgreSQL CDC demo.
-- Run after DataShuttle is started:
--   datashuttle sql -f examples/postgres-cdc/shuttle.sql
--
-- When running from host (DataShuttle outside Docker), use 'localhost'.
-- When DataShuttle runs inside Docker Compose, use 'postgres' as the host.
-- The REST API equivalent:
--   curl -X POST http://localhost:8080/api/v1/shuttles \
--     -H 'Content-Type: application/json' \
--     -d '{"sql": "CREATE CONNECTION ecommerce_pg TYPE POSTGRES PROPERTIES (host = '\''postgres'\'', port = '\''5432'\'', database = '\''ecommerce'\'', username = '\''postgres'\'', password = '\''postgres'\'', replication_slot = '\''datashuttle_demo'\'', publication = '\''datashuttle_pub'\'')"}'

CREATE CONNECTION ecommerce_pg
  TYPE POSTGRES
  PROPERTIES (
    host = 'localhost',
    port = '5432',
    database = 'ecommerce',
    username = 'postgres',
    password = 'postgres',
    replication_slot = 'datashuttle_demo',
    publication = 'datashuttle_pub'
  );

CREATE SHUTTLE ecommerce_cdc
  SOURCE ecommerce_pg
  TABLES (customers, products, orders, order_items, payments)
  TARGET warehouse.ecommerce
  WITH (
    mode = 'SNAPSHOT_THEN_CDC',
    commit_interval = '10 seconds',
    delete_mode = 'deletion_vectors',
    schema_evolution = 'compatible',
    parallelism = '4',
    batch_size = '5000'
  );
