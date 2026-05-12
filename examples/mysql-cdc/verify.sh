#!/usr/bin/env bash
set -euo pipefail
echo "=== MySQL CDC Demo Verification ==="
echo ""
echo "Source row counts:"
mysql -h localhost -u datashuttle -pdatashuttle iot -e "
  SELECT 'devices' AS tbl, COUNT(*) AS cnt FROM devices
  UNION ALL SELECT 'readings', COUNT(*) FROM readings
  UNION ALL SELECT 'alerts', COUNT(*) FROM alerts
  UNION ALL SELECT 'device_configs', COUNT(*) FROM device_configs;
" 2>/dev/null || echo "  (MySQL not reachable)"
echo ""
echo "Shuttle status:"
curl -s http://localhost:8080/api/v1/shuttles/iot_cdc/status 2>/dev/null | python3 -m json.tool || echo "  (API not reachable)"
echo ""
echo "=== Done ==="
