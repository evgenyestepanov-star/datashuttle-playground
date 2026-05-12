#!/usr/bin/env bash
set -euo pipefail
echo "=== MongoDB CDC Demo Verification ==="
echo ""
echo "Source collection counts:"
mongosh --host localhost --quiet --eval "
  const db = db.getSiblingDB('social_media');
  print('  users:    ' + db.users.countDocuments());
  print('  posts:    ' + db.posts.countDocuments());
  print('  comments: ' + db.comments.countDocuments());
" 2>/dev/null || echo "  (MongoDB not reachable)"
echo ""
echo "Shuttle status:"
curl -s http://localhost:8080/api/v1/shuttles/social_cdc/status 2>/dev/null | python3 -m json.tool || echo "  (API not reachable)"
echo ""
echo "=== Done ==="
