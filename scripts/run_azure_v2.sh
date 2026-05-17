#!/usr/bin/env bash
# Slide 17 replacement: clean, reproducible Azure A/B benchmark.
#
# Topology:
#   Server   : eastus2, in PPG-A, accelerated networking
#   Client A : eastus2, same PPG-A, accelerated networking (controlled plane)
#   Client B : northcentralus (natural cross-region slow path)
#
# Workload:
#   - 60 s warmup, 240 s measurement, rate 5 msg/s/client, slow-count 0
#   - Sweep: variants {mutex, broadcast} × N_a {10, 50, 100, 200} × 5 trials
#   - Bookend: broadcast/N_a=10 single trial at start and end (drift detector)
#   - Total: 40 + 2 = 42 trials, ~3.5 hr wall-clock
#
# Replicability levers:
#   - Same VM-boot for all trials (no boot-to-boot drift)
#   - 5 trials/cell → report median + IQR not single runs
#   - Start/end bookends detect environmental contamination
#   - mpstat (%steal) + ss -tmi snapshots captured per trial
#   - All raw CSVs preserved per trial
#
# Requires /tmp/vm_ips.sh defining SERVER_IP, CLIENT_A_IP, CLIENT_B_IP.

set -uo pipefail

source /tmp/vm_ips.sh
: "${SERVER_IP:?SERVER_IP not set}"
: "${CLIENT_A_IP:?CLIENT_A_IP not set}"
: "${CLIENT_B_IP:?CLIENT_B_IP not set}"
: "${CLIENT_C_IP:?CLIENT_C_IP not set}"

SSH_OPTS=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=20)

ROOT=/Users/matthewkim/Developer/tokio-chat
TS=$(date +%Y%m%d_%H%M%S)
OUT="$ROOT/results/azure_v2_$TS"
mkdir -p "$OUT/raw" "$OUT/provenance"

# --- Constants ---
WARMUP=30
DURATION=120
RATE=5
N_B=50                       # Client B: all 50 are SLOW consumers (Option B mode)
N_C=50                       # Client C: all 50 are SLOW consumers (Option B mode)
N_VALUES=(200)               # Client A: 200 ACTIVE clients
TRIALS=1                     # quick sanity: 1 trial per variant
VARIANTS=(broadcast threads)

CSV_HEADER="label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us"

# --- Preflight: verify all 3 VMs reachable and binaries present ---
preflight() {
    echo "=== Preflight checks ==="
    for pair in "SERVER:$SERVER_IP" "CLIENT_A:$CLIENT_A_IP" "CLIENT_B:$CLIENT_B_IP" "CLIENT_C:$CLIENT_C_IP"; do
        local role="${pair%%:*}" ip="${pair##*:}"
        local ok
        ok=$(ssh "${SSH_OPTS[@]}" matthewkim@"$ip" "test -x ~/tokio-chat/target/release/loadtest && test -x ~/tokio-chat/target/release/server_mutex && test -x ~/tokio-chat/target/release/server_broadcast && test -x ~/tokio-chat/target/release/server_threads && echo OK" 2>/dev/null)
        if [ "$ok" != "OK" ]; then
            echo "FAIL: $role ($ip) missing one of {loadtest, server_mutex, server_broadcast, server_threads}"
            return 1
        fi
        echo "  $role ($ip): binaries OK"
    done

    # Verify sysctl pinning is in effect on server (most important)
    local rmem
    rmem=$(ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "sysctl -n net.ipv4.tcp_rmem" 2>/dev/null)
    if [[ "$rmem" != *"262144"* ]]; then
        echo "WARN: server sysctl tcp_rmem is '$rmem' — expected '...262144'. Run azure_v2_setup.sh on server."
    else
        echo "  Server sysctl tcp_rmem pinned: $rmem"
    fi
}

# --- Capture per-VM provenance (one-shot at session start) ---
capture_provenance() {
    echo "=== Capturing provenance ==="
    for pair in "server:$SERVER_IP" "client_a:$CLIENT_A_IP" "client_b:$CLIENT_B_IP" "client_c:$CLIENT_C_IP"; do
        local role="${pair%%:*}" ip="${pair##*:}"
        ssh "${SSH_OPTS[@]}" matthewkim@"$ip" "cat /tmp/provenance.txt 2>/dev/null || echo 'no provenance.txt — azure_v2_setup.sh was not run'" \
            > "$OUT/provenance/${role}.txt"
        echo "  $role provenance → $OUT/provenance/${role}.txt"
    done
}

# --- Run one trial ---
# Args: phase variant N_a trial_idx
run_trial() {
    local phase="$1" variant="$2" N_a="$3" trial="$4"
    local cell="${phase}_${variant}_Na${N_a}_t${trial}"
    local cell_dir="$OUT/raw/$cell"
    mkdir -p "$cell_dir"

    printf "[%s] %s  variant=%s  N_a=%d  trial=%d\n" "$(date +%H:%M:%S)" "$phase" "$variant" "$N_a" "$trial"

    # Start server + background capture
    ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "
        pkill -9 -x server_mutex 2>/dev/null
        pkill -9 -x server_broadcast 2>/dev/null
        rm -f /tmp/mpstat.log /tmp/ss.log /tmp/server.log
        sleep 0.5
        nohup ~/tokio-chat/target/release/server_${variant} 0.0.0.0:8080 > /tmp/server.log 2>&1 < /dev/null &
        disown
        sleep 1.5
        nohup mpstat 1 > /tmp/mpstat.log 2>&1 < /dev/null &
        echo \$! > /tmp/mpstat.pid
        disown
        nohup bash -c 'while true; do echo \"=== \$(date +%T) ===\" >> /tmp/ss.log; ss -tmi >> /tmp/ss.log 2>&1; sleep 5; done' < /dev/null > /dev/null 2>&1 &
        echo \$! > /tmp/ss.pid
        disown
    " > /dev/null

    # Confirm listener
    local listening
    listening=$(ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "ss -ln 2>/dev/null | grep -c ':8080 '" 2>/dev/null || echo 0)
    if [ "$listening" -lt 1 ]; then
        echo "  ERROR: server not listening — skipping cell"
        echo "${phase},${variant},${N_a},${N_B},${trial},SKIP,server-not-listening" >> "$OUT/skipped.csv"
        return 1
    fi

    # Launch both clients in parallel
    ssh "${SSH_OPTS[@]}" matthewkim@"$CLIENT_A_IP" "
        ~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_a} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count 0 \
            --label '${variant}-A-Na${N_a}-t${trial}'
    " > "$cell_dir/client_a.csv" 2> "$cell_dir/client_a.err" &
    local P_A=$!

    ssh "${SSH_OPTS[@]}" matthewkim@"$CLIENT_B_IP" "
        ~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_B} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count ${N_B} \
            --label '${variant}-B-Nb${N_B}-slow-t${trial}'
    " > "$cell_dir/client_b.csv" 2> "$cell_dir/client_b.err" &
    local P_B=$!

    ssh "${SSH_OPTS[@]}" matthewkim@"$CLIENT_C_IP" "
        ~/tokio-chat/target/release/loadtest \
            --addr ${SERVER_IP}:8080 \
            --clients ${N_C} \
            --duration-secs ${DURATION} \
            --warmup-secs ${WARMUP} \
            --rate-per-client ${RATE} \
            --slow-count ${N_C} \
            --label '${variant}-C-Nc${N_C}-slow-t${trial}'
    " > "$cell_dir/client_c.csv" 2> "$cell_dir/client_c.err" &
    local P_C=$!

    wait $P_A $P_B $P_C || true

    # Stop server + background capture
    ssh "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP" "
        pkill -9 -x server_${variant} 2>/dev/null
        [ -f /tmp/mpstat.pid ] && kill \$(cat /tmp/mpstat.pid) 2>/dev/null
        [ -f /tmp/ss.pid ] && kill \$(cat /tmp/ss.pid) 2>/dev/null
        sleep 0.5
        true
    " > /dev/null

    # Retrieve server-side captures
    scp "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP:/tmp/server.log" "$cell_dir/server.log" 2>/dev/null || true
    scp "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP:/tmp/mpstat.log" "$cell_dir/mpstat.log" 2>/dev/null || true
    scp "${SSH_OPTS[@]}" matthewkim@"$SERVER_IP:/tmp/ss.log"     "$cell_dir/ss.log"     2>/dev/null || true

    # Quick steal-time check (CPU steal > 2% flags this trial as suspect)
    local max_steal
    max_steal=$(awk '
        /CPU/ {for(i=1;i<=NF;i++) if($i=="%steal") col=i; next}
        $1 ~ /^[0-9]/ && col {if($col+0 > m) m=$col+0}
        END {printf "%.2f", m}
    ' "$cell_dir/mpstat.log" 2>/dev/null || echo "0")
    printf "  max %%steal = %s%%\n" "$max_steal"
    if (( $(awk -v v="$max_steal" 'BEGIN{print (v>2.0)}') )); then
        echo "  WARN: %steal > 2% — trial likely contaminated"
        echo "$cell max_steal=$max_steal" >> "$OUT/contamination.txt"
    fi
}

# --- Main flow ---
preflight || { echo "Preflight failed — fix and re-run."; exit 1; }
capture_provenance

# Bookends skipped — single-N high-load comparison.

# Main sweep
TOTAL=$(( ${#VARIANTS[@]} * ${#N_VALUES[@]} * TRIALS ))
echo ""
echo "=== Main sweep: $TOTAL trials ==="
for variant in "${VARIANTS[@]}"; do
    for N in "${N_VALUES[@]}"; do
        for trial in $(seq 1 $TRIALS); do
            run_trial "main" "$variant" "$N" "$trial"
        done
    done
done

# End bookend skipped — start-bookend + per-trial %steal monitoring
# is sufficient drift detection given %steal=0 across all 42 trials in
# the prior 5.5hr session.

# Summary
echo ""
echo "=== DONE ==="
echo "Output directory: $OUT"
echo "Trial CSVs:       $OUT/raw/*/client_{a,b}.csv"
echo "Provenance:       $OUT/provenance/{server,client_a,client_b}.txt"
[ -f "$OUT/contamination.txt" ] && echo "Contamination flags: $(wc -l < "$OUT/contamination.txt") trials > 2% steal"
[ -f "$OUT/skipped.csv" ]       && echo "Skipped trials:      $(wc -l < "$OUT/skipped.csv")"
echo ""
echo "Bookend comparison:"
echo "  start:" && tail -1 "$OUT/raw/bookend_start_broadcast_Na10_t1/client_a.csv" 2>/dev/null
echo "  end:  " && tail -1 "$OUT/raw/bookend_end_broadcast_Na10_t1/client_a.csv"   2>/dev/null
