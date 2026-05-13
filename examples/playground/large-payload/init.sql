-- Playground Tier-4 scenario: large payload replication.
-- The playground dispatcher injects `USE {namespace};` so unqualified
-- table names land in the session's private database; no explicit
-- CREATE DATABASE / USE needed here.

CREATE TABLE IF NOT EXISTS blobs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255),
    payload LONGBLOB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Seed two 1MB blobs so the snapshot phase has rows to carry into
-- Iceberg. MySQL's RANDOM_BYTES() caps at 1024 bytes — `REPEAT(MD5(..), N)`
-- builds a 1MB pseudo-random blob (32 chars * 32768).
INSERT INTO blobs (name, payload) VALUES
    ('seed-1', REPEAT(MD5(RAND()), 32768)),
    ('seed-2', REPEAT(MD5(RAND()), 32768));
