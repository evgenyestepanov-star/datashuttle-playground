# ClickHouse Parallel Read Demo

Demonstrates DataShuttle's MPP parallel read capability with a configurable 1–3 shard ClickHouse cluster.

DataShuttle discovers cluster shards via `system.clusters` and reads each shard
directly in parallel, bypassing the ClickHouse coordinator for snapshot reads.

## Architecture

```
                    ┌─────────────────┐
                    │   DataShuttle   │
                    │  Shuttle Mgr   │
                    └────┬───┬───┬────┘
                         │   │   │
              ┌──────────┘   │   └──────────┐
              ▼              ▼              ▼
        ┌───────────┐ ┌───────────┐ ┌───────────┐
        │  Shard 1  │ │  Shard 2  │ │  Shard 3  │
        │ ~33K rows │ │ ~33K rows │ │ ~33K rows │
        └───────────┘ └───────────┘ └───────────┘
              │              │              │
              └──────────────┼──────────────┘
                             ▼
                     Iceberg / MinIO
                    (merged Parquet)
```

## Data

| Table | Total Rows | Per Shard | Engine |
|-------|-----------|-----------|--------|
| `events` | 100,000 | ~33K | Distributed → MergeTree |
| `sessions` | 20,000 | ~7K | Distributed → MergeTree |

## Quick Start

```bash
# From the repo root:
cd examples/clickhouse-snapshot

# 3-shard cluster (default) — full parallel demo
./demo.sh up

# 1-shard — minimal resources (good for a laptop)
./demo.sh up 1

# 2-shard
./demo.sh up 2

# Check status
./demo.sh status

# Inject more data and trigger shuttle
./demo.sh inject 50000
./demo.sh resume

# Stop (keep data)
./demo.sh down

# Stop + destroy everything
./demo.sh clean
```

The `up` command:
1. Generates a shard-specific `docker-compose.override.yml` and `cluster-config.xml`
2. Starts N ClickHouse shards + N DataShuttle nodes + MinIO + Polaris
3. Seeds 100K events distributed across all shards
4. Creates and starts the parallel snapshot shuttles automatically

## What to observe

1. **Shard discovery** — DataShuttle logs:
   ```
   ClickHouse cluster shards discovered cluster=analytics_cluster shards=3
   ```

2. **Parallel readers** — each shard read independently:
   ```
   opening ClickHouse shard reader shard=1 host=clickhouse-1 port=8123
   opening ClickHouse shard reader shard=2 host=clickhouse-2 port=8123
   opening ClickHouse shard reader shard=3 host=clickhouse-3 port=8123
   ```

3. **Throughput** — 3x faster than sequential read through coordinator

## Connection options

| Option | Description |
|--------|-------------|
| `cluster` | ClickHouse cluster name (enables parallel reads) |
| `host` | Coordinator host (for discovery + fallback) |
| `port` | HTTP port (default: 8123) |
| `database` | Database name |

When `cluster` is set, DataShuttle:
1. Queries `system.clusters` to discover shards
2. Opens direct HTTP connections to each shard
3. Runs PK-chunked snapshot on each shard in parallel
4. Merges results and writes to Iceberg
