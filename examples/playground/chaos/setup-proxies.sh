#!/usr/bin/env bash
# Bootstrap Toxiproxy proxies for the chaos scenarios. Run once after
# `docker compose --profile chaos up -d`.
set -euo pipefail
TP="${TOXIPROXY_URL:-http://localhost:8474}"
for p in postgres kafka; do
  case "$p" in
    postgres) UPSTREAM=postgres:5432   LISTEN=25432 ;;
    kafka)    UPSTREAM=redpanda:9092   LISTEN=28015 ;;
  esac
  curl -sf -X POST "$TP/proxies" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"$p\",\"listen\":\"0.0.0.0:$LISTEN\",\"upstream\":\"$UPSTREAM\",\"enabled\":true}" \
    >/dev/null || curl -sf -X POST "$TP/proxies/$p" \
    -H 'Content-Type: application/json' \
    -d "{\"listen\":\"0.0.0.0:$LISTEN\",\"upstream\":\"$UPSTREAM\",\"enabled\":true}" \
    >/dev/null
  echo "proxy ready: $p listen=$LISTEN upstream=$UPSTREAM"
done
