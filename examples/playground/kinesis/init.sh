#!/usr/bin/env bash
# Create a `playground-events` Kinesis stream on LocalStack.
set -euo pipefail
ENDPOINT="${LOCALSTACK_ENDPOINT:-http://localhost:4566}"
AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
  aws --endpoint-url "$ENDPOINT" kinesis create-stream \
    --stream-name playground-events \
    --shard-count 2 \
    >/dev/null 2>&1 || true
echo "playground-events stream ready (2 shards)"
