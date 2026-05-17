#!/usr/bin/env bash
# Shared scenario runner used by acceptance/<scenario>.acceptance.sh.
#
# Responsibilities:
#  1. Pin a working directory + artifact path
#  2. Ensure teardown on any exit (success, failure, signal)
#  3. Run `demo.sh up` and verify it returns clean
#  4. Wait for the API to become ready (HTTP probe on :8080)
#  5. Run `verify.sh` and capture stdout/stderr to the artifact path
#  6. Surface a clear pass/fail line for CI log parsing

set -euo pipefail

# Caller passes scenario as first positional or sets SCENARIO env.
run_scenario() {
    local scenario="${1:-${SCENARIO:-}}"
    if [ -z "$scenario" ]; then
        echo "ERROR: scenario name required (positional arg or SCENARIO env)" >&2
        exit 2
    fi

    local repo_root
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    local scenario_dir="$repo_root/examples/$scenario"
    if [ ! -d "$scenario_dir" ]; then
        echo "ERROR: scenario not found at $scenario_dir" >&2
        exit 2
    fi

    local ts
    ts="$(date -u +%Y%m%dT%H%M%SZ)"
    local artifact_dir="$repo_root/acceptance/artifacts/$scenario/$ts"
    mkdir -p "$artifact_dir"

    echo "[acceptance] scenario=$scenario artifacts=$artifact_dir"

    # ── Teardown on any exit ────────────────────────────────────────
    cleanup() {
        local rc=$?
        echo "[acceptance] cleanup (rc=$rc) — running demo.sh clean"
        ( cd "$scenario_dir" && ./demo.sh clean ) >> "$artifact_dir/teardown.log" 2>&1 || true
        if [ $rc -eq 0 ]; then
            echo "[acceptance] PASS scenario=$scenario"
        else
            echo "[acceptance] FAIL scenario=$scenario rc=$rc"
            echo "[acceptance] last 50 lines of demo log:"
            tail -50 "$artifact_dir/up.log" 2>/dev/null || true
            echo "[acceptance] last 50 lines of verify log:"
            tail -50 "$artifact_dir/verify.log" 2>/dev/null || true
        fi
        exit $rc
    }
    trap cleanup EXIT INT TERM

    # ── 1. Bring stack up ───────────────────────────────────────────
    echo "[acceptance] step=up"
    ( cd "$scenario_dir" && ./demo.sh up ) > "$artifact_dir/up.log" 2>&1

    # ── 2. Wait for API readiness (60s budget) ──────────────────────
    echo "[acceptance] step=wait-ready"
    local deadline=$(( $(date +%s) + 60 ))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if curl -sf -o /dev/null "http://localhost:8080/health" 2>/dev/null; then
            echo "[acceptance] api ready"
            break
        fi
        sleep 1
    done
    if ! curl -sf -o /dev/null "http://localhost:8080/health" 2>/dev/null; then
        echo "[acceptance] ERROR: api did not become ready within 60s" | tee -a "$artifact_dir/verify.log"
        exit 1
    fi

    # ── 3. Give the shuttle a moment to ingest before verifying ─────
    # Most scenarios stage data, then the shuttle picks it up on the
    # next poll. 15s is conservative; scenarios that need longer can
    # set SCENARIO_INGEST_WAIT.
    sleep "${SCENARIO_INGEST_WAIT:-15}"

    # ── 4. Run verify.sh ────────────────────────────────────────────
    echo "[acceptance] step=verify"
    ( cd "$scenario_dir" && ./verify.sh ) > "$artifact_dir/verify.log" 2>&1

    # ── 5. Done. Cleanup in trap. ──────────────────────────────────
    echo "[acceptance] verify completed cleanly"
    return 0
}
