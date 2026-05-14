# Playground init for the redis-streams-cdc scenario. Unlike the
# snapshot variant, CDC mode reads `$` (only new entries after the
# consumer group is created) — so we seed nothing here. The user's
# `produce-burst` action XADDs entries while the shuttle is running
# and watches them land continuously via XREADGROUP.
#
# We still need a stream to exist for XGROUP CREATE MKSTREAM to bind
# to — XADD a single placeholder that the connector reads + emits
# (with stream_id timestamped at session-start so subsequent entries
# remain monotonically increasing).

XADD {namespace}:events * type ready user_id playground
