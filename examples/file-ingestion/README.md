# File Ingestion Demo

Demonstrates DataShuttle ingesting CSV, JSON Lines, and Parquet files from S3 (MinIO).

## Data

| File | Format | Rows | Description |
|------|--------|------|-------------|
| `customers.csv` | CSV | 1,000 | Web analytics events |
| `transactions.json` | JSON Lines | 500 | API request/response logs with nested objects |
| `events.parquet` | Parquet | 2,000 | Financial transactions with decimal, timestamp, nested struct |

## Steps

```bash
# 1. Start infrastructure
docker compose -f examples/docker-compose.yml up -d

# 2. Generate data files
python3 examples/file-ingestion/generate-data.py

# 3. Upload to MinIO
mc alias set local http://localhost:9000 minioadmin minioadmin
mc cp examples/file-ingestion/data/customers.csv local/file-ingestion/csv/
mc cp examples/file-ingestion/data/transactions.json local/file-ingestion/json/
mc cp examples/file-ingestion/data/events.parquet local/file-ingestion/parquet/

# 4. Start DataShuttle and create shuttles
./target/release/datashuttle start
./target/release/datashuttle sql -f examples/file-ingestion/shuttle-csv.sql
./target/release/datashuttle sql -f examples/file-ingestion/shuttle-json.sql
./target/release/datashuttle sql -f examples/file-ingestion/shuttle-parquet.sql

# 5. Verify
bash examples/file-ingestion/verify.sh
```

## Generating the Parquet file

The Parquet file requires `pyarrow`:

```bash
pip install pyarrow
python3 examples/file-ingestion/generate-data.py
```

If `pyarrow` is not installed, only CSV and JSON files are generated.
