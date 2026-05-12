#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# DataShuttle PostgreSQL CDC Demo
#
# Usage:
#   ./demo.sh up                 # start everything from scratch
#   ./demo.sh down               # stop everything, keep data
#   ./demo.sh clean              # stop everything + destroy data & volumes
#   ./demo.sh status             # show what's running
#   ./demo.sh restart            # down + up
#   ./demo.sh inject             # insert 1000 rows (default)
#   ./demo.sh inject 5000        # insert 5000 customers + proportional rows
#   ./demo.sh inject orders 500  # insert 500 orders using existing customers
#   ./demo.sh logs               # tail DataShuttle logs
#   ./demo.sh open               # open UI in browser
# ──────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$PROJECT_ROOT/examples/docker-compose.yml"
BINARY="$PROJECT_ROOT/target/release/datashuttle"
REGISTRY_DB="${HOME}/.datashuttle/registry.db"
PIDDIR="$SCRIPT_DIR/.pids"
LOGDIR="$SCRIPT_DIR/.logs"

DS_API_PORT="${DS_API_PORT:-8080}"

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

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'
log()  { echo -e "${GREEN}▸${NC} $*"; }
warn() { echo -e "${YELLOW}▸${NC} $*"; }
err()  { echo -e "${RED}✗${NC} $*" >&2; }
hdr()  { echo -e "\n${BOLD}${BLUE}═══ $* ═══${NC}"; }

mkdir -p "$PIDDIR" "$LOGDIR"

save_pid() { echo "$2" > "$PIDDIR/$1.pid"; }
read_pid() { local f="$PIDDIR/$1.pid"; [[ -f "$f" ]] && cat "$f" || echo ""; }
is_alive()  { local p; p=$(read_pid "$1"); [[ -n "$p" ]] && kill -0 "$p" 2>/dev/null; }

stop_process() {
    local name="$1" pid
    pid=$(read_pid "$name")
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
        kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null || true
        log "stopped $name (pid $pid)"
    fi
    rm -f "$PIDDIR/$name.pid"
}

wait_for_port() {
    local port="$1" label="$2" timeout="${3:-30}"
    for i in $(seq 1 "$timeout"); do
        nc -z localhost "$port" 2>/dev/null && { log "$label ready on port $port (${i}s)"; return 0; }
        sleep 1
    done
    err "$label failed to start on port $port after ${timeout}s"; return 1
}

wait_for_healthy() {
    local service="$1" timeout="${2:-60}"
    for i in $(seq 1 "$timeout"); do
        local status
        status=$(docker compose -f "$COMPOSE_FILE" ps "$service" --format '{{.Status}}' 2>/dev/null || echo "")
        [[ "$status" == *"healthy"* ]] && { log "$service healthy (${i}s)"; return 0; }
        sleep 1
    done
    err "$service not healthy after ${timeout}s"; return 1
}

# Run psql — prefer native client, fall back to docker exec
pg() {
    if command -v psql &>/dev/null; then
        PGPASSWORD=postgres psql -h localhost -U postgres -d ecommerce "$@"
    else
        docker compose -f "$COMPOSE_FILE" exec -T postgres \
            psql -U postgres -d ecommerce "$@"
    fi
}

cmd_up() {
    hdr "Starting PostgreSQL CDC Demo"

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

    hdr "Infrastructure (PostgreSQL + MinIO + Polaris)"
    # Start only the services needed for this demo — skip the datashuttle container
    # (we run DataShuttle as a native binary below so it can connect to localhost:5432).
    docker compose -f "$COMPOSE_FILE" up -d \
        minio minio-init polaris polaris-init postgres

    wait_for_healthy postgres 30
    wait_for_healthy minio 15
    wait_for_healthy polaris 30

    log "waiting for init containers..."
    sleep 6
    for i in $(seq 1 10); do
        curl -sf http://localhost:8181/api/catalog/v1/config >/dev/null 2>&1 && break
        sleep 1
    done
    log "init containers done"

    hdr "DataShuttle"
    if is_alive datashuttle; then
        warn "DataShuttle already running"
    else
        pushd "$PROJECT_ROOT" >/dev/null
        nohup "$BINARY" start > "$LOGDIR/datashuttle.log" 2>&1 &
        save_pid datashuttle $!
        popd >/dev/null
        wait_for_port "$DS_API_PORT" "DataShuttle API" 15
    fi

    hdr "Shuttle"
    local api="http://localhost:${DS_API_PORT}/api/v1/sql"
    local ct='Content-Type: application/json'

    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "CREATE CONNECTION ecommerce_pg TYPE POSTGRES PROPERTIES (host = '\''localhost'\'', port = '\''5432'\'', database = '\''ecommerce'\'', username = '\''postgres'\'', password = '\''postgres'\'', replication_slot = '\''datashuttle_demo'\'', publication = '\''datashuttle_pub'\'')"}' \
        >/dev/null 2>&1 || true
    log "connection ecommerce_pg ensured"

    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "CREATE SHUTTLE ecommerce_cdc SOURCE ecommerce_pg TABLES (customers, products, orders, order_items, payments) TARGET warehouse.ecommerce WITH (schedule = '\''continuous'\'', commit_interval = '\''10 seconds'\'', delete_mode = '\''deletion_vectors'\'', batch_size = '\''5000'\'')"}' \
        >/dev/null 2>&1 || true
    log "shuttle ecommerce_cdc ensured"

    curl -sf -X POST "$api" -H "$ct" \
        -d '{"sql": "RESUME SHUTTLE ecommerce_cdc"}' >/dev/null 2>&1 || true
    log "shuttle started"

    hdr "Demo Running"
    echo ""
    echo -e "  ${BOLD}DataShuttle UI${NC}   http://localhost:${DS_API_PORT}"
    echo -e "  ${BOLD}MinIO Console${NC}    http://localhost:9001  (minioadmin/minioadmin)"
    echo -e "  ${BOLD}PostgreSQL${NC}       localhost:5432  (postgres/postgres, db: ecommerce)"
    echo ""
    echo -e "  Inject:  ${BOLD}./demo.sh inject [table] [rows]${NC}"
    echo -e "  Status:  ${BOLD}./demo.sh status${NC}"
    echo -e "  Stop:    ${BOLD}./demo.sh down${NC}"
    echo -e "  Clean:   ${BOLD}./demo.sh clean${NC}"
    echo -e "  Logs:    ${BOLD}./demo.sh logs${NC}"
    echo ""
}

cmd_down() {
    hdr "Stopping Demo"
    stop_process datashuttle
    for port in "$DS_API_PORT"; do
        local pid; pid=$(lsof -ti :"$port" 2>/dev/null || true)
        [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    done
    docker compose -f "$COMPOSE_FILE" stop \
        minio minio-init polaris polaris-init postgres 2>/dev/null || true
    log "all stopped"
}

cmd_clean() {
    hdr "Cleaning Demo (destroying all data)"
    cmd_down
    docker compose -f "$COMPOSE_FILE" down -v \
        --remove-orphans 2>/dev/null || true
    rm -f "$REGISTRY_DB"
    rm -rf "$PIDDIR" "$LOGDIR"
    log "volumes, registry, and logs removed"
}

cmd_status() {
    hdr "Demo Status"

    echo -e "\n${BOLD}Processes:${NC}"
    if is_alive datashuttle; then
        echo -e "  ${GREEN}●${NC} datashuttle (pid $(read_pid datashuttle))"
    else
        echo -e "  ${RED}●${NC} datashuttle (stopped)"
    fi

    echo -e "\n${BOLD}Docker:${NC}"
    docker compose -f "$COMPOSE_FILE" ps --format "  {{.Name}}\t{{.Status}}" 2>/dev/null || echo "  (not running)"

    echo -e "\n${BOLD}Shuttle:${NC}"
    local state
    state=$(curl -sf "http://localhost:${DS_API_PORT}/api/v1/shuttles/ecommerce_cdc" 2>/dev/null \
        | python3 -c "import sys,json;print(json.load(sys.stdin)['state'])" 2>/dev/null || echo "unreachable")
    echo "  ecommerce_cdc: $state"

    echo -e "\n${BOLD}Row counts:${NC}"
    pg -t -A -c "SELECT
        'customers: ' || (SELECT count(*) FROM customers) ||
        '  products: ' || (SELECT count(*) FROM products) ||
        '  orders: '   || (SELECT count(*) FROM orders)" 2>/dev/null \
        | sed 's/^/  /' || echo "  (unreachable)"
    echo ""
}

cmd_inject() {
    # inject [table] [rows]
    # inject              → 1000 rows (all tables, proportional)
    # inject 5000         → 5000 customers + proportional rows
    # inject orders 500   → 500 orders using existing customers/products
    # inject customers 50 → 50 customers only
    # inject products 30  → 30 products only
    #
    # Uses set-based INSERT ... SELECT FROM generate_series — no row-by-row
    # PL/pgSQL loops. Batches of BATCH_SIZE rows per transaction so Postgres
    # WAL and DataShuttle pending_events stay bounded.

    local table="all" rows=1000
    local BATCH_SIZE="${INJECT_BATCH_SIZE:-10000}"   # rows per transaction; override: INJECT_BATCH_SIZE=5000 ./demo.sh inject ...

    case $# in
        0) ;;
        1) [[ "$1" =~ ^[0-9]+$ ]] && rows="$1" || table="$1" ;;
        2) table="$1"; rows="$2" ;;
        *) err "Usage: inject [table] [rows]"; exit 1 ;;
    esac

    hdr "Injecting into ${table} (${rows} rows)"

    pg -c "SELECT 1" >/dev/null 2>&1 \
        || { err "PostgreSQL unreachable — run ./demo.sh up first"; exit 1; }

    # ── set-based helpers ─────────────────────────────────────────────

    _insert_customers() {
        local offset=$1 count=$2
        pg -v ON_ERROR_STOP=1 -c "
INSERT INTO customers(first_name,last_name,email,city,country,segment)
SELECT
  ('{James,Mary,Robert,Patricia,John,Jennifer,Michael,Linda,David,Elizabeth}'::text[])[1+(i%10)],
  ('{Smith,Johnson,Williams,Brown,Jones,Garcia,Miller,Davis,Rodriguez,Martinez}'::text[])[1+(i%10)],
  'u'||($offset+i)||'_'||(extract(epoch from now())::bigint%1000000)||'@example.com',
  ('{NYC,LA,Chicago,Houston,Phoenix,Austin,Seattle,Miami,Denver,Boston}'::text[])[1+(i%10)],
  ('{US,GB,DE,FR,CA,AU,JP,BR,IN,MX}'::text[])[1+(i%10)],
  ('{standard,premium,vip}'::text[])[1+(i%3)]
FROM generate_series(1,$count) AS i;" >/dev/null
    }

    _insert_products() {
        local offset=$1 count=$2
        pg -v ON_ERROR_STOP=1 -c "
INSERT INTO products(sku,name,category,price,weight_kg)
SELECT
  'SKU-'||lpad(($offset+i)::text,6,'0'),
  'Product '||($offset+i),
  ('{electronics,clothing,books,sports,home,tools}'::text[])[1+(i%6)],
  round((9.99+(i*7.13)%990)::numeric,2),
  round((0.1+(i*0.37)%9.9)::numeric,2)
FROM generate_series(1,$count) AS i;" >/dev/null
    }

    _insert_orders() {
        local count=$1
        local cm pm
        cm=$(pg -t -A -c "SELECT GREATEST(MAX(id),1) FROM customers")
        pm=$(pg -t -A -c "SELECT GREATEST(MAX(id),1) FROM products")
        pg -v ON_ERROR_STOP=1 -c "
WITH new_orders AS (
  INSERT INTO orders(customer_id,status,total,currency)
  SELECT
    1+((i-1)%$cm),
    ('{pending,confirmed,shipped,delivered,cancelled}'::text[])[1+(i%5)],
    round((10+(i*33.7)%2000)::numeric,2), 'USD'
  FROM generate_series(1,$count) AS i
  RETURNING id
),
numbered AS (
  SELECT id, row_number() OVER (ORDER BY id) AS rn FROM new_orders
),
_items AS (
  INSERT INTO order_items(order_id,product_id,quantity,unit_price)
  SELECT id, 1+((rn-1)%$pm), 1+(rn%3), round((9.99+(rn*12.5)%500)::numeric,2)
  FROM numbered
)
INSERT INTO payments(order_id,method,amount,status,processed_at)
SELECT id,
  ('{credit_card,debit_card,paypal,apple_pay}'::text[])[1+(rn%4)],
  round((10+(rn*33.7)%2000)::numeric,2),
  ('{completed,pending,failed}'::text[])[1+(rn%3)],
  CASE WHEN rn%3!=2 THEN now()-(rn%30)*interval'1 hour' END
FROM numbered;" >/dev/null
    }

    # ── batched loop ──────────────────────────────────────────────────

    _run_batched() {
        local fn=$1 total=$2 offset=${3:-0}
        local done=0
        while (( done < total )); do
            local batch=$(( total - done < BATCH_SIZE ? total - done : BATCH_SIZE ))
            "$fn" "$((offset + done))" "$batch"
            done=$(( done + batch ))
            log "  $fn: $done / $total"
        done
    }

    _run_batched_nooffset() {
        local fn=$1 total=$2
        local done=0
        while (( done < total )); do
            local batch=$(( total - done < BATCH_SIZE ? total - done : BATCH_SIZE ))
            "$fn" "$batch"
            done=$(( done + batch ))
            log "  $fn: $done / $total"
        done
    }

    # ── commands ──────────────────────────────────────────────────────

    case "$table" in
        all)
            local cs ps np no
            cs=$(pg -t -A -c "SELECT COALESCE(MAX(id),0) FROM customers")
            ps=$(pg -t -A -c "SELECT COALESCE(MAX(id),0) FROM products")
            np=$(( rows / 5 < 1 ? 1 : rows / 5 ))
            no=$(( rows * 4 ))

            log "customers ($rows) ..."
            _run_batched _insert_customers "$rows" "$cs"
            log "products ($np) ..."
            _run_batched _insert_products "$np" "$ps"
            log "orders + items + payments ($no) ..."
            _run_batched_nooffset _insert_orders "$no"
            log "done — $rows customers · $np products · $no orders"
            ;;

        customers)
            local cs
            cs=$(pg -t -A -c "SELECT COALESCE(MAX(id),0) FROM customers")
            _run_batched _insert_customers "$rows" "$cs"
            log "done — $rows customers"
            ;;

        products)
            local ps
            ps=$(pg -t -A -c "SELECT COALESCE(MAX(id),0) FROM products")
            _run_batched _insert_products "$rows" "$ps"
            log "done — $rows products"
            ;;

        orders)
            _run_batched_nooffset _insert_orders "$rows"
            log "done — $rows orders + items + payments"
            ;;

        *)
            err "Unknown table: $table"
            err "Valid: all, customers, products, orders"
            exit 1
            ;;
    esac

    log "changes will be captured by the CDC shuttle"
}

cmd_logs() {
    hdr "DataShuttle Logs"
    [[ -f "$LOGDIR/datashuttle.log" ]] \
        && tail -f "$LOGDIR/datashuttle.log" \
        || err "no logs (is DataShuttle running?)"
}

cmd_open() {
    if command -v open &>/dev/null; then open "http://localhost:${DS_API_PORT}"
    elif command -v xdg-open &>/dev/null; then xdg-open "http://localhost:${DS_API_PORT}"
    else log "Open http://localhost:${DS_API_PORT}"; fi
}

cmd_restart() { cmd_down; sleep 2; cmd_up; }

case "${1:-help}" in
    up)      cmd_up ;;
    down)    cmd_down ;;
    clean)   cmd_clean ;;
    status)  cmd_status ;;
    restart) cmd_restart ;;
    inject)  shift; cmd_inject "$@" ;;
    logs)    cmd_logs ;;
    open)    cmd_open ;;
    *)
        echo "Usage: $0 {up|down|clean|status|restart|inject|logs|open}"
        echo ""
        echo "  up                    Start everything from scratch"
        echo "  down                  Stop all processes (keep data)"
        echo "  clean                 Stop + destroy all data and volumes"
        echo "  status                Show processes, shuttle state, row counts"
        echo "  restart               down + up"
        echo "  inject                Insert 1000 rows across all tables (proportional)"
        echo "  inject <n>            Insert <n> customers + proportional rows"
        echo "  inject <table> <n>    Insert <n> rows into specific table"
        echo "                        Tables: all, customers, products, orders"
        echo "  logs                  Tail DataShuttle logs"
        echo "  open                  Open DataShuttle UI in browser"
        exit 1
        ;;
esac
