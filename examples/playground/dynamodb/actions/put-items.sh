#!/usr/bin/env bash
# usage: put-items.sh N TTL_SECS
# Inserts N items into playground_items, each expiring TTL_SECS in the future.
set -euo pipefail
N="${1:-100}"
TTL="${2:-60}"
ENDPOINT="${DYNAMO_ENDPOINT:-http://localhost:8000}"
EXPIRES=$(( $(date +%s) + TTL ))
for i in $(seq 1 "$N"); do
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" dynamodb put-item \
      --table-name playground_items \
      --item "{\"pk\":{\"S\":\"pg-$i\"},\"payload\":{\"S\":\"playground $i\"},\"expires_at\":{\"N\":\"$EXPIRES\"}}" \
      >/dev/null
done
echo "inserted $N items, TTL=$TTL s"
