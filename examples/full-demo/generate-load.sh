#!/usr/bin/env bash
# Generate load on all demo sources simultaneously.
# Usage: bash examples/full-demo/generate-load.sh

set -euo pipefail
cd "$(dirname "$0")/.."

echo "▸ Generating CDC changes on all sources..."
echo ""

echo "  PostgreSQL (e-commerce)..."
PGPASSWORD=postgres psql -h localhost -U postgres -d ecommerce \
  -f postgres-cdc/generate-changes.sql 2>/dev/null &

echo "  MySQL (IoT)..."
mysql -h localhost -u datashuttle -pdatashuttle iot \
  < mysql-cdc/generate-changes.sql 2>/dev/null &

echo "  MongoDB (social media)..."
mongosh --host localhost --quiet examples/../mongodb-cdc/generate-changes.js 2>/dev/null &

echo "  ClickHouse (web analytics)..."
curl -s "http://localhost:8123/?database=analytics" \
  --data-binary @clickhouse-snapshot/generate-changes.sql 2>/dev/null &

wait
echo ""
echo "▸ All changes applied. Check the Web UI at http://localhost:8080"
