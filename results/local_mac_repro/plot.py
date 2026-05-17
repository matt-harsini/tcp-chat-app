#!/usr/bin/env python3
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path

ROOT = Path(__file__).parent

# Hardcoded from threads_n50_v2.csv and broadcast_n50_v2.csv (single trial each)
# label,clients,duration_s,rate_per_client,sent,received,recv_per_sec,p50_us,p95_us,p99_us,p99_9_us,max_us
threads   = (50, 1233175, 131541140, 547, 249036799, 356777983, 391380991, 481034239, 500170751)
broadcast = (50, 281421622, 99528280, 303, 2012159,   2689023,   2916351,   3948543,   5922815)
# fields:    N,  sent,     recv,      wall_s, p50, p95, p99, p999, max  (latencies in µs)

labels = ["p50","p95","p99","p99.9","max"]
t_lat = [v/1e6 for v in threads[4:9]]
b_lat = [v/1e6 for v in broadcast[4:9]]

x = np.arange(len(labels))
width = 0.36

fig, ax = plt.subplots(figsize=(11, 6.5))
b_t = ax.bar(x - width/2, t_lat, width,
             label="std::sync::Mutex + OS threads + blocking I/O",
             color="#d62728")
b_b = ax.bar(x + width/2, b_lat, width,
             label="tokio::sync::broadcast (per-subscriber task + queue)",
             color="#1f77b4")
ax.set_yscale("log")
ax.set_ylabel("End-to-end latency (seconds, log scale)")
ax.set_xticks(x)
ax.set_xticklabels(labels)
ax.set_title("Local Mac, N=50 saturating clients, single host (server + clients colocated)\n"
             "Single 5-minute trial each. Histogram bound = 1 hour (no clipping).")
ax.legend(loc="lower right")
ax.grid(axis='y', alpha=0.3, which='both')

# Annotate the dramatic p99 ratio
ratio_p99 = t_lat[2] / b_lat[2]
ax.annotate(f"{ratio_p99:.0f}× at p99",
            xy=(2, t_lat[2]),
            xytext=(2.5, t_lat[2] * 0.5),
            fontsize=13, fontweight='bold',
            arrowprops=dict(arrowstyle='->', color='black'))

# Add value labels on bars
for bar, val in zip(b_t, t_lat):
    ax.text(bar.get_x() + bar.get_width()/2, val * 1.15,
            f"{val:.0f} s", ha='center', fontsize=9, color="#d62728")
for bar, val in zip(b_b, b_lat):
    ax.text(bar.get_x() + bar.get_width()/2, val * 1.15,
            f"{val:.1f} s", ha='center', fontsize=9, color="#1f77b4")

# Note on wall time
fig.text(0.5, 0.01,
         f"Wall clock: threads = {threads[3]} s (82% over nominal 300 s), broadcast = {broadcast[3]} s (on time)",
         ha='center', fontsize=10, style='italic')

plt.tight_layout(rect=[0, 0.03, 1, 1])
plt.savefig(ROOT / "slide17_n50.png", dpi=150, bbox_inches='tight')
print(f"wrote {ROOT/'slide17_n50.png'}")
print(f"p99 ratio: {ratio_p99:.1f}×")
print(f"p50 ratio: {t_lat[0]/b_lat[0]:.1f}×")
