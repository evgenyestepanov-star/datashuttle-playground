#!/usr/bin/env bash
# Remove every toxic from the postgres proxy (restore healthy link).
set -euo pipefail
TP="${TOXIPROXY_URL:-http://localhost:8474}"
curl -sf "$TP/proxies/postgres/toxics" \
  | python3 -c 'import json,sys; [print(t["name"]) for t in json.load(sys.stdin)]' \
  | while read -r name; do
      curl -sf -X DELETE "$TP/proxies/postgres/toxics/$name" >/dev/null
    done
echo "healed postgres proxy"
