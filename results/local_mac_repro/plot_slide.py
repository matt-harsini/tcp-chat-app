#!/usr/bin/env python3
# Slide-tuned variant of plot.py for the deck (S16): white background,
# larger fonts, ~2:1 aspect. Same verified v2 data as plot.py.
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path

ROOT = Path(__file__).parent

# Hardcoded from threads_n50_v2.csv / broadcast_n50_v2.csv (the v2, NON-clipped
# files; the non-v2 threads_n50.csv is histogram-clipped at 300 s — never use).
# fields: N, sent, recv, wall_s, p50, p95, p99, p999, max   (latencies in µs)
threads   = (50, 1233175, 131541140, 547, 249036799, 356777983, 391380991, 481034239, 500170751)
broadcast = (50, 281421622, 99528280, 303, 2012159,   2689023,   2916351,   3948543,   5922815)

# Guard: if this fails we picked up the clipped CSV (would read ~300 s).
assert threads[6] > 3.5e8, f"threads p99={threads[6]} looks clipped — wrong CSV"

plt.rcParams.update({
    "font.size": 13, "axes.titlesize": 15, "axes.labelsize": 14,
    "xtick.labelsize": 13, "ytick.labelsize": 12, "legend.fontsize": 12,
})

labels = ["p50", "p95", "p99", "p99.9", "max"]
t_lat = [v / 1e6 for v in threads[4:9]]
b_lat = [v / 1e6 for v in broadcast[4:9]]
x = np.arange(len(labels))
width = 0.36

fig, ax = plt.subplots(figsize=(11, 5.4))
fig.patch.set_facecolor("white")
b_t = ax.bar(x - width / 2, t_lat, width, color="#d62728",
             label="server_threads — std::sync::Mutex + OS threads")
b_b = ax.bar(x + width / 2, b_lat, width, color="#1f77b4",
             label="server_broadcast — tokio::sync::broadcast")
ax.set_yscale("log")
ax.set_ylabel("End-to-end latency (s, log)")
ax.set_xticks(x)
ax.set_xticklabels(labels)
ax.set_title("Local Mac · 50 saturating clients · single host · 270 s · "
             "HDR 1 µs–3600 s", fontweight="bold")
ax.legend(loc="upper center", bbox_to_anchor=(0.5, -0.09), ncol=2,
          frameon=False)
ax.grid(axis="y", alpha=0.3, which="both")

ratio_p99 = t_lat[2] / b_lat[2]
ax.annotate(f"{ratio_p99:.0f}× at p99", xy=(2, t_lat[2]),
            xytext=(2.45, t_lat[2] * 0.42), fontsize=15,
            fontweight="bold",
            arrowprops=dict(arrowstyle="->", color="black", lw=1.6))

for bar, val in zip(b_t, t_lat):
    ax.text(bar.get_x() + bar.get_width() / 2, val * 1.18,
            f"{val:.0f} s", ha="center", fontsize=10.5, color="#d62728")
for bar, val in zip(b_b, b_lat):
    ax.text(bar.get_x() + bar.get_width() / 2, val * 1.18,
            f"{val:.1f} s", ha="center", fontsize=10.5, color="#1f77b4")

plt.tight_layout()
out = ROOT / "slide16_headline.png"
plt.savefig(out, dpi=200, bbox_inches="tight", facecolor="white")
print(f"wrote {out}")
print(f"p99 ratio: {ratio_p99:.1f}x  p50 ratio: {t_lat[0]/b_lat[0]:.1f}x")
