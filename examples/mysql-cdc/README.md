# MySQL CDC Demo

Demonstrates DataShuttle ingesting IoT telemetry data from MySQL via binlog replication.

## Data

| Table | Rows | Description |
|-------|------|-------------|
| `devices` | 50 | IoT sensors: type, location, firmware, status |
| `readings` | 10,000 | Temperature, humidity, pressure, CO2, motion |
| `alerts` | 200 | Severity levels, acknowledgement tracking |
| `device_configs` | 150 | Key-value configuration per device |

## Steps

```bash
# 1. Start infrastructure
docker compose -f examples/docker-compose.yml up -d

# 2. Start DataShuttle
./target/release/datashuttle start

# 3. Create shuttle
./target/release/datashuttle sql -f examples/mysql-cdc/shuttle.sql

# 4. Generate CDC changes
mysql -h localhost -u datashuttle -pdatashuttle iot < examples/mysql-cdc/generate-changes.sql

# 5. Verify
bash examples/mysql-cdc/verify.sh

# 6. Cleanup
./target/release/datashuttle sql -e "DROP SHUTTLE iot_cdc"
```
