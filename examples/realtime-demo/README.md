# DataShuttle Realtime Demo

**Kafka → DataShuttle → Arrow Flight (hot) + Iceberg (cold) → DuckDB unified queries**

## Architecture

```
  Kafka Producer ──→ Redpanda ──→ DataShuttle Shuttle (realtime=true)
                                        │
                         ┌──────────────┼──────────────┐
                         ▼                             ▼
                  Arrow Flight Buffer           Iceberg / MinIO
                   (hot, <1ms reads)          (cold, Polaris catalog)
                         │                             │
                         └──────────┬──────────────────┘
                                    ▼
                             DuckDB (unified)
                          UNION ALL + dedup by offset
                                    │
                                    ▼
                        Fraud Detection Dashboard
                          http://localhost:3000
```

## Quick Start

```bash
# One command — starts Docker, DataShuttle, shuttle, producer, dashboard:
cd examples/realtime-demo
./demo.sh up

# Other commands:
./demo.sh status    # show what's running
./demo.sh down      # stop everything (keep data)
./demo.sh clean     # stop + destroy all data & volumes
./demo.sh restart   # down + up
./demo.sh logs      # tail DataShuttle logs
./demo.sh open      # open dashboard in browser
```

### Prerequisites

- Docker (for Redpanda, MinIO, Polaris)
- Rust release binary: `cargo build --release -p datashuttle-cli`
- Python 3 with `pyarrow`, `duckdb`, `requests` (auto-installed by `demo.sh`)

### Manual Setup

<details>
<summary>If you prefer step-by-step</summary>

```bash
# 1. Start infrastructure (Redpanda + MinIO + Polaris)
docker compose -f examples/realtime-demo/docker-compose.yml up -d

# 2. Start DataShuttle (release build)
DS_SERVER_API_PORT=8080 DS_METRICS_PORT=9090 \
DS_CATALOG_TYPE=rest DS_CATALOG_URI="http://localhost:8181/api/catalog" \
DS_CATALOG_NAME=warehouse DS_WAREHOUSE="s3://warehouse/" \
DS_S3_ENDPOINT="http://localhost:9000" DS_S3_ACCESS_KEY=minioadmin \
DS_S3_SECRET_KEY=minioadmin DS_S3_REGION=us-east-1 \
DS_CATALOG_CLIENT_ID=root DS_CATALOG_CLIENT_SECRET=s3cr3t \
./target/release/datashuttle start

# 3. Create Kafka connection + realtime shuttle
curl -X POST http://localhost:8080/api/v1/sql -H 'Content-Type: application/json' \
  -d '{"sql": "CREATE CONNECTION kafka_rt TYPE KAFKA WITH (bootstrap_servers = '\''http://localhost:18082'\'', topic = '\''clickstream'\'', group_id = '\''ds-demo'\'')"}'

curl -X POST http://localhost:8080/api/v1/sql -H 'Content-Type: application/json' \
  -d '{"sql": "CREATE SHUTTLE clickstream_rt SOURCE kafka_rt TABLE clickstream TARGET warehouse.realtime WITH (schedule = '\''continuous'\'', realtime = '\''true'\'', commit_interval = '\''5 seconds'\'', batch_size = '\''1000'\'', hot_buffer_max_rows = '\''10000'\'')"}'

curl -X POST http://localhost:8080/api/v1/sql -H 'Content-Type: application/json' \
  -d '{"sql": "RESUME SHUTTLE clickstream_rt"}'

# 4. Start event producer
pip install requests
python3 examples/realtime-demo/producer.py 50   # 50 events/sec

# 5. Launch dashboard
pip install pyarrow duckdb
python3 examples/realtime-demo/dashboard-server.py
open http://localhost:3000
```

</details>

## Components

| Component | Port | Description |
|-----------|------|-------------|
| DataShuttle API + UI | 8080 | Shuttle management, monitoring |
| Arrow Flight gRPC | 8815 | Hot buffer reads (<1ms) |
| Redpanda (Kafka) | 19092 | Event streaming |
| Panda Proxy REST | 18082 | Kafka REST API for DataShuttle |
| MinIO (S3) | 9000/9001 | Iceberg data storage |
| Polaris | 8181 | Iceberg REST catalog |
| Dashboard | 3000 | Fraud detection analytics |

## Dashboard Features

| Panel | Source | Description |
|-------|--------|-------------|
| Hot Buffer | Arrow Flight | Live in-memory buffer, <1ms |
| Cold Store | Iceberg/S3 | Historical Parquet on MinIO, Polaris-managed |
| Unified | DuckDB | UNION ALL + dedup by offset — no duplicates |
| Regions | Unified | Geographic distribution (hot+cold) |
| Actions | Hot only | Real-time user action breakdown |
| Fraud Alerts | Hot | Users with >15 events or >4 regions |
| Live Events | Hot | Latest events from Flight buffer |
| Top Users | Hot | Most active users (potential bots) |
| Devices | Unified | Desktop/mobile/tablet split |

## How Unified Queries Work

DuckDB queries both Arrow Flight (hot buffer) and Iceberg (cold, S3 Parquet files) in a single SQL query:

```sql
-- Hot: Arrow Flight → pyarrow → DuckDB
SELECT * FROM hot

UNION ALL

-- Cold: Iceberg data files on S3 → DuckDB
SELECT * FROM cold
WHERE "offset" NOT IN (SELECT "offset" FROM hot)
```

No duplicates. Hot buffer has the latest data (<1ms). Cold has the full history.
DuckDB runs in-process in the dashboard server — no separate database needed.
