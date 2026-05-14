# Playground init for the redis-streams-events scenario.
#
# The playground dispatcher's exec_redis branch parses this file as a
# newline-delimited Redis command script — one command per line,
# whitespace-tokenized, `#`-comments stripped. `{namespace}` is
# replaced at the handler before the body reaches the dispatcher so
# every key reference here lands in the session's private keyspace.
#
# We seed 500 stream entries in a single EVAL/Lua loop so the round-
# trip count stays at one. The DataShuttle XRANGE-based connector
# picks the same key up at shuttle resume time and materializes each
# entry as one row in warehouse.{namespace}.events.

EVAL "for i=1,500 do redis.call('XADD','{namespace}:events','*','type',ARGV[1+(i%3)],'amount',tostring(math.random()*100),'user_id','u'..tostring(math.random(1000))) end; return 500" 0 purchase refund review
