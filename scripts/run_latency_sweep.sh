#!/usr/bin/env bash
# Latency sweep at sub-saturation rate so p99 reflects fan-out cost,
# not open-loop queue growth.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS="$ROOT/results"
mkdir -p "$RESULTS"

NS=(10 25 50 100 200 400)
DURATION=20
WARMUP=3
RATE=0.1   # 0.1 msg/sec/client — well under saturation for both variants at all N
PORT=9099

CSV="$RESULTS/latency_$(date +%Y%m%d_%H%M%S).csv"
echo "label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us" > "$CSV"

run_one() {
    local variant="$1"; local n="$2"
    local bin="$ROOT/target/release/server_${variant}"
    echo ">>> ${variant} N=${n}" >&2
    "$bin" "127.0.0.1:${PORT}" > "/tmp/latserver_${variant}_${n}.log" 2>&1 &
    local pid=$!; sleep 0.4
    "$ROOT/target/release/loadtest" \
        --addr "127.0.0.1:${PORT}" \
        --clients "$n" \
        --duration-secs "$DURATION" \
        --warmup-secs "$WARMUP" \
        --rate-per-client "$RATE" \
        --label "${variant},N=${n}" >> "$CSV"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    sleep 0.4
}

for n in "${NS[@]}"; do run_one mutex "$n"; done
for n in "${NS[@]}"; do run_one broadcast "$n"; done

echo ""; echo "Wrote: $CSV" >&2
column -t -s, "$CSV"
