#!/usr/bin/env bash
# Insert one row with a 50MB BLOB payload into large_payload.blobs.
set -euo pipefail
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT
dd if=/dev/urandom of="$TMPFILE" bs=1M count=50 status=none
HEX=$(xxd -p -c 99999999 "$TMPFILE")
SQL="INSERT INTO large_payload.blobs (name, payload) VALUES ('blob-$(date +%s)', UNHEX('$HEX'));"
printf '%s\n' "$SQL" \
  | docker compose -f examples/docker-compose.yml exec -T mysql \
      mysql -uroot -prootpass --default-character-set=utf8mb4
echo "inserted 50MB blob"
