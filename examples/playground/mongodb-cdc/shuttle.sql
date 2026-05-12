-- Playground template for MongoDB CDC scenario.

CREATE CONNECTION IF NOT EXISTS {connection}
  TYPE MONGODB
  PROPERTIES (
    uri = 'mongodb://localhost:27017/social_media?replicaSet=rs0',
    change_stream_name = '{shuttle}'
  );

CREATE SHUTTLE IF NOT EXISTS {shuttle}
  SOURCE {connection}
  TABLES (users, posts, comments)
  TARGET warehouse.{namespace}
  SCHEDULE CONTINUOUS
  WITH (
    commit_interval = '10 seconds'
  );

RESUME SHUTTLE {shuttle};
