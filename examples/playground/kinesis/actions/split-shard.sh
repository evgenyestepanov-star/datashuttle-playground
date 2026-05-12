#!/usr/bin/env bash
# Split the first shard of playground-events; a resharding event the
# shuttle must follow without data loss.
set -euo pipefail
ENDPOINT="${LOCALSTACK_ENDPOINT:-http://localhost:4566}"
SHARD_ID=$(
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" kinesis describe-stream \
      --stream-name playground-events \
      --query 'StreamDescription.Shards[0].ShardId' \
      --output text
)
START=$(
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" kinesis describe-stream \
      --stream-name playground-events \
      --query 'StreamDescription.Shards[0].HashKeyRange.StartingHashKey' \
      --output text
)
END=$(
  AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
    aws --endpoint-url "$ENDPOINT" kinesis describe-stream \
      --stream-name playground-events \
      --query 'StreamDescription.Shards[0].HashKeyRange.EndingHashKey' \
      --output text
)
MID=$(python3 -c "print(($START + $END) // 2)")
AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_DEFAULT_REGION=us-east-1 \
  aws --endpoint-url "$ENDPOINT" kinesis split-shard \
    --stream-name playground-events \
    --shard-to-split "$SHARD_ID" \
    --new-starting-hash-key "$MID"
echo "split shard $SHARD_ID at $MID"
