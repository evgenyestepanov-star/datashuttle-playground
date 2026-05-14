# Action: append 200 more stream entries to the session's events
# stream. The next shuttle tick picks them up via XRANGE and grows
# the warehouse table by ~200 rows.

EVAL "for i=1,200 do redis.call('XADD','{namespace}:events','*','type',ARGV[1+(i%3)],'amount',tostring(math.random()*100),'user_id','u'..tostring(math.random(1000))) end; return 200" 0 purchase refund review
