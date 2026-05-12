# Examples inventory (Phase 0 of playground rollout)

Status matrix for every directory under `examples/`, used by the playground manifest
(`examples/manifest.json`) to decide which scenarios are `stable` vs. `beta` vs. `hidden`.

| Demo                      | Source              | OSS? | Status    | Notes                                                                 |
|---------------------------|---------------------|------|-----------|-----------------------------------------------------------------------|
| postgres-cdc              | PostgreSQL 16       | yes  | stable    | Confirmed working — base Tier-1 scenarios hang off this.              |
| clickhouse-snapshot       | ClickHouse 24.8     | yes  | stable    | Confirmed working; includes K8s variant under `k8s/`.                 |
| realtime-demo (Kafka)     | Redpanda (Kafka)    | yes  | stable    | Arrow Flight hot buffer + Iceberg cold + DuckDB unified. Key demo.    |
| mysql-cdc                 | MySQL 8.4           | yes  | unverified | Scripts present (`shuttle.sql`, `generate-changes.sql`, `verify.sh`). Needs smoke run before being promoted to `stable` in the manifest. |
| mongodb-cdc               | MongoDB 7 (replset) | yes  | unverified | Change streams require replica-set init — see `init.js`. Manifest will start this at `beta`. |
| file-ingestion            | S3 (MinIO)          | yes  | unverified | Parquet generator needs `pyarrow`; manifest will start `beta`.        |
| full-demo                 | all of the above    | yes  | composite | Orchestrates the others. Status derived from its parts.               |
| polaris-config            | Iceberg catalog     | yes  | supporting | Not a scenario — shared infra for all demos.                         |
<!-- #813 — `salesforce-connector/` deleted 2026-04-21. The directory
     shipped nothing but a 123-line README describing a
     `connector-salesforce` cargo feature that never existed, and a
     `SALESFORCE` connection type that isn't registered anywhere. It
     advertised capability we don't have. If Salesforce ingestion
     lands on the roadmap, a future epic can seed a real example. -->


## Kafka clarification

Kafka is in `examples/realtime-demo/` (Redpanda), not a standalone `examples/kafka/`.
When the manifest references `kafka-json-poison` or `kafka-throughput`, those scenarios
live under `realtime-demo/scenarios/` — added in Phase 5.

## Tiering logic

- **Tier 1** scenarios hang off `stable` sources only (postgres, clickhouse, kafka).
- **Tier 2** scenarios use `unverified` sources — get promoted to Tier 1 once a manual
  smoke run confirms them. Until then the UI shows a "Beta" badge from `status`.
- **Tier 3** scenarios use new containers added in Phase 3
  (cassandra, dynamodb-local, localstack, wiremock).
- **Tier 4** chaos scenarios layer toxiproxy on top of any Tier 1 source.

## Re-running this inventory

`examples/manifest.json` is the consumed artifact — UI reads `status`, CLI reads it too.
When a demo is re-verified, flip the `status` field in the manifest. This `STATUS.md`
document is human narration and is updated when the set of demos changes materially.
