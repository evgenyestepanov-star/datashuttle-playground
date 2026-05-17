# Playground acceptance suite

End-to-end acceptance tests that exercise the **built OSS image**
against the playground scenarios. Sprint 4 of the master test plan
(see `datashuttle/.planning/INTEGRATION_TESTS_PLAN.md`) — playground
plays the dual role of demo source-of-truth + acceptance harness.

## What this is

- **Not unit tests.** Those live in each crate's `src/` / `tests/`.
- **Not integration tests.** Those use `datashuttle-test-harness`
  via testcontainers, scoped per crate. They live in the source repos.
- **This is.** "Does the published product work for a user end-to-end,
  on real infrastructure, running the documented scenarios?"
  Spins up the playground compose stack, runs `demo.sh up` for a
  scenario, runs `verify.sh`, asserts exit code + parses outputs.

## Cadence

- **Manual** (`workflow_dispatch`) during development.
- **Nightly** against `ghcr.io/datashuttle-io/datashuttle:latest`.
- **On release**: every Tier-1 scenario must pass before publishing
  the GitHub Release notes.

## Adding a scenario

Each scenario `examples/<name>/` already has `demo.sh` (lifecycle:
`up`/`down`/`clean`) and `verify.sh` (assertions). The acceptance
wrapper for it is just one file:

```bash
acceptance/<name>.acceptance.sh
```

Skeleton:

```bash
#!/usr/bin/env bash
# Acceptance wrapper for examples/<name>.
set -euo pipefail
SCENARIO=<name>
source "$(dirname "${BASH_SOURCE[0]}")/run_scenario.sh"
run_scenario "$SCENARIO"
```

The shared `run_scenario.sh` handles teardown on failure, timeout,
artifact capture, and exit-code propagation.

## Coverage matrix (target)

| Scenario | Source | Status |
|---|---|---|
| `postgres-cdc` | PostgreSQL CDC → Iceberg | ✓ wired (Sprint 4) |
| `mysql-cdc` | MySQL binlog → Iceberg | TODO |
| `mongodb-cdc` | MongoDB change streams → Iceberg | TODO |
| `file-ingestion` | Filesystem (CSV/JSON/Parquet) → Iceberg | TODO |
| `clickhouse-snapshot` | ClickHouse snapshot → Iceberg | TODO |
| `realtime-demo` | UI + multi-source playground | TODO (visual / smoke) |
| `full-demo` | All Tier-1 sources together | TODO |

## Running locally

```bash
# Build the image (or pull latest)
docker pull ghcr.io/datashuttle-io/datashuttle:latest
export DATASHUTTLE_IMAGE=ghcr.io/datashuttle-io/datashuttle:latest

# Run one scenario
./acceptance/postgres-cdc.acceptance.sh

# Run all
./acceptance/run_all.sh
```

Artifacts (logs, captured parquet metadata) land in
`acceptance/artifacts/<scenario>/<timestamp>/`.
