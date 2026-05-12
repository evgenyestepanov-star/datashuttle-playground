#!/usr/bin/env bash
# Tear down the ClickHouse K8s demo — uninstall Helm release and delete kind cluster.
# Usage: bash examples/clickhouse-snapshot/k8s/teardown.sh

set -euo pipefail

NAMESPACE="datashuttle-demo"

echo "▸ Uninstalling DataShuttle Helm release..."
helm uninstall datashuttle --namespace "$NAMESPACE" 2>/dev/null || true

echo "▸ Deleting namespace..."
kubectl delete namespace "$NAMESPACE" --ignore-not-found 2>/dev/null || true

echo "▸ Deleting kind cluster..."
kind delete cluster --name ds-clickhouse 2>/dev/null || true

echo "✓ Teardown complete."
