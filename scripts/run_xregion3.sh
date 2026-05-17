#!/usr/bin/env bash
# 3-way cross-region p99 benchmark.
# Server in eastus2; clients in eastus2 (LOCAL), northcentralus (CENTRAL), mexicocentral (MEXICO).
#
# Requires three env vars (or sourced from /tmp/vm_ips.sh):
#   SERVER_IP, CENTRAL_IP, MEXICO_IP
#
# Output: appends rows to results/xregion3_verified.csv
# Reproducible methodology: --slow-count 0 is passed EXPLICITLY to every loadtest
# invocation so the CSV alone is auditable.

set -uo pipefail

# Allow the caller to set IPs via env, or load them from a sidecar file.
if [ -f /tmp/vm_ips.sh ]; then
    # shellcheck disable=SC1091
    source /tmp/vm_ips.sh
fi
: "${SERVER_IP:?SERVER_IP required}"
: "${CENTRAL_IP:?CENTRAL_IP required}"
: "${MEXICO_IP:?MEXICO_IP required}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS="$ROOT/results"
COMBINED="$RESULTS/xregion3_verified.csv"
mkdir -p "$RESULTS"

# Fresh CSV header if file doesn't exist.
if [ ! -f "$COMBINED" ]; then
    echo "label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us" > "$COMBINED"
fi

SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

run_one() {
    local variant="$1"   # mutex | broadcast
    local n="$2"         # clients per region
    local rate="$3"      # msgs/sec/client
    local duration="${4:-20}"
    local warmup=3

    echo ">>> [$(date +%H:%M:%S)] ${variant}  N=${n}/region  R=${rate}/s"

    # 1. Stop any running server, then spawn the variant we want.
    # NOTE: pkill -x matches process name exactly (not cmdline), so it won't
    # match the parent bash whose cmdline contains "server_mutex" etc.
    ssh $SSH_OPTS matthewkim@"$SERVER_IP" "
        pkill -9 -x server_mutex 2>/dev/null
        pkill -9 -x server_broadcast 2>/dev/null
        sleep 0.5
        nohup ./tokio-chat/target/release/server_${variant} 0.0.0.0:8080 \
            > /tmp/server_${variant}.log 2>&1 < /dev/null &
        disown
        sleep 1
        pgrep -x server_${variant} > /tmp/server_pid.txt || true
    " > /dev/null

    sleep 1

    # 2. Verify server is listening.
    local listening
    listening=$(ssh $SSH_OPTS matthewkim@"$SERVER_IP" "ss -ln | grep -c ':8080 '" 2>/dev/null || echo 0)
    if [ "$listening" -lt 1 ]; then
        echo "!!! server not listening on 8080, skipping ${variant} N=${n} R=${rate}" >&2
        return 1
    fi

    # 3. Launch all three loadtest instances in parallel.
    #    --slow-count 0 is passed EXPLICITLY so the methodology is auditable from this script alone.
    local TMP="/tmp/xreg3_${variant}_n${n}_r${rate}"
    mkdir -p "$TMP"

    ssh $SSH_OPTS matthewkim@"$SERVER_IP" \
        "./tokio-chat/target/release/loadtest \
            --addr 127.0.0.1:8080 \
            --clients $n \
            --duration-secs $duration \
            --warmup-secs $warmup \
            --rate-per-client $rate \
            --slow-count 0 \
            --label '${variant}-LOCAL,N=${n},R=${rate}'" > "$TMP/local.csv" 2>/dev/null &
    local PID_LOCAL=$!

    ssh $SSH_OPTS matthewkim@"$CENTRAL_IP" \
        "./tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients $n \
            --duration-secs $duration \
            --warmup-secs $warmup \
            --rate-per-client $rate \
            --slow-count 0 \
            --label '${variant}-CENTRAL,N=${n},R=${rate}'" > "$TMP/central.csv" 2>/dev/null &
    local PID_CENTRAL=$!

    ssh $SSH_OPTS matthewkim@"$MEXICO_IP" \
        "./tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients $n \
            --duration-secs $duration \
            --warmup-secs $warmup \
            --rate-per-client $rate \
            --slow-count 0 \
            --label '${variant}-MEXICO,N=${n},R=${rate}'" > "$TMP/mexico.csv" 2>/dev/null &
    local PID_MEXICO=$!

    wait $PID_LOCAL   $PID_CENTRAL   $PID_MEXICO || true

    # 4. Stop the server cleanly.
    ssh $SSH_OPTS matthewkim@"$SERVER_IP" "pkill -9 -x server_${variant} 2>/dev/null; true" > /dev/null

    # 5. Append last (=only) line of each CSV to the combined file.
    tail -1 "$TMP/local.csv"   >> "$COMBINED"
    tail -1 "$TMP/central.csv" >> "$COMBINED"
    tail -1 "$TMP/mexico.csv"  >> "$COMBINED"

    echo "    appended 3 rows to $COMBINED"
}

# === Sweep ===
# 3 (N, R) conditions × 2 variants = 6 server runs, 18 CSV rows total.
for variant in mutex broadcast; do
    run_one "$variant" 50  1 20
    run_one "$variant" 50  5 20
    run_one "$variant" 100 5 20
done

echo
echo "=== combined CSV ==="
column -t -s, "$COMBINED" | head -25
