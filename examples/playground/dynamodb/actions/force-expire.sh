#!/usr/bin/env bash
# DynamoDB Local has a background scan that reaps expired items within
# a minute. This helper waits then lists the remaining rows so the user
# can see the TTL sweep has happened.
set -euo pipefail
ENDPOINT="${DYNAMO_ENDPOINT:-http://localhost:8000}"
echo "waiting 65s for DynamoDB TTL sweep..."
sleep 65
AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
  aws --endpoint-url "$ENDPOINT" dynamodb scan \
    --table-name playground_items \
    --select COUNT
