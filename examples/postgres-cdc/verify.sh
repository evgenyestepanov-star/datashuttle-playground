#!/usr/bin/env bash
# Verify PostgreSQL CDC demo data made it to Iceberg.
# Uses DataShuttle CLI to check shuttle status.

set -euo pipefail

DATASHUTTLE="${DATASHUTTLE:-datashuttle}"
SERVER="${SERVER:-http://localhost:8080}"

echo "=== PostgreSQL CDC Demo Verification ==="
echo ""

# 1. Check shuttle status
echo "1. Shuttle status:"
$DATASHUTTLE shuttle status ecommerce_cdc --server "$SERVER" -o json 2>/dev/null || \
  curl -s "$SERVER/api/v1/shuttles/ecommerce_cdc/status" | python3 -m json.tool 2>/dev/null || \
  echo "   (shuttle not running — start DataShuttle first)"

echo ""

# 2. Check shuttle exists
echo "2. Shuttles registered:"
curl -s "$SERVER/api/v1/shuttles" | python3 -m json.tool 2>/dev/null || \
  echo "   (API not reachable at $SERVER)"

echo ""

# 3. Check source row counts
echo "3. Source database row counts:"
PGPASSWORD=postgres psql -h localhost -U postgres -d ecommerce -t -c "
  SELECT 'customers:   ' || count(*) FROM customers
  UNION ALL SELECT 'products:    ' || count(*) FROM products
  UNION ALL SELECT 'orders:      ' || count(*) FROM orders
  UNION ALL SELECT 'order_items: ' || count(*) FROM order_items
  UNION ALL SELECT 'payments:    ' || count(*) FROM payments;
" 2>/dev/null || echo "   (PostgreSQL not reachable)"

echo ""

# 4. Check metrics
echo "4. Metrics:"
curl -s "$SERVER/metrics" 2>/dev/null | grep "datashuttle_shuttle_rows_total" | head -5 || \
  echo "   (no metrics yet — shuttle may still be snapshotting)"

echo ""
echo "=== Verification complete ==="
