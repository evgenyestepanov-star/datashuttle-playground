#!/usr/bin/env bash
# Merge the two most recently open adjacent shards back together.
set -euo pipefail
ENDPOINT="${LOCALSTACK_ENDPOINT:-http://localhost:4566}"
SHARDS=$(
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" kinesis describe-stream \
      --stream-name playground-events \
      --query 'StreamDescription.Shards[-2:].ShardId' \
      --output text
)
read -r A B <<<"$SHARDS"
if [[ -z "$A" || -z "$B" ]]; then
  echo "need at least 2 shards to merge (only found: $SHARDS)" >&2
  exit 1
fi
AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
  aws --endpoint-url "$ENDPOINT" kinesis merge-shards \
    --stream-name playground-events \
    --shard-to-merge "$A" \
    --adjacent-shard-to-merge "$B"
echo "merged $A + $B"
