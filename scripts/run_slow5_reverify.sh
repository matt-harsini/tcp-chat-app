#!/usr/bin/env bash
# Re-verify last night's slow-count=5 240s run, with today's 4-VM topology.
# - SERVER: client-mex (mexicocentral) running server_mutex / server_broadcast
# - SLOW:   slow-ncus (NCUS) running loadtest --clients 5 --slow-count 5
# - ACTIVE-NCUS:   client-ncus (NCUS) running loadtest --clients 100 --slow-count 0
# - ACTIVE-LOCAL:  client-mex2 (MEX, co-located with server) running loadtest --clients 100 --slow-count 0
#
# 240s duration, R=5/sec/client, no tc netem.
# Output: /tmp/reverify/<variant>/{slow,ncus,local}.csv per run; combined to xregion3_slow5_240s_reverify.csv

set -uo pipefail
source /tmp/vm_ips.sh
: "${SERVER_IP:?}"; : "${SLOW_IP:?}"; : "${NCUS_IP:?}"; : "${LOCAL_IP:?}"

SSH_OPTS=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=20)

ROOT=/Users/matthewkim/Developer/tokio-chat
COMBINED="$ROOT/results/xregion3_slow5_240s_reverify.csv"
echo "label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us" > "$COMBINED"

DURATION=240
WARMUP=3
RATE=5
N_ACTIVE=100
N_SLOW=5

run_one() {
    local variant="$1"

    echo ""
    echo "######## $(date +%H:%M:%S)  variant=${variant}  N_active=${N_ACTIVE}  R=${RATE}  slow=${N_SLOW}  duration=${DURATION}s ########"

    # 1. Kill any stale servers; spawn the variant we want.
    ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "
        pkill -9 -x server_mutex 2>/dev/null
        pkill -9 -x server_broadcast 2>/dev/null
        sleep 0.5
        nohup ~/tokio-chat/target/release/server_${variant} 0.0.0.0:8080 \
            > /tmp/server_${variant}.log 2>&1 < /dev/null &
        disown
        sleep 1.5
    " > /dev/null

    # 2. Verify server is listening.
    local listening
    listening=$(ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "ss -ln | grep -c ':8080 '" 2>/dev/null || echo 0)
    if [ "$listening" -lt 1 ]; then
        echo "!!! server not listening on 8080, skipping ${variant}" >&2
        return 1
    fi

    local TMP="/tmp/reverify/${variant}"
    rm -rf "$TMP"
    mkdir -p "$TMP"

    # 3. Launch loadtests in parallel.
    #    Slow VM: 5 slow clients, --slow-count 5.
    #    Active VMs (NCUS, LOCAL): 100 active clients each, --slow-count 0.
    echo "  starting loadtests (will run ~$((WARMUP + DURATION + 10))s)..."

    ssh "${SSH_OPTS[@]}" matthewkim@"$SLOW_IP" \
        "~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_SLOW} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count ${N_SLOW} \
            --label '${variant}-SLOW,N=${N_SLOW},R=${RATE}'" > "$TMP/slow.csv" 2>/dev/null &
    P_SLOW=$!

    ssh "${SSH_OPTS[@]}" matthewkim@"$NCUS_IP" \
        "~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_ACTIVE} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count 0 \
            --label '${variant}-NCUS,N=${N_ACTIVE},R=${RATE}'" > "$TMP/ncus.csv" 2>/dev/null &
    P_NCUS=$!

    ssh "${SSH_OPTS[@]}" matthewkim@"$LOCAL_IP" \
        "~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_ACTIVE} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count 0 \
            --label '${variant}-LOCAL,N=${N_ACTIVE},R=${RATE}'" > "$TMP/local.csv" 2>/dev/null &
    P_LOCAL=$!

    wait $P_SLOW $P_NCUS $P_LOCAL || true

    # 4. Stop server cleanly.
    ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "pkill -9 -x server_${variant} 2>/dev/null; true" > /dev/null

    # 5. Append CSVs (each loadtest emits exactly one data line).
    tail -1 "$TMP/slow.csv"  >> "$COMBINED"
    tail -1 "$TMP/ncus.csv"  >> "$COMBINED"
    tail -1 "$TMP/local.csv" >> "$COMBINED"

    echo "  done. appended 3 rows."
}

for v in mutex broadcast; do
    run_one "$v"
done

echo ""
echo "=== combined ==="
column -t -s, "$COMBINED"
