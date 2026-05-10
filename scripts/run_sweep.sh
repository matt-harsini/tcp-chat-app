#!/usr/bin/env bash
set -euo pipefail

# Sweep N=[10,25,50,100,200,400] for both server variants.
# Each run: start server on a clean port, drive load, kill server.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS="$ROOT/results"
mkdir -p "$RESULTS"

NS=(10 25 50 100 200 400)
DURATION=15
WARMUP=3
RATE=0  # 0 = saturation
PORT=9099

CSV="$RESULTS/sweep_$(date +%Y%m%d_%H%M%S).csv"
echo "label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us" > "$CSV"

run_one() {
    local variant="$1"   # mutex | broadcast
    local n="$2"
    local bin="$ROOT/target/release/server_${variant}"

    echo ">>> ${variant} N=${n}" >&2

    # Start server.
    "$bin" "127.0.0.1:${PORT}" > "/tmp/server_${variant}_${n}.log" 2>&1 &
    local server_pid=$!
    # Brief sleep for the listen socket to come up.
    sleep 0.4

    # Drive load.
    "$ROOT/target/release/loadtest" \
        --addr "127.0.0.1:${PORT}" \
        --clients "$n" \
        --duration-secs "$DURATION" \
        --warmup-secs "$WARMUP" \
        --rate-per-client "$RATE" \
        --label "${variant},N=${n}" >> "$CSV"

    # Tear down server cleanly.
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
    sleep 0.5
}

for n in "${NS[@]}"; do
    run_one mutex "$n"
done

for n in "${NS[@]}"; do
    run_one broadcast "$n"
done

echo "" >&2
echo "Wrote: $CSV" >&2
echo "" >&2
column -t -s, "$CSV"
