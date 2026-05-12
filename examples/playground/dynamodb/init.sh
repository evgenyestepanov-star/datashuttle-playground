#!/usr/bin/env bash
# Create a `playground_items` table with Streams enabled + TTL configured
# on the `expires_at` attribute, so the dynamodb-streams scenario can
# emit TTL-delete events into the shuttle.
set -euo pipefail

ENDPOINT="${DYNAMO_ENDPOINT:-http://localhost:8000}"
AWS_ACCESS_KEY_ID=local \
AWS_SECRET_ACCESS_KEY=local \
AWS_DEFAULT_REGION=us-east-1 \
aws --endpoint-url "$ENDPOINT" dynamodb create-table \
    --table-name playground_items \
    --attribute-definitions AttributeName=pk,AttributeType=S \
    --key-schema AttributeName=pk,KeyType=HASH \
    --billing-mode PAY_PER_REQUEST \
    --stream-specification StreamEnabled=true,StreamViewType=NEW_AND_OLD_IMAGES \
    >/dev/null 2>&1 || true

AWS_ACCESS_KEY_ID=local \
AWS_SECRET_ACCESS_KEY=local \
AWS_DEFAULT_REGION=us-east-1 \
aws --endpoint-url "$ENDPOINT" dynamodb update-time-to-live \
    --table-name playground_items \
    --time-to-live-specification "Enabled=true,AttributeName=expires_at" \
    >/dev/null 2>&1 || true

echo "playground_items table ready with Streams+TTL"
