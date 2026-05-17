#!/usr/bin/env bash
# Acceptance wrapper for examples/mongodb-cdc.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/run_scenario.sh"
run_scenario mongodb-cdc
