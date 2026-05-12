#!/usr/bin/env python3
"""
DataShuttle Realtime Fraud Detection Dashboard.

Unified analytics powered by DuckDB:
  - Arrow Flight hot buffer (<1ms, in-memory)
  - Iceberg cold store (Polaris-managed, S3/MinIO)
  - Deduplicated unified view via UNION ALL

Usage:
    pip install pyarrow duckdb
    python3 examples/realtime-demo/dashboard-server.py
    open http://localhost:3000
"""

import json
import time
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

try:
    import pyarrow.flight as flight
    import duckdb
except ImportError:
    print("pip install pyarrow duckdb")
    exit(1)

FLIGHT_URI = "grpc://localhost:8815"
S3_DATA = "s3://warehouse/realtime/clickstream/data/*.parquet"
PORT = 3000

# Cache: cold data refreshed every 10s, full stats every 5s
_cold_cache = {"data": None, "rows": 0, "ts": 0}
_COLD_TTL = 10  # seconds
_stats_cache = {"data": None, "ts": 0}
_STATS_TTL = 4  # seconds — analytics refresh interval

# Persistent DuckDB connection (reused across requests)
_duckdb_con = None
_duckdb_lock = threading.Lock()


def get_duckdb():
    """Get or create a persistent DuckDB connection."""
    global _duckdb_con
    if _duckdb_con is None:
        _duckdb_con = duckdb.connect()
        _duckdb_con.sql("""CREATE SECRET IF NOT EXISTS s3 (TYPE S3, KEY_ID 'minioadmin',
            SECRET 'minioadmin', REGION 'us-east-1', ENDPOINT 'localhost:9000',
            USE_SSL false, URL_STYLE 'path')""")
    return _duckdb_con


def unescape(col):
    """SQL to unescape double-escaped Kafka JSON into valid JSON."""
    return f"""json(replace(trim({col}, '"'), '\\"', '"'))"""


def query_stats():
    """Run unified DuckDB queries across hot (Flight) + cold (Iceberg/S3)."""
    global _cold_cache

    with _duckdb_lock:
        con = get_duckdb()

    # Hot path — read full buffer for count, but limit analytics to last 10K rows
    hot_rows = 0
    try:
        client = flight.FlightClient(FLIGHT_URI)
        hot = client.do_get(flight.Ticket(b"clickstream")).read_all()
        hot_rows = hot.num_rows
        # For analytics, use only last 10K rows to keep JSON parsing fast
        if hot.num_rows > 10000:
            hot = hot.slice(hot.num_rows - 10000)
        con.register("hot_raw", hot)
        con.sql(f"""CREATE OR REPLACE VIEW hot AS
            SELECT "offset", "partition", key as user_id,
                   {unescape('"value"')} as payload, topic, _op, _ts
            FROM hot_raw""")
    except Exception:
        con.sql("CREATE OR REPLACE VIEW hot AS SELECT NULL::VARCHAR as \"offset\", NULL::VARCHAR as \"partition\", NULL::VARCHAR as user_id, NULL::VARCHAR as payload, NULL::VARCHAR as topic, NULL::VARCHAR as _op, NULL::BIGINT as _ts WHERE false")

    # Cold path — cached (S3 parquet changes only at Iceberg commit intervals)
    cold_rows = 0
    now = time.time()
    if _cold_cache["data"] is not None and (now - _cold_cache["ts"]) < _COLD_TTL:
        # Reuse cached cold data
        con.register("cold_cached", _cold_cache["data"])
        con.sql(f"""CREATE OR REPLACE VIEW cold AS
            SELECT "offset", "partition", key as user_id,
                   {unescape('"value"')} as payload,
                   topic, _op, _ts
            FROM cold_cached""")
        cold_rows = _cold_cache["rows"]
    else:
        try:
            cold_raw = con.sql(f"SELECT * FROM read_parquet('{S3_DATA}')").fetch_arrow_table()
            _cold_cache["data"] = cold_raw
            _cold_cache["rows"] = cold_raw.num_rows
            _cold_cache["ts"] = now
            cold_rows = cold_raw.num_rows
            con.register("cold_cached", cold_raw)
            con.sql(f"""CREATE OR REPLACE VIEW cold AS
                SELECT "offset", "partition", key as user_id,
                       {unescape('"value"')} as payload,
                       topic, _op, _ts
                FROM cold_cached""")
        except Exception as e:
            import traceback; traceback.print_exc()
            con.sql("CREATE OR REPLACE VIEW cold AS SELECT NULL::VARCHAR as \"offset\", NULL::VARCHAR as \"partition\", NULL::VARCHAR as user_id, NULL::VARCHAR as payload, NULL::VARCHAR as topic, NULL::VARCHAR as _op, NULL::BIGINT as _ts WHERE false")

    # Unified — deduplicated by (partition, offset) composite key.
    # Kafka offsets are per-partition, so offset alone is not unique.
    con.sql("""CREATE OR REPLACE VIEW unified AS
        SELECT *, 'hot' as source FROM hot
        UNION ALL
        SELECT *, 'cold' as source FROM cold
        WHERE ("partition", "offset") NOT IN (
            SELECT "partition", "offset" FROM hot
            WHERE "offset" IS NOT NULL AND "partition" IS NOT NULL
        )""")
    uni_rows = con.sql("SELECT count(*) FROM unified").fetchone()[0]

    # Analytics
    regions = [{"region": r[0] or "?", "events": r[1], "users": r[2]}
        for r in con.sql("""SELECT json_extract_string(payload,'$.region') as r,
            count(*) as n, count(distinct user_id) as u
            FROM unified WHERE payload IS NOT NULL AND payload IS NOT NULL GROUP BY r ORDER BY n DESC""").fetchall()]

    actions = [{"action": a[0] or "?", "events": a[1]}
        for a in con.sql("""SELECT json_extract_string(payload,'$.action') as a, count(*) as n
            FROM hot WHERE payload IS NOT NULL AND payload IS NOT NULL GROUP BY a ORDER BY n DESC""").fetchall()]

    top_users = [{"user_id": u[0], "events": u[1], "sessions": u[2]}
        for u in con.sql("""SELECT user_id, count(*) as n,
            count(distinct json_extract_string(payload,'$.session_id')) as s
            FROM hot WHERE user_id IS NOT NULL GROUP BY user_id ORDER BY n DESC LIMIT 10""").fetchall()]

    devices = [{"device": d[0] or "?", "events": d[1]}
        for d in con.sql("""SELECT json_extract_string(payload,'$.device') as d, count(*) as n
            FROM unified WHERE payload IS NOT NULL AND payload IS NOT NULL GROUP BY d ORDER BY n DESC""").fetchall()]

    recent = [{"offset": r[0], "user": r[1], "action": r[2], "page": r[3], "region": r[4], "device": r[5]}
        for r in con.sql("""SELECT "offset", user_id,
            json_extract_string(payload,'$.action'), json_extract_string(payload,'$.page'),
            json_extract_string(payload,'$.region'), json_extract_string(payload,'$.device')
            FROM hot WHERE payload IS NOT NULL AND payload IS NOT NULL
            ORDER BY CAST("offset" AS INTEGER) DESC LIMIT 8""").fetchall()]

    fraud = [{"user_id": f[0], "events": f[1], "sessions": f[2], "regions": f[3]}
        for f in con.sql("""SELECT user_id, count(*) as n,
            count(distinct json_extract_string(payload,'$.session_id')) as s,
            count(distinct json_extract_string(payload,'$.region')) as r
            FROM hot WHERE user_id IS NOT NULL GROUP BY user_id
            HAVING count(*) > 15 OR count(distinct json_extract_string(payload,'$.region')) > 4
            ORDER BY n DESC LIMIT 5""").fetchall()]

    result = {
        "hot_rows": hot_rows, "cold_rows": cold_rows, "unified_rows": uni_rows,
        "hot_status": "online" if hot_rows > 0 else "offline",
        "cold_status": "online" if cold_rows > 0 else "offline",
        "regions": regions, "actions": actions, "top_users": top_users,
        "devices": devices, "recent": recent, "fraud_alerts": fraud,
    }
    _stats_cache["data"] = result
    _stats_cache["ts"] = time.time()
    return result


def get_stats():
    """Return cached stats or recompute if stale."""
    now = time.time()
    if _stats_cache["data"] is not None and (now - _stats_cache["ts"]) < _STATS_TTL:
        return _stats_cache["data"]
    return query_stats()


HTML = r"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8">
<title>DataShuttle — Fraud Detection</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0a0e17;color:#e0e0e0;padding:16px;overflow-x:hidden}
h1{text-align:center;margin-bottom:4px;color:#60a5fa;font-size:22px}
.sub{text-align:center;color:#6b7280;margin-bottom:16px;font-size:12px}
.g3{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;max-width:1400px;margin:0 auto 10px}
.g2{display:grid;grid-template-columns:1fr 1fr;gap:10px;max-width:1400px;margin:0 auto 10px}
.c{background:#111827;border:1px solid #1f2937;border-radius:10px;padding:12px;transition:border-color .3s}
.c:hover{border-color:#374151}
.c h2{font-size:11px;text-transform:uppercase;letter-spacing:1px;margin-bottom:8px;color:#9ca3af}
.big{font-size:32px;font-weight:700;text-align:center;padding:6px 0;transition:all .4s ease-out}
.or{color:#f97316}.bl{color:#3b82f6}.gr{color:#22c55e}.rd{color:#ef4444}.pu{color:#a855f7}
.row{display:flex;justify-content:space-between;padding:4px 0;border-bottom:1px solid #1f2937;font-size:11px}
.row:last-child{border:none}.lab{color:#9ca3af}
.bar{height:5px;border-radius:3px;margin-top:2px;transition:width .6s ease-out}
.ratio-bar{height:8px;border-radius:4px;background:#1f2937;overflow:hidden;margin:6px 0}
.ratio-fill{height:100%;border-radius:4px;transition:width .8s ease-out;background:linear-gradient(90deg,#f97316,#3b82f6)}
table{width:100%;font-size:10px;border-collapse:collapse}
th{text-align:left;color:#6b7280;padding:3px 5px;border-bottom:1px solid #1f2937}
td{padding:3px 5px;border-bottom:1px solid #0f1521;color:#d1d5db}
.al{background:#7f1d1d;border:1px solid #ef4444;border-radius:6px;padding:8px;margin-bottom:6px;font-size:11px;animation:alertPulse 2s infinite}
.al-t{color:#fca5a5;font-weight:600}
.dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:5px}
.on{background:#22c55e;box-shadow:0 0 6px #22c55e;animation:pulse 1.5s infinite}
.off{background:#ef4444}
.r{color:#4b5563;font-size:10px;text-align:right;margin-bottom:4px}
.spark{display:block;margin:4px auto}
.evt-new{animation:slideIn .3s ease-out}
.delta{font-size:11px;text-align:center;color:#6b7280;margin-top:2px}
.delta b{color:#f97316}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.5}}
@keyframes alertPulse{0%,100%{border-color:#ef4444}50%{border-color:#f97316}}
@keyframes slideIn{from{opacity:0;transform:translateX(20px)}to{opacity:1;transform:translateX(0)}}
@keyframes countUp{from{opacity:.5;transform:scale(.95)}to{opacity:1;transform:scale(1)}}
.count-anim{animation:countUp .3s ease-out}
@media(max-width:900px){.g3,.g2{grid-template-columns:1fr}}
</style></head><body>
<h1>⚡ DataShuttle — Real-Time Fraud Detection</h1>
<p class="sub">DuckDB unified: Arrow Flight (hot) ∪ Iceberg/Polaris (cold) · Kafka → DataShuttle → Analytics</p>
<p class="r" id="r">Loading...</p>

<!-- Row 1: Counters + Sparkline -->
<div class="g3">
  <div class="c"><h2>🔥 Hot Buffer (Flight)</h2><div class="big or" id="h">—</div>
    <div class="row"><span class="lab">Status</span><span id="hs">—</span></div>
    <div class="row"><span class="lab">Latency</span><span>&lt;1ms (gRPC)</span></div>
    <svg class="spark" id="sparkH" width="100%" height="30" viewBox="0 0 200 30"></svg></div>
  <div class="c"><h2>❄️ Cold (Iceberg/Polaris)</h2><div class="big bl" id="co">—</div>
    <div class="row"><span class="lab">Status</span><span id="cs">—</span></div>
    <div class="row"><span class="lab">Catalog</span><span>Polaris REST</span></div>
    <svg class="spark" id="sparkC" width="100%" height="30" viewBox="0 0 200 30"></svg></div>
  <div class="c"><h2>🔗 Unified (DuckDB)</h2><div class="big pu" id="u">—</div>
    <div class="row"><span class="lab">Engine</span><span>DuckDB in-process</span></div>
    <div class="delta" id="delta">—</div>
    <div class="ratio-bar"><div class="ratio-fill" id="ratio" style="width:0%"></div></div></div>
</div>

<!-- Row 2: Analytics -->
<!-- Row 2: Regions in one frame -->
<div class="g2">
  <div class="c"><h2>🌍 Regions (unified)</h2>
    <div id="rg" style="display:grid;grid-template-columns:repeat(3,1fr);gap:8px"></div>
  </div>
  <div class="c"><h2>🚨 Fraud Alerts</h2><div id="fr"></div></div>
</div>

<!-- Row 3: Actions + Events -->
<div class="g2">
  <div class="c"><h2>👆 Actions (live)</h2><div id="ac"></div></div>
  <div class="c"><h2>📡 Live Events <span style="color:#22c55e;font-size:9px" id="evPulse">●</span></h2>
    <table id="ev"><tr><th>#</th><th>user</th><th>action</th><th>page</th><th>region</th></tr></table></div>
</div>

<!-- Row 4: Users + Devices -->
<div class="g2">
  <div class="c"><h2>👤 Top Users (hot)</h2><div id="us"></div></div>
  <div class="c"><h2>📱 Devices (unified)</h2><div id="dv"></div></div>
</div>

<script>
const hist={h:[],c:[]};
let prevH=0,prevC=0,prevEvts=[];

function F(n){return n!=null?n.toLocaleString():'—'}
function D(s){return'<span class="dot '+(s==='online'?'on':'off')+'"></span>'+s}
function B(p,c){return'<div class="bar" style="width:'+Math.min(p,100)+'%;background:'+c+'"></div>'}

function animateNum(el,to){
  const from=parseInt(el.dataset.val||'0')||0;
  if(from===to)return;
  el.dataset.val=to;
  el.classList.remove('count-anim');
  void el.offsetWidth;
  el.classList.add('count-anim');
  const dur=400,start=performance.now();
  function step(t){
    const p=Math.min((t-start)/dur,1);
    const ease=1-Math.pow(1-p,3);
    el.textContent=Math.round(from+(to-from)*ease).toLocaleString();
    if(p<1)requestAnimationFrame(step);
  }
  requestAnimationFrame(step);
}

function sparkline(svgId,data,color){
  const svg=document.getElementById(svgId);if(!svg)return;
  const pts=data.slice(-30);if(pts.length<2)return;
  const max=Math.max(...pts,1),min=Math.min(...pts,0);
  const range=max-min||1;
  const w=200,h=30;
  let path='M';
  pts.forEach((v,i)=>{
    const x=(i/(pts.length-1))*w;
    const y=h-((v-min)/range)*(h-4)-2;
    path+=(i?'L':'')+x.toFixed(1)+','+y.toFixed(1);
  });
  const area=path+` L${w},${h} L0,${h} Z`;
  svg.innerHTML=`<defs><linearGradient id="g${svgId}" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0%" stop-color="${color}" stop-opacity="0.3"/>
    <stop offset="100%" stop-color="${color}" stop-opacity="0.02"/>
    </linearGradient></defs>
    <path d="${area}" fill="url(#g${svgId})"/>
    <path d="${path}" fill="none" stroke="${color}" stroke-width="1.5" stroke-linecap="round"/>
    <circle cx="${w}" cy="${h-((pts[pts.length-1]-min)/range)*(h-4)-2}" r="2.5" fill="${color}"/>`;
}

async function R(){try{
  const d=(await(await fetch('/api/stats')).json());

  // Animated counters
  animateNum(document.getElementById('h'),d.hot_rows||0);
  animateNum(document.getElementById('co'),d.cold_rows||0);
  animateNum(document.getElementById('u'),d.unified_rows||0);

  // Status dots
  document.getElementById('hs').innerHTML=D(d.hot_status);
  document.getElementById('cs').innerHTML=D(d.cold_status);

  // Delta + ratio bar
  const delta=d.hot_rows-d.cold_rows;
  const ratio=d.hot_rows>0?Math.round(d.cold_rows*100/d.hot_rows):0;
  document.getElementById('delta').innerHTML=`Δ <b>${delta>0?'+':''}${F(delta)}</b> · Cold at ${ratio}%`;
  document.getElementById('ratio').style.width=ratio+'%';

  // Sparkline history
  hist.h.push(d.hot_rows||0);hist.c.push(d.cold_rows||0);
  sparkline('sparkH',hist.h,'#f97316');
  sparkline('sparkC',hist.c,'#3b82f6');

  // Regions — compact cards with flags
  const flags={'us-east':'🇺🇸','us-west':'🇺🇸','eu-west':'🇪🇺','eu-central':'🇪🇺','ap-south':'🇮🇳','ap-east':'🇯🇵'};
  const labels={'us-east':'US East','us-west':'US West','eu-west':'EU West','eu-central':'EU Central','ap-south':'Asia South','ap-east':'Asia East'};
  let s='';const M=Math.max(...(d.regions||[]).map(r=>r.events),1);
  (d.regions||[]).forEach(r=>{
    const pct=(r.events/M*100).toFixed(0);
    const flag=flags[r.region]||'🌍';
    const label=labels[r.region]||r.region;
    s+=`<div style="padding:8px;text-align:center;background:#151d2a;border-radius:8px">
      <div style="font-size:20px;margin-bottom:2px">${flag}</div>
      <div style="font-size:20px;font-weight:700;color:#a855f7">${F(r.events)}</div>
      <div style="font-size:10px;color:#9ca3af;margin-bottom:4px">${label}</div>
      <div class="ratio-bar" style="height:4px"><div class="ratio-fill" style="width:${pct}%;background:#a855f7"></div></div>
      <div style="font-size:9px;color:#6b7280;margin-top:3px">${F(r.users)} users</div>
    </div>`;
  });
  document.getElementById('rg').innerHTML=s||'';

  // Actions
  s='';const A=Math.max(...(d.actions||[]).map(a=>a.events),1);
  (d.actions||[]).forEach(a=>{s+=`<div class="row"><span>${a.action}</span><span>${F(a.events)}</span></div>`+B(a.events/A*100,'#f97316')});
  document.getElementById('ac').innerHTML=s||'<span class="lab">No data</span>';

  // Fraud alerts
  s='';if(!(d.fraud_alerts||[]).length)s='<div style="color:#22c55e;text-align:center;padding:16px">✅ No suspicious activity detected</div>';
  (d.fraud_alerts||[]).forEach(f=>{s+=`<div class="al"><div class="al-t">⚠️ ${f.user_id} — ${f.events} events in buffer</div><div class="row"><span class="lab">Sessions</span><span>${f.sessions}</span></div><div class="row"><span class="lab">Regions</span><span>${f.regions>3?'<span class="rd">'+f.regions+' ⚠️ multi-region</span>':f.regions}</span></div></div>`});
  document.getElementById('fr').innerHTML=s;

  // Live events with slide-in animation for new rows
  const newEvts=d.recent||[];
  const isNew=(e)=>!prevEvts.some(p=>p.offset===e.offset);
  s='<tr><th>#</th><th>user</th><th>action</th><th>page</th><th>region</th></tr>';
  newEvts.forEach(e=>{
    const cls=isNew(e)?'evt-new':'';
    s+=`<tr class="${cls}"><td>${e.offset||''}</td><td>${e.user||''}</td><td>${e.action||''}</td><td style="max-width:120px;overflow:hidden;text-overflow:ellipsis">${e.page||''}</td><td>${e.region||''}</td></tr>`;
  });
  document.getElementById('ev').innerHTML=s;
  prevEvts=newEvts;

  // Pulse indicator
  const pulse=document.getElementById('evPulse');
  pulse.style.opacity='1';setTimeout(()=>pulse.style.opacity='.3',500);

  // Top users
  s='';(d.top_users||[]).slice(0,8).forEach(u=>{s+=`<div class="row"><span>${u.user_id}</span><span>${u.events} events · ${u.sessions} sess</span></div>`});
  document.getElementById('us').innerHTML=s||'<span class="lab">No data</span>';

  // Devices
  s='';const X=Math.max(...(d.devices||[]).map(x=>x.events),1);
  const cl={desktop:'#3b82f6',mobile:'#22c55e',tablet:'#f97316'};
  (d.devices||[]).forEach(x=>{s+=`<div class="row"><span>${x.device}</span><span>${F(x.events)}</span></div>`+B(x.events/X*100,cl[x.device]||'#6b7280')});
  document.getElementById('dv').innerHTML=s||'<span class="lab">No data</span>';

  document.getElementById('r').textContent='Updated '+new Date().toLocaleTimeString()+' · 3s refresh · DuckDB unified';
}catch(e){document.getElementById('r').textContent='Error: '+e.message}}
R();setInterval(R,3000);
</script></body></html>"""


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path=="/api/stats":
            d=get_stats()
            self.send_response(200);self.send_header("Content-Type","application/json");self.send_header("Access-Control-Allow-Origin","*");self.end_headers()
            self.wfile.write(json.dumps(d).encode())
        else:
            self.send_response(200);self.send_header("Content-Type","text/html");self.end_headers()
            self.wfile.write(HTML.encode())
    def log_message(self,*a):pass

if __name__=="__main__":
    print(f"⚡ DataShuttle Fraud Detection Dashboard — http://localhost:{PORT}")
    print(f"   Flight: {FLIGHT_URI}  ·  Iceberg: {S3_DATA}  ·  Engine: DuckDB")
    HTTPServer(("0.0.0.0",PORT),H).serve_forever()
