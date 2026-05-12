#!/usr/bin/env bash
# Full DataShuttle demo: start all infrastructure, create all shuttles.
# Usage: bash examples/full-demo/setup.sh

set -euo pipefail
cd "$(dirname "$0")/.."

DATASHUTTLE="${DATASHUTTLE:-../target/release/datashuttle}"

echo "╔══════════════════════════════════════════════╗"
echo "║       DataShuttle Full Demo Setup            ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

# 1. Infrastructure
# The demos below rely on mysql, mongodb, and clickhouse — all gated
# behind the `full` profile. Without `--profile full` only the default
# services (postgres, minio, polaris) come up and the mysql/mongodb/
# clickhouse shuttle creations below silently error out.
echo "▸ Starting infrastructure..."
docker compose -f docker-compose.yml --profile full up -d
echo "  Waiting for services to be healthy..."
sleep 10

# 2. Verify services
echo "▸ Checking services..."
docker compose -f docker-compose.yml ps --format "table {{.Name}}\t{{.Status}}" 2>/dev/null || \
  docker compose -f docker-compose.yml ps
echo ""

# 3. Generate file data
echo "▸ Generating file demo data..."
python3 file-ingestion/generate-data.py 2>/dev/null || echo "  (pyarrow not installed — CSV/JSON only)"

# 4. Upload files to MinIO
echo "▸ Uploading files to MinIO..."
mc alias set local http://localhost:9000 minioadmin minioadmin 2>/dev/null
mc cp file-ingestion/data/customers.csv local/file-ingestion/csv/ 2>/dev/null || true
mc cp file-ingestion/data/transactions.json local/file-ingestion/json/ 2>/dev/null || true
mc cp file-ingestion/data/events.parquet local/file-ingestion/parquet/ 2>/dev/null || true

# 5. Start DataShuttle
echo "▸ Starting DataShuttle server..."
$DATASHUTTLE start &
DS_PID=$!
sleep 3

# 6. Create all shuttles
echo ""
echo "▸ Creating shuttles..."
echo "  → PostgreSQL CDC (e-commerce)"
$DATASHUTTLE sql -f postgres-cdc/shuttle.sql 2>/dev/null || true
echo "  → MySQL CDC (IoT)"
$DATASHUTTLE sql -f mysql-cdc/shuttle.sql 2>/dev/null || true
echo "  → MongoDB CDC (social media)"
$DATASHUTTLE sql -f mongodb-cdc/shuttle.sql 2>/dev/null || true
echo "  → ClickHouse snapshot (web analytics)"
$DATASHUTTLE sql -f clickhouse-snapshot/shuttle.sql 2>/dev/null || true
echo "  → File ingestion (CSV, JSON, Parquet)"
$DATASHUTTLE sql -f file-ingestion/shuttle-csv.sql 2>/dev/null || true
$DATASHUTTLE sql -f file-ingestion/shuttle-json.sql 2>/dev/null || true
$DATASHUTTLE sql -f file-ingestion/shuttle-parquet.sql 2>/dev/null || true

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║  Setup complete!                             ║"
echo "║                                              ║"
echo "║  Web UI:    http://localhost:8080             ║"
echo "║  Metrics:   http://localhost:8080/metrics     ║"
echo "║  MinIO:     http://localhost:9001             ║"
echo "║                                              ║"
echo "║  Next: bash examples/full-demo/generate-load.sh ║"
echo "╚══════════════════════════════════════════════╝"
