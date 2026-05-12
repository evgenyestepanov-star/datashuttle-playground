# ClickHouse Demo — Kubernetes (3-node cluster)

Deploys a full DataShuttle 3-node cluster on Kubernetes with ClickHouse as the source, Polaris as the Iceberg catalog, and MinIO as S3 storage.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Kubernetes (kind)                         │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │datashuttle-0│  │datashuttle-1│  │datashuttle-2│        │
│  │  (leader)   │◄─►│  (follower) │◄─►│  (follower) │        │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘        │
│         │    gossip protocol (7946)        │                │
│         └────────────┬────────────────────┘                │
│                      ▼                                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                 │
│  │ClickHouse│  │  Polaris  │  │   MinIO   │                 │
│  │  :8123   │  │   :8181   │  │ :9000/9001│                 │
│  └──────────┘  └──────────┘  └──────────┘                 │
└─────────────────────────────────────────────────────────────┘
```

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [kind](https://kind.sigs.k8s.io/docs/user/quick-start/#installation) (Kubernetes in Docker)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- [Helm](https://helm.sh/docs/intro/install/) 3.x

## Quick start

```bash
# One command to set up everything:
bash examples/clickhouse-snapshot/k8s/setup.sh

# Watch shuttles:
kubectl -n datashuttle-demo port-forward svc/datashuttle 8080:8080 &
open http://localhost:8080

# Generate live changes:
bash examples/clickhouse-snapshot/k8s/generate-load.sh

# Tear down:
bash examples/clickhouse-snapshot/k8s/teardown.sh
```

## Step-by-step

### 1. Create the kind cluster

```bash
kind create cluster --name ds-clickhouse --config examples/clickhouse-snapshot/k8s/kind-config.yaml
```

### 2. Deploy infrastructure (MinIO, Polaris, ClickHouse)

```bash
kubectl apply -f examples/clickhouse-snapshot/k8s/namespace.yaml
kubectl apply -f examples/clickhouse-snapshot/k8s/infra.yaml
kubectl -n datashuttle-demo wait --for=condition=ready pod -l app=clickhouse --timeout=120s
kubectl -n datashuttle-demo wait --for=condition=ready pod -l app=polaris --timeout=120s
kubectl -n datashuttle-demo wait --for=condition=ready pod -l app=minio --timeout=120s
```

### 3. Initialize ClickHouse data

```bash
kubectl -n datashuttle-demo exec -i clickhouse-0 -- clickhouse-client \
  < examples/clickhouse-snapshot/init.sql
```

### 4. Initialize Polaris catalog

```bash
kubectl apply -f examples/clickhouse-snapshot/k8s/polaris-init-job.yaml
kubectl -n datashuttle-demo wait --for=condition=complete job/polaris-init --timeout=60s
```

### 5. Deploy DataShuttle (3 nodes)

```bash
helm install datashuttle deploy/helm/datashuttle \
  --namespace datashuttle-demo \
  --values examples/clickhouse-snapshot/k8s/values-clickhouse-demo.yaml
kubectl -n datashuttle-demo rollout status statefulset/datashuttle --timeout=120s
```

### 6. Create the shuttle

```bash
kubectl -n datashuttle-demo exec datashuttle-0 -- \
  datashuttle sql -e "
    CREATE CONNECTION analytics_ch TYPE CLICKHOUSE PROPERTIES (
      host = 'clickhouse',
      port = '8123',
      database = 'analytics',
      username = 'default',
      password = ''
    );
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
  "
```

### 7. Verify

```bash
bash examples/clickhouse-snapshot/k8s/verify.sh
```

## Cluster behavior

With 3 nodes, DataShuttle distributes shuttle work via gossip protocol:

- **Leader election**: one node owns the shuttle scheduler
- **Table sharding**: snapshot chunks are distributed across nodes
- **Failover**: if a node goes down, remaining nodes pick up its work

Test failover:

```bash
# Kill one node
kubectl -n datashuttle-demo delete pod datashuttle-1

# Watch the remaining nodes redistribute
kubectl -n datashuttle-demo logs datashuttle-0 -f --tail=20
```

## Port forwards (for local access)

```bash
kubectl -n datashuttle-demo port-forward svc/datashuttle 8080:8080 &   # DataShuttle UI
kubectl -n datashuttle-demo port-forward svc/minio 9001:9001 &         # MinIO console
kubectl -n datashuttle-demo port-forward svc/clickhouse 8123:8123 &    # ClickHouse HTTP
```
