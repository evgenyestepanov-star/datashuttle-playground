#!/usr/bin/env bash
# Drop a percentage of bytes on the postgres proxy (limit_data + reset_peer).
# usage: toxic-loss.sh PERCENT
set -euo pipefail
PCT="${1:-10}"
TP="${TOXIPROXY_URL:-http://localhost:8474}"
# Compute a byte budget for approximately PCT% drop over a 30s window.
BUDGET=$((100000 * (100 - PCT) / 100))
curl -sf -X POST "$TP/proxies/postgres/toxics" \
  -H 'Content-Type: application/json' \
  -d "{\"name\":\"loss\",\"type\":\"limit_data\",\"stream\":\"upstream\",\"attributes\":{\"bytes\":$BUDGET}}" \
  >/dev/null || true
echo "injected ~${PCT}% packet loss (byte budget $BUDGET)"
