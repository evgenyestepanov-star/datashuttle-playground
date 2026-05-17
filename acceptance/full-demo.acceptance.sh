#!/usr/bin/env bash
# Acceptance wrapper for examples/full-demo.
# Spins up the entire compose stack (Polaris + MinIO + Postgres +
# MySQL + Mongo + all sidecars). Heaviest scenario — give the
# shuttles longer to drain before verify runs.
set -euo pipefail
export SCENARIO_INGEST_WAIT=45
source "$(dirname "${BASH_SOURCE[0]}")/run_scenario.sh"
run_scenario full-demo
