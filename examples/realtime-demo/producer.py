#!/usr/bin/env python3
"""
Realtime event producer for DataShuttle demo.

Generates clickstream events and sends them to Redpanda/Kafka
via the Panda Proxy REST API (no librdkafka needed).

Usage:
    pip install requests
    python3 examples/realtime-demo/producer.py

Events:
    - clickstream: page views, clicks, searches with user_id + session
    - transactions: purchases with amount, currency, status
"""

import json
import random
import time
import sys
from datetime import datetime, timezone

try:
    import requests
except ImportError:
    print("pip install requests")
    sys.exit(1)

PROXY_URL = "http://localhost:18082"
CLICKSTREAM_TOPIC = "clickstream"
TRANSACTIONS_TOPIC = "transactions"

PAGES = ["/home", "/products", "/cart", "/checkout", "/account", "/search", "/deals", "/category/electronics", "/category/clothing", "/product/detail"]
ACTIONS = ["page_view", "click", "scroll", "search", "add_to_cart", "remove_from_cart"]
DEVICES = ["desktop", "mobile", "tablet"]
REGIONS = ["us-east", "us-west", "eu-west", "eu-central", "ap-south", "ap-east"]
CURRENCIES = ["USD", "EUR", "GBP", "JPY"]
STATUSES = ["completed", "pending", "failed", "refunded"]

def produce_message(topic: str, key: str, value: dict):
    """Send a message via Panda Proxy REST API."""
    url = f"{PROXY_URL}/topics/{topic}"
    payload = {
        "records": [
            {
                "key": key,
                "value": json.dumps(value),
                "partition": hash(key) % 3,
            }
        ]
    }
    headers = {"Content-Type": "application/vnd.kafka.json.v2+json"}
    try:
        resp = requests.post(url, json=payload, headers=headers, timeout=5)
        resp.raise_for_status()
    except Exception as e:
        print(f"  produce error: {e}", file=sys.stderr)

def gen_clickstream_event() -> tuple[str, dict]:
    user_id = f"user_{random.randint(1, 10000)}"
    event = {
        "event_id": f"evt_{random.randint(100000, 999999)}",
        "user_id": user_id,
        "session_id": f"sess_{random.randint(1000, 9999)}",
        "page": random.choice(PAGES),
        "action": random.choice(ACTIONS),
        "device": random.choice(DEVICES),
        "region": random.choice(REGIONS),
        "referrer": random.choice(["google", "direct", "social", "email", "ads"]),
        "duration_ms": random.randint(100, 30000),
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    return user_id, event

def gen_transaction_event() -> tuple[str, dict]:
    tx_id = f"tx_{random.randint(100000, 999999)}"
    event = {
        "transaction_id": tx_id,
        "user_id": f"user_{random.randint(1, 10000)}",
        "amount": round(random.uniform(1.0, 999.99), 2),
        "currency": random.choice(CURRENCIES),
        "status": random.choice(STATUSES),
        "product_count": random.randint(1, 10),
        "region": random.choice(REGIONS),
        "payment_method": random.choice(["credit_card", "debit_card", "paypal", "apple_pay"]),
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    return tx_id, event

def main():
    rate = int(sys.argv[1]) if len(sys.argv) > 1 else 100  # events/sec
    print(f"🚀 DataShuttle Realtime Demo Producer")
    print(f"   Rate: {rate} events/sec")
    print(f"   Topics: {CLICKSTREAM_TOPIC}, {TRANSACTIONS_TOPIC}")
    print(f"   Proxy: {PROXY_URL}")
    print()

    count = 0
    start = time.time()
    try:
        while True:
            batch_start = time.time()
            for _ in range(rate):
                # 80% clickstream, 20% transactions
                if random.random() < 0.8:
                    key, event = gen_clickstream_event()
                    produce_message(CLICKSTREAM_TOPIC, key, event)
                else:
                    key, event = gen_transaction_event()
                    produce_message(TRANSACTIONS_TOPIC, key, event)
                count += 1

            elapsed = time.time() - start
            eps = count / elapsed if elapsed > 0 else 0
            print(f"\r  📊 {count:,} events sent | {eps:,.0f} events/sec", end="", flush=True)

            # Sleep to maintain target rate
            batch_elapsed = time.time() - batch_start
            sleep_time = max(0, 1.0 - batch_elapsed)
            if sleep_time > 0:
                time.sleep(sleep_time)

    except KeyboardInterrupt:
        elapsed = time.time() - start
        print(f"\n\n✅ Stopped. {count:,} events in {elapsed:.1f}s ({count/elapsed:,.0f} eps)")

if __name__ == "__main__":
    main()
