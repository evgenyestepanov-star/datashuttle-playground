#!/usr/bin/env python3
"""
DataShuttle Realtime Demo Dashboard.

Queries both hot path (Arrow Flight) and cold path (Trino/Iceberg)
to show a unified real-time analytics view.

Requirements:
    pip install pyarrow trino rich

Usage:
    python3 examples/realtime-demo/dashboard.py
"""

import sys
import time
from datetime import datetime, timezone

try:
    import pyarrow.flight as flight
    from rich.console import Console
    from rich.table import Table
    from rich.live import Live
    from rich.panel import Panel
    from rich.layout import Layout
    from rich.text import Text
    import trino as trino_mod
except ImportError:
    print("Install dependencies: pip install pyarrow trino rich")
    sys.exit(1)

FLIGHT_HOST = "localhost"
FLIGHT_PORT = 8815
TRINO_HOST = "localhost"
TRINO_PORT = 8090
DS_API = "http://localhost:8080"

console = Console()


def query_flight(table_name: str) -> list[dict]:
    """Query DataShuttle Arrow Flight hot buffer — sub-second latency."""
    try:
        client = flight.FlightClient(f"grpc://{FLIGHT_HOST}:{FLIGHT_PORT}")
        ticket = flight.Ticket(table_name.encode())
        reader = client.do_get(ticket)
        table = reader.read_all()
        return table.to_pylist()
    except Exception as e:
        return [{"error": str(e)}]


def query_trino(sql: str) -> list[dict]:
    """Query Trino for Iceberg historical data."""
    try:
        conn = trino_mod.dbapi.connect(
            host=TRINO_HOST,
            port=TRINO_PORT,
            user="datashuttle",
            catalog="iceberg",
            schema="default",
        )
        cursor = conn.cursor()
        cursor.execute(sql)
        columns = [desc[0] for desc in cursor.description]
        rows = cursor.fetchall()
        return [dict(zip(columns, row)) for row in rows]
    except Exception as e:
        return [{"error": str(e)}]


def flight_stats(table_name: str) -> dict:
    """Get stats from Flight hot buffer."""
    rows = query_flight(table_name)
    if rows and "error" in rows[0]:
        return {"status": "offline", "error": rows[0]["error"], "rows": 0}
    return {
        "status": "online",
        "rows": len(rows),
        "latest": rows[-1] if rows else None,
    }


def build_dashboard() -> Layout:
    """Build the dashboard layout."""
    layout = Layout()
    layout.split_column(
        Layout(name="header", size=3),
        Layout(name="body"),
        Layout(name="footer", size=3),
    )
    layout["body"].split_row(
        Layout(name="hot", ratio=1),
        Layout(name="cold", ratio=1),
    )
    return layout


def render_hot_panel(stats: dict) -> Panel:
    """Render Arrow Flight hot buffer stats."""
    table = Table(title="🔥 Hot Path — Arrow Flight Buffer", show_header=True)
    table.add_column("Metric", style="cyan")
    table.add_column("Value", style="green")

    table.add_row("Status", stats.get("status", "unknown"))
    table.add_row("Buffer Rows", f"{stats.get('rows', 0):,}")
    if stats.get("latest"):
        latest = stats["latest"]
        for k, v in list(latest.items())[:5]:
            table.add_row(f"  latest.{k}", str(v)[:50])
    if stats.get("error"):
        table.add_row("Error", Text(str(stats["error"])[:80], style="red"))

    return Panel(table, border_style="red")


def render_cold_panel(iceberg_stats: list[dict], trino_agg: list[dict]) -> Panel:
    """Render Trino/Iceberg cold path stats."""
    table = Table(title="❄️  Cold Path — Trino → Iceberg", show_header=True)
    table.add_column("Metric", style="cyan")
    table.add_column("Value", style="blue")

    if iceberg_stats and "error" not in iceberg_stats[0]:
        for row in iceberg_stats[:5]:
            for k, v in row.items():
                table.add_row(k, str(v))
    else:
        err = iceberg_stats[0].get("error", "no data") if iceberg_stats else "no data"
        table.add_row("Status", Text(str(err)[:80], style="yellow"))

    if trino_agg and "error" not in trino_agg[0]:
        table.add_row("---", "--- Aggregations ---")
        for row in trino_agg[:5]:
            for k, v in row.items():
                table.add_row(k, str(v))

    return Panel(table, border_style="blue")


def main():
    console.print("[bold green]DataShuttle Realtime Demo Dashboard[/]")
    console.print(f"  Flight: grpc://{FLIGHT_HOST}:{FLIGHT_PORT}")
    console.print(f"  Trino:  {TRINO_HOST}:{TRINO_PORT}")
    console.print()

    layout = build_dashboard()

    with Live(layout, refresh_per_second=1, console=console) as live:
        while True:
            now = datetime.now(timezone.utc).strftime("%H:%M:%S")

            # Header
            layout["header"].update(
                Panel(f"[bold]DataShuttle Realtime Analytics[/]  |  {now}  |  Ctrl+C to exit")
            )

            # Hot path: Flight buffer
            hot = flight_stats("clickstream")
            layout["hot"].update(render_hot_panel(hot))

            # Cold path: Trino → Iceberg
            iceberg_stats = query_trino(
                "SELECT count(*) as total_rows FROM iceberg.bench.clickstream"
            )
            trino_agg = query_trino("""
                SELECT region, count(*) as events, 
                       round(avg(duration_ms), 0) as avg_duration_ms
                FROM iceberg.bench.clickstream 
                GROUP BY region 
                ORDER BY events DESC 
                LIMIT 5
            """)
            layout["cold"].update(render_cold_panel(iceberg_stats, trino_agg))

            # Footer
            layout["footer"].update(
                Panel(
                    f"[dim]Hot: {hot.get('rows', 0)} buffered rows (sub-ms) | "
                    f"Cold: {iceberg_stats[0].get('total_rows', '?') if iceberg_stats and 'error' not in iceberg_stats[0] else 'N/A'} Iceberg rows[/]"
                )
            )

            time.sleep(2)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        console.print("\n[yellow]Dashboard stopped.[/]")
