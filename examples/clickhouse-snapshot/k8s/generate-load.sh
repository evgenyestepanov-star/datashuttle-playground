#!/usr/bin/env bash
# Generate live data in ClickHouse while the K8s shuttle is running.
# Usage: bash examples/clickhouse-snapshot/k8s/generate-load.sh

set -euo pipefail

NAMESPACE="datashuttle-demo"

echo "▸ Generating ClickHouse changes (K8s)..."
kubectl -n "$NAMESPACE" exec -i clickhouse-0 -- clickhouse-client --database analytics \
  < examples/clickhouse-snapshot/generate-changes.sql

echo ""
echo "▸ Changes applied. Check the shuttle status:"
echo "  kubectl -n $NAMESPACE exec datashuttle-0 -- datashuttle shuttle status analytics_snapshot"
