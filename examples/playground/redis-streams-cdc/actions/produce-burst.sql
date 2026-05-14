# Action: XADD 200 events in a single round-trip. The connector's
# XREADGROUP loop is blocking on this very stream key, so the entries
# materialize through Arrow Flight within seconds (vs the 5-minute
# poll cadence of the snapshot scenario).

EVAL "for i=1,200 do redis.call('XADD','{namespace}:events','*','type',ARGV[1+(i%3)],'amount',tostring(math.random()*100),'user_id','u'..tostring(math.random(1000))) end; return 200" 0 purchase refund review
