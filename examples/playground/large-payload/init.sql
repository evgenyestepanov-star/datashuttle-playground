-- Playground Tier-4 scenario: large payload replication.
CREATE DATABASE IF NOT EXISTS large_payload;
USE large_payload;
CREATE TABLE IF NOT EXISTS blobs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255),
    payload LONGBLOB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
