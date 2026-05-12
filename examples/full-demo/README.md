# Full Demo

Run all DataShuttle demos simultaneously: PostgreSQL CDC, MySQL CDC, MongoDB CDC, and file ingestion.

## What you get

| Source | Schema | Tables | Rows |
|--------|--------|--------|------|
| PostgreSQL | E-commerce | customers, products, orders, order_items, payments | ~9,600 |
| MySQL | IoT telemetry | devices, readings, alerts, device_configs | ~10,400 |
| MongoDB | Social media | users, posts, comments | ~4,200 |
| ClickHouse | Web analytics | users, sessions, events, page_views | ~20,500 |
| S3 files | Analytics | CSV events, JSON logs, Parquet transactions | ~3,500 |

**Total: ~48,200 rows across 20 tables from 5 source types.**

## Quick start (5 minutes)

```bash
# 1. Build DataShuttle
cargo build --release

# 2. Run the full setup
bash examples/full-demo/setup.sh

# 3. Open the dashboard
open http://localhost:8080

# 4. Generate live changes
bash examples/full-demo/generate-load.sh

# 5. Teardown when done
bash examples/full-demo/teardown.sh
```

## Architecture

```
PostgreSQL (CDC) ──┐
MySQL (CDC) ───────┤
MongoDB (CDC) ─────┤──► DataShuttle ──► Iceberg V3 (MinIO + Polaris)
ClickHouse (snap) ─┤        │
S3 files ──────────┘        └──► Web UI (:8080) + Metrics (:8080/metrics)
```

## Individual demos

Each source has its own directory with init scripts, shuttle SQL, change generators, and verification:

- [PostgreSQL CDC](../postgres-cdc/README.md)
- [MySQL CDC](../mysql-cdc/README.md)
- [MongoDB CDC](../mongodb-cdc/README.md)
- [ClickHouse Snapshot](../clickhouse-snapshot/README.md)
- [File Ingestion](../file-ingestion/README.md)
