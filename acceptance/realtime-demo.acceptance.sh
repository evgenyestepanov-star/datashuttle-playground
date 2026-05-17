#!/usr/bin/env bash
# Acceptance wrapper for examples/realtime-demo.
# This scenario is multi-source + UI-heavy; budget extra ingest time
# so verify.sh sees ingestion through the slower paths (DynamoDB,
# Kinesis emulators take ~20s to first poll).
set -euo pipefail
export SCENARIO_INGEST_WAIT=30
source "$(dirname "${BASH_SOURCE[0]}")/run_scenario.sh"
run_scenario realtime-demo
