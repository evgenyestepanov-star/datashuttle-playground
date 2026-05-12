#!/usr/bin/env bash
set -euo pipefail
echo "=== File Ingestion Demo Verification ==="
echo ""
echo "Files in MinIO:"
mc alias set local http://localhost:9000 minioadmin minioadmin 2>/dev/null
mc ls local/file-ingestion/ --recursive 2>/dev/null || echo "  (MinIO not reachable or mc not installed)"
echo ""
echo "Shuttle statuses:"
for p in csv_events json_logs parquet_transactions; do
  echo "  $p:"
  curl -s "http://localhost:8080/api/v1/shuttles/$p/status" 2>/dev/null | python3 -m json.tool 2>/dev/null || echo "    (not found)"
done
echo ""
echo "=== Done ==="
