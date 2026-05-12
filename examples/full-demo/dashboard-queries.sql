-- Dashboard queries for verifying demo data in Iceberg via DuckDB.
-- Usage: duckdb -c ".read examples/full-demo/dashboard-queries.sql"
--
-- Requires DuckDB with Iceberg extension:
--   INSTALL iceberg; LOAD iceberg;

-- ── PostgreSQL E-Commerce ─────────────────────────────

-- SELECT count(*) AS customer_count FROM iceberg_scan('s3://warehouse/ecommerce/customers');
-- SELECT count(*) AS order_count FROM iceberg_scan('s3://warehouse/ecommerce/orders');
-- SELECT status, count(*) FROM iceberg_scan('s3://warehouse/ecommerce/orders') GROUP BY status;

-- ── MySQL IoT ─────────────────────────────────────────

-- SELECT count(*) AS reading_count FROM iceberg_scan('s3://warehouse/iot/readings');
-- SELECT metric_type, avg(value) FROM iceberg_scan('s3://warehouse/iot/readings') GROUP BY metric_type;

-- ── MongoDB Social Media ──────────────────────────────

-- SELECT count(*) AS post_count FROM iceberg_scan('s3://warehouse/social/posts');

-- ── ClickHouse Web Analytics ───────────────────────────

-- SELECT count(*) AS event_count FROM iceberg_scan('s3://warehouse/analytics/events');
-- SELECT event_type, count(*) FROM iceberg_scan('s3://warehouse/analytics/events') GROUP BY event_type;
-- SELECT count(*) AS session_count FROM iceberg_scan('s3://warehouse/analytics/sessions');
-- SELECT device_type, count(*) FROM iceberg_scan('s3://warehouse/analytics/sessions') GROUP BY device_type;

-- ── File Ingestion ────────────────────────────────────

-- SELECT count(*) FROM iceberg_scan('s3://warehouse/analytics/customers');
-- SELECT count(*) FROM iceberg_scan('s3://warehouse/analytics/transactions');

-- Note: These queries assume the Iceberg catalog is configured in DuckDB.
-- Actual paths depend on the Polaris catalog namespace layout.

SELECT 'Dashboard queries ready — uncomment and run with DuckDB' AS info;
