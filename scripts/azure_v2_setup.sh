#!/usr/bin/env bash
# Per-VM setup for the slide 17 replacement Azure run.
# Idempotent — safe to re-run. Pins kernel TCP knobs, disables NIC offloads
# that vary by hardware, and dumps a full provenance record to /tmp/provenance.txt.
#
# Usage (run via SSH on each VM):
#   bash azure_v2_setup.sh

set -uo pipefail

ROLE="${1:-unknown}"

echo "=== azure_v2_setup.sh role=${ROLE} host=$(hostname) ==="

# --- Kernel TCP buffer pinning ---
# Disables autotuning. Pinned to 256 KiB on both rmem and wmem to match the
# loadtest.rs and server_*.rs binary-side setsockopt(SO_RCVBUF/SO_SNDBUF, 262144).
sudo sysctl -w net.ipv4.tcp_moderate_rcvbuf=0
sudo sysctl -w net.ipv4.tcp_rmem='65536 262144 262144'
sudo sysctl -w net.ipv4.tcp_wmem='65536 262144 262144'
sudo sysctl -w net.core.rmem_default=262144
sudo sysctl -w net.core.wmem_default=262144
sudo sysctl -w net.core.rmem_max=262144
sudo sysctl -w net.core.wmem_max=262144

# --- NIC offloads off ---
# TSO/GSO/GRO/LRO segment-merge timing varies by hardware and confounds latency
# measurement. Disable for reproducibility. eth0 is Azure's primary interface.
sudo ethtool -K eth0 tso off gso off gro off lro off 2>/dev/null || true

# --- Install mpstat if not present (used during runs) ---
if ! command -v mpstat >/dev/null 2>&1; then
    sudo apt-get update -qq && sudo apt-get install -y -qq sysstat >/dev/null 2>&1 || true
fi

# --- Provenance dump ---
PROV=/tmp/provenance.txt
{
    echo "=== provenance: role=${ROLE} host=$(hostname) ==="
    echo "captured_at: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "--- uname -a ---"
    uname -a
    echo ""
    echo "--- lscpu ---"
    lscpu
    echo ""
    echo "--- /proc/cpuinfo (first cpu) ---"
    sed -n '1,30p' /proc/cpuinfo
    echo ""
    echo "--- memory ---"
    free -h
    echo ""
    echo "--- relevant sysctl ---"
    sudo sysctl -a 2>/dev/null | grep -E 'tcp_(r|w)mem|tcp_moderate_rcvbuf|net\.core\.(r|w)mem'
    echo ""
    echo "--- ethtool -k eth0 (offload state) ---"
    sudo ethtool -k eth0 2>/dev/null | grep -E 'tso|gso|gro|lro|tx-|rx-' | head -20
    echo ""
    echo "--- chronyc tracking ---"
    chronyc tracking 2>/dev/null || timedatectl
    echo ""
    echo "--- chronyc sources -v ---"
    chronyc sources -v 2>/dev/null || true
    echo ""
    echo "--- network interfaces ---"
    ip -br addr
    echo ""
    echo "--- kernel command line ---"
    cat /proc/cmdline
} > "$PROV"

echo ""
echo "=== Provenance written to ${PROV} ==="
echo "=== Setup complete ==="
