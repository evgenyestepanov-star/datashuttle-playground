-- Playground Tier-3 scenario: Cassandra wide rows + batching.
-- A single-row keyspace with a wide CLUSTERING column so we can create
-- rows with up to thousands of cells to exercise the batching code.

CREATE KEYSPACE IF NOT EXISTS playground
  WITH REPLICATION = { 'class': 'SimpleStrategy', 'replication_factor': 1 };

CREATE TABLE IF NOT EXISTS playground.wide_rows (
    partition_key TEXT,
    clustering    TEXT,
    payload       TEXT,
    ts            TIMESTAMP,
    PRIMARY KEY (partition_key, clustering)
) WITH CLUSTERING ORDER BY (clustering ASC);
