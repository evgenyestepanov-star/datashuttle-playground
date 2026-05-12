#!/usr/bin/env bash
# Verify ClickHouse snapshot demo data made it to Iceberg.
# Uses DataShuttle CLI to check shuttle status.

set -euo pipefail

DATASHUTTLE="${DATASHUTTLE:-datashuttle}"
SERVER="${SERVER:-http://localhost:8080}"
CH_HOST="${CH_HOST:-localhost}"
CH_PORT="${CH_PORT:-8123}"

echo "=== ClickHouse Snapshot Demo Verification ==="
echo ""

# 1. Check shuttle status
echo "1. Shuttle status:"
$DATASHUTTLE shuttle status analytics_snapshot --server "$SERVER" -o json 2>/dev/null || \
  curl -s "$SERVER/api/v1/shuttles/analytics_snapshot/status" | python3 -m json.tool 2>/dev/null || \
  echo "   (shuttle not running — start DataShuttle first)"

echo ""

# 2. Check shuttle exists
echo "2. Shuttles registered:"
curl -s "$SERVER/api/v1/shuttles" | python3 -m json.tool 2>/dev/null || \
  echo "   (API not reachable at $SERVER)"

echo ""

# 3. Check source row counts
echo "3. Source database row counts:"
curl -s "http://${CH_HOST}:${CH_PORT}/?database=analytics" \
  --data "SELECT 'users:       ' || toString(count()) FROM analytics.users
UNION ALL SELECT 'sessions:    ' || toString(count()) FROM analytics.sessions
UNION ALL SELECT 'events:      ' || toString(count()) FROM analytics.events
UNION ALL SELECT 'page_views:  ' || toString(count()) FROM analytics.page_views" 2>/dev/null || \
  echo "   (ClickHouse not reachable at ${CH_HOST}:${CH_PORT})"

echo ""

# 4. Check metrics
echo "4. Metrics:"
curl -s "$SERVER/metrics" 2>/dev/null | grep "datashuttle_shuttle_rows_total" | head -5 || \
  echo "   (no metrics yet — shuttle may still be snapshotting)"

echo ""
echo "=== Verification complete ==="
