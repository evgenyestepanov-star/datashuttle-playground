# DataShuttle Examples

Ready-to-run demos for every supported DataShuttle source type.

## Quick start

```bash
# Build DataShuttle
cargo build --release

# Run everything
bash examples/full-demo/setup.sh

# Open http://localhost:8080
```

## Demos

| Demo | Source | Data | Description |
|------|--------|------|-------------|
| [postgres-cdc](postgres-cdc/) | PostgreSQL 16 | E-commerce (9.6K rows) | Logical replication CDC |
| [mysql-cdc](mysql-cdc/) | MySQL 8.4 | IoT telemetry (10.4K rows) | Binlog CDC |
| [mongodb-cdc](mongodb-cdc/) | MongoDB 7 | Social media (4.2K docs) | Change streams |
| [clickhouse-snapshot](clickhouse-snapshot/) | ClickHouse 24.8 | Web analytics (20.5K rows) | Snapshot + watermark ([K8s variant](clickhouse-snapshot/k8s/)) |
| [file-ingestion](file-ingestion/) | S3 (MinIO) | Analytics (3.5K rows) | CSV, JSON, Parquet |
| [full-demo](full-demo/) | All of the above | 48.2K total | End-to-end |

## Prerequisites

- Docker + Docker Compose
- Rust 1.82+ (to build DataShuttle)
- Python 3 + pyarrow (for Parquet file generation — optional)
- MinIO client `mc` (for file upload — optional)

## Infrastructure

The `docker-compose.yml` in this directory starts:

| Service | Port | Purpose |
|---------|------|---------|
| PostgreSQL 16 | 5432 | Source DB (wal_level=logical) |
| MySQL 8.4 | 3306 | Source DB (binlog=ROW, GTID) |
| MongoDB 7 | 27017 | Source DB (replica set) |
| ClickHouse 24.8 | 8123/9009 | Source DB (MergeTree, HTTP+native) |
| MinIO | 9000/9001 | S3-compatible storage |
| Apache Polaris | 8181 | Iceberg REST catalog |

## Profiles

Most sources are gated behind docker-compose profiles so `docker compose up -d`
only provisions what you actually need. Default (no profile) starts PostgreSQL,
MinIO, and Polaris — enough for `postgres-cdc` and `file-ingestion` demos plus
their playground equivalents.

| Profile | Services | Enables |
|---------|----------|---------|
| _(default)_ | postgres, minio, polaris | `postgres-cdc-*`, `file-s3-*`, `file-bad-encoding` |
| `full` | mysql, mongodb (+init/seed), clickhouse | `mysql-*`, `mongodb-*`, `clickhouse-*`, `large-payload`, full-demo |
| `clickhouse` | clickhouse | `clickhouse-high-cardinality`, `clickhouse-time-travel` (subset of `full`) |
| `playground` | redpanda, cassandra, dynamodb-local, localstack, wiremock | `kafka-*`, `rest-api-polling`, `dynamodb-streams`, `kinesis-shards`, `cassandra-wide-row`, `slow-consumer` |
| `chaos` | toxiproxy | `network-chaos` |
| `dremio` | nessie, dremio | Dremio/Nessie target explorations (not wired to a playground scenario) |
| `greenplum` | greenplum (+init) | Greenplum source explorations (not wired to a playground scenario) |

Combine profiles with repeated `--profile` flags. To light up every playground
scenario source the UI exposes:

```bash
docker compose -f examples/docker-compose.yml \
  --profile full --profile playground --profile chaos \
  up -d
```

Set env vars so DataShuttle can talk to Polaris + MinIO:

```bash
export DS_CATALOG_CLIENT_ID=root
export DS_CATALOG_CLIENT_SECRET=s3cr3t
export DS_AWS_ACCESS_KEY_ID=minioadmin
export DS_AWS_SECRET_ACCESS_KEY=minioadmin
```
