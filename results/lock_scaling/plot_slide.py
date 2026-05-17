#!/usr/bin/env python3
# Slide-optimized variant of plot.py: gentler aspect (~1.9:1), larger fonts,
# so it stays legible embedded at ~4.7 in wide next to a code block.
import csv
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path
from collections import defaultdict

ROOT = Path(__file__).parent
rows = list(csv.DictReader(open(ROOT / "sweep.csv")))
agg = defaultdict(list)
for r in rows:
    agg[(r["primitive"], int(r["threads"]))].append(float(r["mops_per_sec"]))

prims = ["mutex", "rwlock", "atomic"]
colors = {"mutex": "#d62728", "rwlock": "#1f77b4", "atomic": "#2ca02c"}
labels = {"mutex": "Mutex (read)", "rwlock": "RwLock (read)",
          "atomic": "AtomicU64 (baseline)"}
threads = sorted({int(r["threads"]) for r in rows})

plt.rcParams.update({
    "font.size": 13, "axes.titlesize": 15, "axes.labelsize": 13,
    "xtick.labelsize": 12, "ytick.labelsize": 12, "legend.fontsize": 11.5,
})


def series(prim):
    med, lo, hi = [], [], []
    for t in threads:
        v = np.array(agg[(prim, t)])
        m = float(np.median(v))
        med.append(m); lo.append(m - v.min()); hi.append(v.max() - m)
    return np.array(med), np.array([lo, hi])


fig, (axL, axR) = plt.subplots(1, 2, figsize=(10.5, 5.4))

for prim in ("mutex", "rwlock"):
    med, err = series(prim)
    axL.errorbar(threads, med, yerr=err, marker="o", capsize=4,
                 color=colors[prim], label=labels[prim], linewidth=2.4,
                 markersize=7)
axL.set_title("Both locks collapse past 1 thread", fontweight="bold")
axL.set_xlabel("Number of threads")
axL.set_ylabel("Million ops / second")
axL.set_xticks(threads)
axL.grid(alpha=0.3)
axL.legend()
axL.set_ylim(bottom=0)

for prim in prims:
    med, err = series(prim)
    axR.errorbar(threads, med, yerr=err, marker="o", capsize=4,
                 color=colors[prim], label=labels[prim], linewidth=2.4,
                 markersize=7)
axR.set_yscale("log")
axR.set_title("Atomic scales; locks don't (log)", fontweight="bold")
axR.set_xlabel("Number of threads")
axR.set_ylabel("Million ops / second (log)")
axR.set_xticks(threads)
axR.grid(alpha=0.3, which="both")
axR.legend()

plt.tight_layout()
out = ROOT / "lock_scaling_slide.png"
plt.savefig(out, dpi=200, bbox_inches="tight", facecolor="white")
print(f"wrote {out}")
