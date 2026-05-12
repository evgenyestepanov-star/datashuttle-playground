# MongoDB CDC Demo

Demonstrates DataShuttle ingesting social media data from MongoDB via Change Streams.

## Data

| Collection | Documents | Description |
|-----------|-----------|-------------|
| `users` | 200 | Profiles with nested settings, follower counts |
| `posts` | 1,000 | Content with embedded author, tags array, media |
| `comments` | 3,000 | Thread comments with likes and edit tracking |

## Steps

```bash
# 1. Start infrastructure (MongoDB runs as replica set for change streams)
docker compose -f examples/docker-compose.yml up -d

# 2. Start DataShuttle
./target/release/datashuttle start

# 3. Create shuttle
./target/release/datashuttle sql -f examples/mongodb-cdc/shuttle.sql

# 4. Generate changes
mongosh --host localhost examples/mongodb-cdc/generate-changes.js

# 5. Verify
bash examples/mongodb-cdc/verify.sh

# 6. Cleanup
./target/release/datashuttle sql -e "DROP SHUTTLE social_cdc"
```
