#!/usr/bin/env bash
# Slow-consumer pathology sweep.
# Setup: 1 deliberately slow client (just connects, never reads — TCP recv buffer
# fills, server's writes to it block) + (N-1) active clients sending and receiving.
# Measure p99 latency of the ACTIVE clients only.
#
# Predicted result: mutex variant's p99 explodes with N because the lock is held
# during the (slow) write to the misbehaving client; broadcast variant's p99 is
# unaffected because writers are decoupled per receiver task.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS="$ROOT/results"
mkdir -p "$RESULTS"

# N here = TOTAL clients (including the 1 slow one). Active clients = N - 1.
NS=(10 25 50 100 200 400)
DURATION=15
WARMUP=3
RATE=2   # 2 msgs/sec/client; modest load so latency = fan-out cost, not queue depth
SLOW_K=1
PORT=9099

CSV="$RESULTS/slow_$(date +%Y%m%d_%H%M%S).csv"
echo "label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us" > "$CSV"

run_one() {
    local variant="$1"; local n="$2"
    local bin="$ROOT/target/release/server_${variant}"
    echo ">>> ${variant} N=${n} (1 slow + $((n-1)) active)" >&2
    "$bin" "127.0.0.1:${PORT}" > "/tmp/slowsrv_${variant}_${n}.log" 2>&1 &
    local pid=$!; sleep 0.4
    "$ROOT/target/release/loadtest" \
        --addr "127.0.0.1:${PORT}" \
        --clients "$n" \
        --duration-secs "$DURATION" \
        --warmup-secs "$WARMUP" \
        --rate-per-client "$RATE" \
        --slow-count "$SLOW_K" \
        --label "${variant},N=${n}" >> "$CSV"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    sleep 0.4
}

for n in "${NS[@]}"; do run_one mutex "$n"; done
for n in "${NS[@]}"; do run_one broadcast "$n"; done

echo ""; echo "Wrote: $CSV" >&2
column -t -s, "$CSV"
