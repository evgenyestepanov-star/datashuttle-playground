#!/usr/bin/env bash
# Verify the ClickHouse K8s demo — check all 3 DataShuttle nodes, shuttle, and source data.
# Usage: bash examples/clickhouse-snapshot/k8s/verify.sh

set -euo pipefail

NAMESPACE="datashuttle-demo"

echo "=== ClickHouse K8s Demo Verification ==="
echo ""

# 1. Cluster nodes
echo "1. DataShuttle cluster nodes:"
kubectl -n "$NAMESPACE" get pods -l app.kubernetes.io/name=datashuttle \
  -o custom-columns="NAME:.metadata.name,STATUS:.status.phase,NODE:.spec.nodeName,IP:.status.podIP" 2>/dev/null || \
  echo "   (no DataShuttle pods found)"
echo ""

# 2. Shuttle status
echo "2. Shuttle status:"
kubectl -n "$NAMESPACE" exec datashuttle-0 -- \
  datashuttle shuttle status analytics_snapshot 2>/dev/null || \
  echo "   (shuttle not created yet)"
echo ""

# 3. Source row counts
echo "3. ClickHouse source row counts:"
kubectl -n "$NAMESPACE" exec clickhouse-0 -- clickhouse-client --database analytics -q "
  SELECT 'users:       ' || toString(count()) FROM analytics.users
  UNION ALL SELECT 'sessions:    ' || toString(count()) FROM analytics.sessions
  UNION ALL SELECT 'events:      ' || toString(count()) FROM analytics.events
  UNION ALL SELECT 'page_views:  ' || toString(count()) FROM analytics.page_views
" 2>/dev/null || echo "   (ClickHouse not reachable)"
echo ""

# 4. Check each node health
echo "4. Node health:"
for i in 0 1 2; do
  STATUS=$(kubectl -n "$NAMESPACE" exec "datashuttle-$i" -- \
    curl -sf http://localhost:8080/health 2>/dev/null || echo '{"status":"unreachable"}')
  echo "   datashuttle-$i: $STATUS"
done
echo ""

# 5. Metrics from leader
echo "5. Shuttle metrics (from datashuttle-0):"
kubectl -n "$NAMESPACE" exec datashuttle-0 -- \
  curl -sf http://localhost:9090/metrics 2>/dev/null | \
  grep "datashuttle_shuttle_rows_total" | head -5 || \
  echo "   (no metrics yet)"
echo ""

echo "=== Verification complete ==="
