#!/usr/bin/env python3
import csv, statistics
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).parent
TRIALS = sorted(ROOT.glob("trial_*.csv"))

# Load: rows[(variant, metric)] -> list of values
rows = {}
for t in TRIALS:
    with open(t) as f:
        reader = csv.DictReader(f)
        for r in reader:
            v = r["variant"]
            for k in ("p50_us","p95_us","p99_us","p99_9_us","max_us"):
                rows.setdefault((v, k), []).append(int(r[k]))

# Aggregate (median, min, max)
agg = {}
for key, vals in rows.items():
    agg[key] = (statistics.median(vals), min(vals), max(vals))

# Write aggregated CSV
with open(ROOT/"aggregated.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["variant","metric","median_us","min_us","max_us"])
    for (v, k), (med, mn, mx) in sorted(agg.items()):
        w.writerow([v, k, med, mn, mx])

# Plot: bar chart, log scale, mutex vs broadcast across percentiles
metrics = ["p50_us","p95_us","p99_us","p99_9_us","max_us"]
labels  = ["p50","p95","p99","p99.9","max"]
x = np.arange(len(metrics))
width = 0.36

m_med = [agg[("mutex",     k)][0] for k in metrics]
b_med = [agg[("broadcast", k)][0] for k in metrics]
m_err = [[agg[("mutex",     k)][0]-agg[("mutex",     k)][1] for k in metrics],
         [agg[("mutex",     k)][2]-agg[("mutex",     k)][0] for k in metrics]]
b_err = [[agg[("broadcast", k)][0]-agg[("broadcast", k)][1] for k in metrics],
         [agg[("broadcast", k)][2]-agg[("broadcast", k)][0] for k in metrics]]

fig, ax = plt.subplots(figsize=(10, 6))
bars_m = ax.bar(x - width/2, m_med, width, yerr=m_err, capsize=4,
                label="Mutex<Vec<Subscriber>> (lock held across .await)",
                color="#d62728")
bars_b = ax.bar(x + width/2, b_med, width, yerr=b_err, capsize=4,
                label="tokio::sync::broadcast (per-subscriber task + queue)",
                color="#1f77b4")

ax.set_yscale("log")
ax.set_ylabel("Fast-subscriber latency (µs, log scale)")
ax.set_xticks(x)
ax.set_xticklabels(labels)
ax.set_title("Fast subscribers, head-of-line blocked by one slow consumer\n"
             "8 publishers × 200 Hz, 200 subscribers, 1 slow @ 10 ms, 5 trials")
ax.legend(loc="upper left")
ax.grid(axis='y', alpha=0.3, which='both')

# Annotate the dramatic p99 ratio
ratio = agg[("mutex","p99_us")][0] / max(agg[("broadcast","p99_us")][0], 1)
ax.annotate(f"{ratio:.0f}× at p99",
            xy=(2, agg[("mutex","p99_us")][0]),
            xytext=(2.5, agg[("mutex","p99_us")][0] * 0.4),
            fontsize=12, fontweight='bold',
            arrowprops=dict(arrowstyle='->', color='black'))

plt.tight_layout()
plt.savefig(ROOT/"bench_pathology.png", dpi=150, bbox_inches='tight')
print(f"wrote {ROOT/'bench_pathology.png'}")
print(f"wrote {ROOT/'aggregated.csv'}")
print()
print(f"Headline: mutex p99 = {agg[('mutex','p99_us')][0]/1000:.1f} ms, "
      f"broadcast p99 = {agg[('broadcast','p99_us')][0]:.0f} µs "
      f"→ {ratio:.0f}× difference")
