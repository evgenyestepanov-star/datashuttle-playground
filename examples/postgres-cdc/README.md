# PostgreSQL CDC Demo

Demonstrates DataShuttle ingesting a full e-commerce database from PostgreSQL via logical replication (CDC).

## What's included

| Table | Rows | Description |
|-------|------|-------------|
| `customers` | 500 | Names, emails, addresses, segments |
| `products` | 100 | SKUs, prices, categories, weights |
| `orders` | 2,000 | Status lifecycle, totals, dates |
| `order_items` | 5,000 | Line items with quantities and discounts |
| `payments` | 2,000 | Payment methods, statuses, processing times |

## Prerequisites

- Docker and Docker Compose
- DataShuttle binary (`cargo build --release`)

## Steps

### 1. Start infrastructure

```bash
docker compose -f examples/docker-compose.yml up -d
```

Wait for PostgreSQL to be healthy:

```bash
docker compose -f examples/docker-compose.yml ps
```

### 2. Start DataShuttle

```bash
./target/release/datashuttle start
```

### 3. Create the shuttle

```bash
./target/release/datashuttle sql -f examples/postgres-cdc/shuttle.sql
```

This creates a connection to the `ecommerce` database and a CDC shuttle for all 5 tables.

### 4. Watch the initial snapshot

```bash
./target/release/datashuttle shuttle status ecommerce_cdc
```

Expected: state = `SNAPSHOTTING`, then transitions to `RUNNING`.

### 5. Generate changes

While the shuttle is running, apply CDC changes:

```bash
PGPASSWORD=postgres psql -h localhost -U postgres -d ecommerce \
  -f examples/postgres-cdc/generate-changes.sql
```

This inserts new customers and orders, updates statuses, cancels old orders, adjusts prices, and deactivates products.

### 6. Verify

```bash
bash examples/postgres-cdc/verify.sh
```

Or check the Web UI at http://localhost:8080.

### 7. Cleanup

```bash
./target/release/datashuttle sql -e "DROP SHUTTLE ecommerce_cdc"
docker compose -f examples/docker-compose.yml down -v
```
