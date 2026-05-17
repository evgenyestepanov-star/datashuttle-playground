#!/usr/bin/env bash
# Acceptance wrapper for examples/postgres-cdc.
#
# Runs the full PostgreSQL CDC → Iceberg pipeline end-to-end on a
# built OSS image and asserts via the scenario's existing verify.sh.

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/run_scenario.sh"
run_scenario postgres-cdc
