#!/usr/bin/env bash
# Produce N records into the playground-events Kinesis stream.
set -euo pipefail
N="${1:-1000}"
ENDPOINT="${LOCALSTACK_ENDPOINT:-http://localhost:4566}"
for i in $(seq 1 "$N"); do
  PAYLOAD=$(printf '{"id":%s,"event":"playground","ts":"%s"}' "$i" "$(date -Iseconds)")
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" kinesis put-record \
      --stream-name playground-events \
      --partition-key "pk-$((i % 8))" \
      --data "$(printf '%s' "$PAYLOAD" | base64)" \
      >/dev/null
done
echo "produced $N records"
