#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# DataShuttle Realtime Demo — one-script orchestrator
#
# Usage:
#   ./demo.sh up        # start everything from scratch
#   ./demo.sh down      # stop everything, keep data
#   ./demo.sh clean     # stop everything + destroy data & volumes
#   ./demo.sh status    # show what's running
#   ./demo.sh restart   # down + up
#   ./demo.sh logs      # tail DataShuttle logs
#   ./demo.sh open      # open dashboard in browser
# ──────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"
BINARY="$PROJECT_ROOT/target/release/datashuttle"
REGISTRY_DB="${HOME}/.datashuttle/registry.db"
PIDDIR="$SCRIPT_DIR/.pids"
LOGDIR="$SCRIPT_DIR/.logs"

# ── Env defaults (override via env vars) ──────────────────────────────
DS_API_PORT="${DS_API_PORT:-8080}"
DS_FLIGHT_PORT="${DS_FLIGHT_PORT:-8815}"
DASHBOARD_PORT="${DASHBOARD_PORT:-3000}"
PRODUCER_RATE="${PRODUCER_RATE:-50}"

export DS_SERVER_API_PORT="$DS_API_PORT"
export DS_METRICS_PORT="${DS_METRICS_PORT:-9090}"
export DS_CATALOG_TYPE=rest
export DS_CATALOG_URI="http://localhost:8181/api/catalog"
export DS_CATALOG_NAME=warehouse
export DS_WAREHOUSE="s3://warehouse/"
export DS_S3_ENDPOINT="http://localhost:9000"
export DS_S3_ACCESS_KEY=minioadmin
export DS_S3_SECRET_KEY=minioadmin
export DS_S3_REGION=us-east-1
export DS_CATALOG_CLIENT_ID=root
export DS_CATALOG_CLIENT_SECRET=s3cr3t

# ── Colors ────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

log()  { echo -e "${GREEN}▸${NC} $*"; }
warn() { echo -e "${YELLOW}▸${NC} $*"; }
err()  { echo -e "${RED}✗${NC} $*" >&2; }
hdr()  { echo -e "\n${BOLD}${BLUE}═══ $* ═══${NC}"; }

# ── Helpers ───────────────────────────────────────────────────────────
mkdir -p "$PIDDIR" "$LOGDIR"

save_pid() { echo "$2" > "$PIDDIR/$1.pid"; }

read_pid() {
    local f="$PIDDIR/$1.pid"
    [[ -f "$f" ]] && cat "$f" || echo ""
}

is_alive() {
    local pid
    pid=$(read_pid "$1")
    [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

stop_process() {
    local name="$1"
    local pid
    pid=$(read_pid "$name")
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        # Wait up to 5s for graceful shutdown
        for _ in $(seq 1 50); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.1
        done
        # Force kill if still alive
        kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null || true
        log "stopped $name (pid $pid)"
    fi
    rm -f "$PIDDIR/$name.pid"
}

wait_for_port() {
    local port="$1" label="$2" timeout="${3:-30}"
    for i in $(seq 1 "$timeout"); do
        if nc -z localhost "$port" 2>/dev/null; then
            log "$label ready on port $port (${i}s)"
            return 0
        fi
        sleep 1
    done
    err "$label failed to start on port $port after ${timeout}s"
    return 1
}

wait_for_healthy() {
    local service="$1" timeout="${2:-60}"
    for i in $(seq 1 "$timeout"); do
        local status
        status=$(docker compose -f "$COMPOSE_FILE" ps "$service" --format '{{.Status}}' 2>/dev/null || echo "")
        if [[ "$status" == *"healthy"* ]]; then
            log "$service healthy (${i}s)"
            return 0
        fi
        sleep 1
    done
    err "$service not healthy after ${timeout}s"
    return 1
}

# ── Commands ──────────────────────────────────────────────────────────

cmd_up() {
    hdr "Starting Realtime Demo"

    # 1. Check binary
    if [[ ! -x "$BINARY" ]]; then
        err "Binary not found at $BINARY"
        err "Run: cargo build --release -p datashuttle-cli"
        exit 1
    fi

    # Warn if any source file is newer than the binary.
    local stale
    stale=$(find "$PROJECT_ROOT/crates" -name "*.rs" -newer "$BINARY" 2>/dev/null | head -1)
    if [[ -n "$stale" ]]; then
        warn "Binary may be stale (sources changed since last build)"
        warn "Rebuilding: cargo build --release -p datashuttle-cli"
        ( cd "$PROJECT_ROOT" && \
          PATH="$HOME/.cargo/bin:$PATH" \
          cargo build --release -p datashuttle-cli 2>&1 | tail -2 )
        log "rebuild done"
    fi

    # 2. Check Python deps
    if ! python3 -c "import pyarrow, duckdb, requests" 2>/dev/null; then
        warn "Installing Python dependencies..."
        pip3 install --quiet pyarrow duckdb requests
    fi

    # 3. Start infrastructure
    hdr "Infrastructure (Redpanda + MinIO + Polaris)"
    docker compose -f "$COMPOSE_FILE" up -d

    wait_for_healthy redpanda 30
    wait_for_healthy minio 15
    wait_for_healthy polaris 30

    # Wait for init containers to finish
    log "waiting for init containers..."
    sleep 5
    # Verify Polaris catalog exists
    for i in $(seq 1 10); do
        if curl -sf http://localhost:8181/api/catalog/v1/config >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    log "init containers done"

    # 4. Start DataShuttle
    hdr "DataShuttle"
    if is_alive datashuttle; then
        warn "DataShuttle already running"
    else
        pushd "$PROJECT_ROOT" >/dev/null
        nohup "$BINARY" start > "$LOGDIR/datashuttle.log" 2>&1 &
        save_pid datashuttle $!
        popd >/dev/null
        wait_for_port "$DS_API_PORT" "DataShuttle API" 15
        wait_for_port "$DS_FLIGHT_PORT" "Arrow Flight" 5
    fi

    # 5. Create shuttle
    hdr "Shuttle"
    local api="http://localhost:${DS_API_PORT}/api/v1/sql"
    local ct='Content-Type: application/json'

    # Idempotent: create connection + shuttle, ignore "already exists"
    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "CREATE CONNECTION kafka_rt TYPE KAFKA WITH (bootstrap_servers = '\''http://localhost:18082'\'', topic = '\''clickstream'\'', group_id = '\''ds-demo'\'')"}' \
        >/dev/null 2>&1 || true
    log "connection kafka_rt ensured"

    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "CREATE SHUTTLE clickstream_rt SOURCE kafka_rt TABLE clickstream TARGET warehouse.realtime WITH (schedule = '\''continuous'\'', realtime = '\''true'\'', commit_interval = '\''5 seconds'\'', batch_size = '\''1000'\'', hot_buffer_max_rows = '\''10000'\'')"}' \
        >/dev/null 2>&1 || true
    log "shuttle clickstream_rt ensured"

    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "RESUME SHUTTLE clickstream_rt"}' \
        >/dev/null 2>&1 || true
    log "shuttle started"

    # 6. Start producer
    hdr "Producer (${PRODUCER_RATE} events/sec)"
    if is_alive producer; then
        warn "producer already running"
    else
        nohup python3 "$SCRIPT_DIR/producer.py" "$PRODUCER_RATE" \
            > "$LOGDIR/producer.log" 2>&1 &
        save_pid producer $!
        log "producer started (pid $!)"
    fi

    # 7. Start dashboard
    hdr "Dashboard"
    if is_alive dashboard; then
        warn "dashboard already running"
    else
        nohup python3 "$SCRIPT_DIR/dashboard-server.py" \
            > "$LOGDIR/dashboard.log" 2>&1 &
        save_pid dashboard $!
        wait_for_port "$DASHBOARD_PORT" "dashboard" 10
    fi

    # 8. Summary
    hdr "Demo Running"
    echo ""
    echo -e "  ${BOLD}Dashboard${NC}        http://localhost:${DASHBOARD_PORT}"
    echo -e "  ${BOLD}DataShuttle UI${NC}   http://localhost:${DS_API_PORT}"
    echo -e "  ${BOLD}Arrow Flight${NC}     grpc://localhost:${DS_FLIGHT_PORT}"
    echo -e "  ${BOLD}MinIO Console${NC}    http://localhost:9001  (minioadmin/minioadmin)"
    echo -e "  ${BOLD}Redpanda${NC}         localhost:19092"
    echo ""
    echo -e "  Stop:   ${BOLD}./demo.sh down${NC}"
    echo -e "  Clean:  ${BOLD}./demo.sh clean${NC}"
    echo -e "  Status: ${BOLD}./demo.sh status${NC}"
    echo -e "  Logs:   ${BOLD}./demo.sh logs${NC}"
    echo ""
}

cmd_down() {
    hdr "Stopping Demo"

    stop_process dashboard
    stop_process producer
    stop_process datashuttle

    # Kill any stragglers on known ports
    for port in "$DS_API_PORT" "$DS_FLIGHT_PORT" "$DASHBOARD_PORT"; do
        local pid
        pid=$(lsof -ti :"$port" 2>/dev/null || true)
        [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    done

    docker compose -f "$COMPOSE_FILE" stop 2>/dev/null || true
    log "all stopped"
}

cmd_clean() {
    hdr "Cleaning Demo (destroying all data)"

    cmd_down

    docker compose -f "$COMPOSE_FILE" down -v 2>/dev/null || true
    rm -f "$REGISTRY_DB"
    rm -rf "$PIDDIR" "$LOGDIR"
    log "volumes, registry, and logs removed"
}

cmd_status() {
    hdr "Demo Status"

    echo -e "\n${BOLD}Processes:${NC}"
    for svc in datashuttle producer dashboard; do
        if is_alive "$svc"; then
            local pid
            pid=$(read_pid "$svc")
            echo -e "  ${GREEN}●${NC} $svc (pid $pid)"
        else
            echo -e "  ${RED}●${NC} $svc (stopped)"
        fi
    done

    echo -e "\n${BOLD}Docker:${NC}"
    docker compose -f "$COMPOSE_FILE" ps --format "  {{.Name}}\t{{.Status}}" 2>/dev/null || echo "  (not running)"

    echo -e "\n${BOLD}Shuttle:${NC}"
    local state
    state=$(curl -sf "http://localhost:${DS_API_PORT}/api/v1/shuttles/clickstream_rt" 2>/dev/null \
        | python3 -c "import sys,json;print(json.load(sys.stdin)['state'])" 2>/dev/null || echo "unreachable")
    echo "  clickstream_rt: $state"

    echo -e "\n${BOLD}Hot Buffer:${NC}"
    python3 -c "
import pyarrow.flight as flight
try:
    client = flight.FlightClient('grpc://localhost:${DS_FLIGHT_PORT}')
    t = client.do_get(flight.Ticket(b'clickstream')).read_all()
    print(f'  {t.num_rows} rows')
except: print('  (unavailable)')
" 2>/dev/null

    echo -e "\n${BOLD}Dashboard:${NC}"
    local stats
    stats=$(curl -sf "http://localhost:${DASHBOARD_PORT}/api/stats" 2>/dev/null || echo "")
    if [[ -n "$stats" ]]; then
        echo "$stats" | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(f'  Hot: {d[\"hot_rows\"]} | Cold: {d[\"cold_rows\"]} | Unified: {d[\"unified_rows\"]}')
" 2>/dev/null
    else
        echo "  (unavailable)"
    fi
    echo ""
}

cmd_logs() {
    hdr "DataShuttle Logs"
    if [[ -f "$LOGDIR/datashuttle.log" ]]; then
        tail -f "$LOGDIR/datashuttle.log"
    else
        err "no logs found (is DataShuttle running?)"
    fi
}

cmd_open() {
    if command -v open &>/dev/null; then
        open "http://localhost:${DASHBOARD_PORT}"
    elif command -v xdg-open &>/dev/null; then
        xdg-open "http://localhost:${DASHBOARD_PORT}"
    else
        log "Open http://localhost:${DASHBOARD_PORT} in your browser"
    fi
}

cmd_restart() {
    cmd_down
    sleep 2
    cmd_up
}

# ── Main ──────────────────────────────────────────────────────────────
case "${1:-help}" in
    up)      cmd_up ;;
    down)    cmd_down ;;
    clean)   cmd_clean ;;
    status)  cmd_status ;;
    restart) cmd_restart ;;
    logs)    cmd_logs ;;
    open)    cmd_open ;;
    *)
        echo "Usage: $0 {up|down|clean|status|restart|logs|open}"
        echo ""
        echo "  up       Start everything from scratch"
        echo "  down     Stop all processes (keep data)"
        echo "  clean    Stop + destroy all data & volumes"
        echo "  status   Show what's running"
        echo "  restart  Stop + start"
        echo "  logs     Tail DataShuttle logs"
        echo "  open     Open dashboard in browser"
        exit 1
        ;;
esac
