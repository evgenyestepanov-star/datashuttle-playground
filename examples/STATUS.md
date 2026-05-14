# Examples inventory

Status matrix for every directory under `examples/`, used as human narration alongside
`examples/manifest.json` (the consumed artefact). The manifest's per-source `status`
field drives Tier 1 / 2 promotion in the UI; this file documents the rationale and any
operator notes that don't belong in the JSON.

| Demo                      | Source              | Manifest status | Notes |
|---------------------------|---------------------|-----------------|-------|
| postgres-cdc              | PostgreSQL 16       | `stable`        | Base Tier-1 scenarios hang off this. Dremio-verified Iceberg V2 read path. |
| clickhouse-snapshot       | ClickHouse 24.8     | `stable`        | Includes K8s variant under `k8s/`. Powers `clickhouse-high-cardinality` and `clickhouse-time-travel` (both cloud-eligible). |
| realtime-demo (Kafka)     | Redpanda (Kafka)    | `stable`        | Arrow Flight hot buffer + Iceberg cold + DuckDB unified. Hosts `kafka-json-poison` and `kafka-throughput`. |
| mysql-cdc                 | MySQL 8.4           | `beta`          | E2E-verified after `mysql-binlog-restart` cloud-eligible cutover and `large-payload` (1 MB BLOB) audit. |
| mongodb-cdc               | MongoDB 7 (replset) | `beta`          | Change streams require replica-set init — see `init.js`. `mongodb-nested-evolution` runs on this. |
| file-ingestion            | S3 (MinIO)          | `beta`          | Parquet generator needs `pyarrow`. Hosts `file-s3-mixed-formats` and `file-bad-encoding`. Cloud-eligible after the persistent `FileTracker` re-snapshot dedup fix. |
| playground                | (sandbox harness)   | `supporting`    | Shared session machinery, not a scenario itself. |
| polaris-config            | Iceberg catalog     | `supporting`    | Shared infra — Polaris bootstrap (warehouse + admin role + default namespace). |
| full-demo                 | all of the above    | `composite`     | Orchestrates the others. Status derived from its parts. |

Tier 3 / Tier 4 sources advertised by the manifest (`cassandra`, `dynamodb-local`,
`localstack-kinesis`, `wiremock-rest`, `toxiproxy`, `redis`, `in-memory`,
`in-memory-generator`) ship with their compose stacks alongside the relevant
scenario rather than as standalone demo directories. They are introduced and
verified through the manifest itself; cross-reference there for status.

## Recent additions (post-Phase-5.C)

- **redis** source — Redis Streams entries → Iceberg
  (`redis-streams-events` scenario; status `beta` in manifest).
  Driven from the new `datashuttle-connector-redis` Tier-2 connector.
- **TTL session reaper + S3 purge in playground teardown** — applies to
  every scenario regardless of cloud-eligibility.

## Cloud-eligible scenarios

A growing subset of scenarios is approved for managed-Cloud sessions
(no privileged sidecar exec required). E2E-verified set as of 2026-05-14:

- `large-payload` (1 MB blob via MySQL)
- `slow-consumer`
- `rest-api-polling`
- `clickhouse-high-cardinality`
- `clickhouse-time-travel` (read-only flow)
- `mysql-binlog-restart`
- `redis-streams-events`

The remaining scenarios still execute `docker compose exec` against a
sibling container and run only on local self-hosted demos. Per-user
Cloud sandbox provisioning for the full set is a future phase.

## Kafka clarification

Kafka is in `examples/realtime-demo/` (Redpanda), not a standalone `examples/kafka/`.
When the manifest references `kafka-json-poison` or `kafka-throughput`, those scenarios
live under `realtime-demo/scenarios/`.

## Tiering logic

- **Tier 1** scenarios hang off `stable` sources only (postgres, clickhouse, kafka).
- **Tier 2** scenarios use `beta` sources — get promoted to Tier 1 once a manual
  smoke run plus E2E audit confirms them. Until then the UI shows a "Beta" badge
  derived from the manifest `status` field.
- **Tier 3** scenarios use new containers (cassandra, dynamodb-local, localstack-kinesis,
  wiremock-rest).
- **Tier 4** chaos scenarios layer toxiproxy on top of any Tier 1 source.

## Re-running this inventory

`examples/manifest.json` is the consumed artefact — UI reads `status`, CLI reads it too.
When a demo is re-verified or its source flips status, flip the corresponding `status`
field in the manifest **first**. This `STATUS.md` document is human narration and is
updated when the set of demos changes materially or new cloud-eligibility lands.

## Related documentation

- [`../docs/playground.md`](../docs/playground.md) — user-facing playground guide
  (scenario catalogue, web UI, CLI, security model).
- [`../README.md`](../README.md) — repo overview, env config, HTTP surface, releases.
