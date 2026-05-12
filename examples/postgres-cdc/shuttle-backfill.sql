-- Playground variant of the postgres-cdc shuttle that opts into a
-- full table snapshot before switching to logical decoding. The UI
-- runs this when the user picks the "backfill + live" scenario.

CREATE CONNECTION IF NOT EXISTS ecommerce_pg_backfill
  TYPE POSTGRES
  PROPERTIES (
    host = 'localhost',
    port = '5432',
    database = 'ecommerce',
    username = 'postgres',
    password = 'postgres',
    replication_slot = 'datashuttle_playground_backfill',
    publication = 'datashuttle_pub'
  );

CREATE SHUTTLE IF NOT EXISTS ecommerce_backfill
  SOURCE ecommerce_pg_backfill
  TABLE orders, customers, products
  TARGET warehouse.playground
  WITH (
    schedule = 'continuous',
    start_mode = 'snapshot_then_cdc',
    commit_interval = '5 seconds',
    batch_size = '1000'
  );

RESUME SHUTTLE ecommerce_backfill;
