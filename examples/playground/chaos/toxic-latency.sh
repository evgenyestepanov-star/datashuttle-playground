#!/usr/bin/env bash
# Inject added latency on both upstream + downstream of the postgres proxy.
# usage: toxic-latency.sh MILLIS
set -euo pipefail
MS="${1:-500}"
TP="${TOXIPROXY_URL:-http://localhost:8474}"
for dir in upstream downstream; do
  curl -sf -X POST "$TP/proxies/postgres/toxics" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"lat_$dir\",\"type\":\"latency\",\"stream\":\"$dir\",\"attributes\":{\"latency\":$MS,\"jitter\":50}}" \
    >/dev/null || true
done
echo "injected ${MS}ms latency on postgres proxy"
