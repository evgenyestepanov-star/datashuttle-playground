#!/usr/bin/env bash
# Stop all demo infrastructure.
# Usage: bash examples/full-demo/teardown.sh

set -euo pipefail
cd "$(dirname "$0")/.."

echo "▸ Stopping DataShuttle..."
pkill -f "datashuttle start" 2>/dev/null || true

echo "▸ Stopping Docker services..."
docker compose -f docker-compose.yml down -v

echo "▸ Cleaning generated files..."
rm -f file-ingestion/data/customers.csv
rm -f file-ingestion/data/transactions.json
rm -f file-ingestion/data/events.parquet

echo "✓ Teardown complete."
