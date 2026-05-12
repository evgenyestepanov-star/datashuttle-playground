#!/usr/bin/env python3
"""Generate demo files for DataShuttle file ingestion demo.

Produces:
  - data/customers.csv      (1000 rows)
  - data/transactions.json   (500 JSON Lines records)
  - data/events.parquet      (2000 rows with varied types)

Requires: pip install pyarrow
"""

import csv
import json
import os
import random
import datetime

OUTPUT_DIR = os.path.join(os.path.dirname(__file__), "data")
os.makedirs(OUTPUT_DIR, exist_ok=True)

# ── Helpers ──────────────────────────────────────────

FIRST_NAMES = ["Emma","Liam","Olivia","Noah","Ava","Oliver","Sophia","Elijah",
    "Isabella","Lucas","Mia","Mason","Charlotte","Logan","Amelia","Ethan"]
LAST_NAMES = ["Smith","Johnson","Williams","Brown","Jones","Garcia","Miller",
    "Davis","Rodriguez","Martinez","Anderson","Taylor","Thomas","Moore","Jackson"]
CITIES = ["New York","Los Angeles","Chicago","Houston","Phoenix","Philadelphia",
    "San Antonio","San Diego","Dallas","Austin","Seattle","Denver","Boston"]
EVENT_TYPES = ["page_view","click","purchase","signup","logout","search","add_to_cart",
    "remove_from_cart","checkout_start","checkout_complete"]
METHODS = ["GET","POST","PUT","DELETE","PATCH"]
ENDPOINTS = ["/api/users","/api/orders","/api/products","/api/search",
    "/api/auth/login","/api/auth/refresh","/api/cart","/api/checkout"]
STATUSES = [200,200,200,200,201,204,400,401,403,404,500]

def rand_ts(days_back=90):
    return datetime.datetime.now() - datetime.timedelta(
        seconds=random.randint(0, days_back * 86400))

# ── CSV: customer_events (1000 rows) ─────────────────

print("Generating data/customers.csv (1000 rows)...")
with open(os.path.join(OUTPUT_DIR, "customers.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["timestamp","user_id","event_type","page","session_id","device","country","duration_ms"])
    for i in range(1000):
        w.writerow([
            rand_ts(30).isoformat() + "Z",
            f"usr_{random.randint(1,200):04d}",
            random.choice(EVENT_TYPES),
            f"/page/{random.choice(['home','products','cart','checkout','profile','settings'])}",
            f"sess_{random.randint(10000,99999)}",
            random.choice(["desktop","mobile","tablet"]),
            random.choice(["US","CA","GB","DE","FR","JP","AU","BR"]),
            random.randint(50, 30000),
        ])

# ── JSON Lines: api_logs (500 records) ────────────────

print("Generating data/transactions.json (500 JSON Lines)...")
with open(os.path.join(OUTPUT_DIR, "transactions.json"), "w") as f:
    for i in range(500):
        status = random.choice(STATUSES)
        record = {
            "request_id": f"req-{i:06d}",
            "timestamp": rand_ts(14).isoformat() + "Z",
            "method": random.choice(METHODS),
            "endpoint": random.choice(ENDPOINTS),
            "status_code": status,
            "latency_ms": random.randint(5, 2000) if status < 500 else random.randint(5000, 30000),
            "request": {
                "headers": {
                    "content_type": "application/json",
                    "user_agent": random.choice(["Mozilla/5.0","curl/8.0","PostmanRuntime/7.0","DataShuttle/0.1"]),
                    "x_request_id": f"trace-{random.randint(100000,999999)}",
                },
                "body_size_bytes": random.randint(0, 10000),
            },
            "response": {
                "body_size_bytes": random.randint(50, 50000),
                "cache_hit": random.random() > 0.7,
            },
            "user_id": f"usr_{random.randint(1,200):04d}" if random.random() > 0.3 else None,
            "ip_address": f"{random.randint(10,192)}.{random.randint(0,255)}.{random.randint(0,255)}.{random.randint(1,254)}",
        }
        f.write(json.dumps(record) + "\n")

# ── Parquet: financial_transactions (2000 rows) ──────

print("Generating data/events.parquet (2000 rows)...")
try:
    import pyarrow as pa
    import pyarrow.parquet as pq

    n = 2000
    schema = pa.schema([
        ("transaction_id", pa.string()),
        ("timestamp", pa.timestamp("us")),
        ("account_from", pa.string()),
        ("account_to", pa.string()),
        ("amount", pa.decimal128(12, 2)),
        ("currency", pa.string()),
        ("category", pa.string()),
        ("status", pa.string()),
        ("metadata", pa.struct([
            ("channel", pa.string()),
            ("device_id", pa.string()),
            ("ip_address", pa.string()),
        ])),
    ])

    categories = ["transfer","payment","refund","fee","interest","deposit","withdrawal"]
    statuses = ["completed","pending","failed","reversed"]
    channels = ["web","mobile","api","atm","branch"]

    data = {
        "transaction_id": [f"txn-{i:07d}" for i in range(n)],
        "timestamp": [rand_ts(60) for _ in range(n)],
        "account_from": [f"ACC-{random.randint(10000,99999)}" for _ in range(n)],
        "account_to": [f"ACC-{random.randint(10000,99999)}" for _ in range(n)],
        "amount": [random.randint(100, 1000000) for _ in range(n)],  # cents
        "currency": [random.choice(["USD","EUR","GBP","JPY","CHF"]) for _ in range(n)],
        "category": [random.choice(categories) for _ in range(n)],
        "status": [random.choice(statuses) for _ in range(n)],
    }

    metadata = [
        {"channel": random.choice(channels),
         "device_id": f"dev-{random.randint(1000,9999)}",
         "ip_address": f"{random.randint(10,192)}.{random.randint(0,255)}.{random.randint(0,255)}.{random.randint(1,254)}"}
        for _ in range(n)
    ]

    table = pa.table({
        "transaction_id": pa.array(data["transaction_id"]),
        "timestamp": pa.array(data["timestamp"], type=pa.timestamp("us")),
        "account_from": pa.array(data["account_from"]),
        "account_to": pa.array(data["account_to"]),
        "amount": pa.array(data["amount"], type=pa.decimal128(12, 2)),
        "currency": pa.array(data["currency"]),
        "category": pa.array(data["category"]),
        "status": pa.array(data["status"]),
        "metadata": pa.array(metadata, type=pa.struct([
            ("channel", pa.string()),
            ("device_id", pa.string()),
            ("ip_address", pa.string()),
        ])),
    })
    pq.write_table(table, os.path.join(OUTPUT_DIR, "events.parquet"))
    print(f"  events.parquet: {n} rows, {os.path.getsize(os.path.join(OUTPUT_DIR, 'events.parquet'))} bytes")

except ImportError:
    print("  SKIPPED: pyarrow not installed (pip install pyarrow)")
    print("  The CSV and JSON files are still generated.")

print("\nDone! Files in examples/file-ingestion/data/")
