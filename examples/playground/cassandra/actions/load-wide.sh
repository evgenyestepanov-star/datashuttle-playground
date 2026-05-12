#!/usr/bin/env bash
# usage: load-wide.sh N
# Insert N rows each containing 1000 clustering columns — plenty to
# demonstrate batching and memory stability.
set -euo pipefail
N="${1:-500}"
for pk in $(seq 1 "$N"); do
  (
    echo "BEGIN BATCH"
    for c in $(seq 1 1000); do
      echo "  INSERT INTO playground.wide_rows (partition_key, clustering, payload, ts) VALUES ('pk-$pk', 'c-$c', 'payload-$pk-$c', toTimestamp(now()));"
    done
    echo "APPLY BATCH;"
  ) | docker compose -f examples/docker-compose.yml exec -T cassandra cqlsh >/dev/null
done
echo "loaded $N wide rows"
