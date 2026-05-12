#!/usr/bin/env bash
# Deploy the full ClickHouse demo on a local kind cluster with 3 DataShuttle nodes.
# Usage: bash examples/clickhouse-snapshot/k8s/setup.sh

set -euo pipefail
cd "$(dirname "$0")/../../.."

K8S_DIR="examples/clickhouse-snapshot/k8s"
NAMESPACE="datashuttle-demo"

echo "╔══════════════════════════════════════════════════╗"
echo "║   DataShuttle ClickHouse K8s Demo (3 nodes)     ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# ── 1. kind cluster ────────────────────────────────────

if kind get clusters 2>/dev/null | grep -q "ds-clickhouse"; then
  echo "▸ kind cluster 'ds-clickhouse' already exists, reusing..."
else
  echo "▸ Creating kind cluster (1 control-plane + 3 workers)..."
  kind create cluster --config "$K8S_DIR/kind-config.yaml"
fi
echo ""

# ── 2. Namespace ───────────────────────────────────────

echo "▸ Creating namespace..."
kubectl apply -f "$K8S_DIR/namespace.yaml"
echo ""

# ── 3. Infrastructure ─────────────────────────────────

echo "▸ Deploying infrastructure (MinIO, Polaris, ClickHouse)..."
kubectl apply -f "$K8S_DIR/infra.yaml"

echo "  Waiting for MinIO..."
kubectl -n "$NAMESPACE" wait --for=condition=available deployment/minio --timeout=120s

echo "  Waiting for Polaris..."
kubectl -n "$NAMESPACE" wait --for=condition=available deployment/polaris --timeout=120s

echo "  Waiting for ClickHouse..."
kubectl -n "$NAMESPACE" wait --for=condition=ready pod -l app=clickhouse --timeout=120s
echo ""

# ── 4. Init ClickHouse data ───────────────────────────

echo "▸ Loading ClickHouse demo data (~20.5K rows)..."
kubectl -n "$NAMESPACE" exec -i clickhouse-0 -- clickhouse-client \
  < examples/clickhouse-snapshot/init.sql
echo ""

# ── 5. Init Polaris catalog ───────────────────────────

echo "▸ Initializing Polaris catalog..."
kubectl apply -f "$K8S_DIR/polaris-init-job.yaml"
kubectl -n "$NAMESPACE" wait --for=condition=complete job/polaris-init --timeout=120s
echo ""

# ── 6. Deploy DataShuttle (3 nodes) ───────────────────

echo "▸ Deploying DataShuttle cluster (3 nodes)..."
helm upgrade --install datashuttle deploy/helm/datashuttle \
  --namespace "$NAMESPACE" \
  --values "$K8S_DIR/values-clickhouse-demo.yaml" \
  --wait --timeout 180s
echo ""

# ── 7. Verify cluster ─────────────────────────────────

echo "▸ DataShuttle pods:"
kubectl -n "$NAMESPACE" get pods -l app.kubernetes.io/name=datashuttle -o wide
echo ""

# ── 8. Create shuttle ────────────────────────────────

echo "▸ Creating ClickHouse shuttle..."
kubectl -n "$NAMESPACE" exec datashuttle-0 -- \
  datashuttle sql -e "
    CREATE CONNECTION analytics_ch TYPE CLICKHOUSE PROPERTIES (
      host = 'clickhouse',
      port = '8123',
      database = 'analytics',
      username = 'default',
      password = ''
    );
  " 2>/dev/null || true

kubectl -n "$NAMESPACE" exec datashuttle-0 -- \
  datashuttle sql -e "
    CREATE SHUTTLE analytics_snapshot
      SOURCE analytics_ch
      TABLES (users, sessions, events, page_views)
      TARGET warehouse.analytics
      WITH (
        mode = 'SNAPSHOT',
        commit_interval = '30 seconds',
        parallelism = '4',
        batch_size = '10000',
        watermark_column = 'created_at'
      );
  " 2>/dev/null || true
echo ""

# ── 9. Summary ─────────────────────────────────────────

echo "╔══════════════════════════════════════════════════╗"
echo "║  Setup complete!                                 ║"
echo "║                                                  ║"
echo "║  Access (port-forward):                          ║"
echo "║    kubectl -n $NAMESPACE \\                ║"
echo "║      port-forward svc/datashuttle 8080:8080      ║"
echo "║                                                  ║"
echo "║  Web UI:    http://localhost:8080                 ║"
echo "║  Metrics:   http://localhost:8080/metrics         ║"
echo "║                                                  ║"
echo "║  Next steps:                                     ║"
echo "║    bash examples/clickhouse-snapshot/k8s/generate-load.sh ║"
echo "║    bash examples/clickhouse-snapshot/k8s/verify.sh        ║"
echo "╚══════════════════════════════════════════════════╝"
