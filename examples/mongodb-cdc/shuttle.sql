-- DataShuttle shuttle for MongoDB social media demo.
-- Run: datashuttle sql -f examples/mongodb-cdc/shuttle.sql

CREATE CONNECTION social_mongo
  TYPE MONGODB
  PROPERTIES (
    uri = 'mongodb://localhost:27017/social_media?replicaSet=rs0'
  );

CREATE SHUTTLE social_cdc
  SOURCE social_mongo
  TABLES (users, posts, comments)
  TARGET warehouse.social
  WITH (
    mode = 'SNAPSHOT_THEN_CDC',
    commit_interval = '10 seconds'
  );
