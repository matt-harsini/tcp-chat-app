#!/usr/bin/env python3
# Reproduces Gjengset's lock-scaling result on this machine.
# Left panel  : Mutex vs RwLock, linear y — direct visual match to his slide 7.
# Right panel : + AtomicU64 baseline, log y — "locks collapse, atomics don't".
import csv
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path
from collections import defaultdict

ROOT = Path(__file__).parent
rows = list(csv.DictReader(open(ROOT / "sweep.csv")))

# (primitive, threads) -> [mops, ...]
agg = defaultdict(list)
for r in rows:
    agg[(r["primitive"], int(r["threads"]))].append(float(r["mops_per_sec"]))

prims = ["mutex", "rwlock", "atomic"]
colors = {"mutex": "#d62728", "rwlock": "#1f77b4", "atomic": "#2ca02c"}
labels = {
    "mutex": "std::sync::Mutex (read)",
    "rwlock": "std::sync::RwLock (read)",
    "atomic": "AtomicU64::load (baseline)",
}
threads = sorted({int(r["threads"]) for r in rows})


def series(prim):
    med, lo, hi = [], [], []
    for t in threads:
        v = np.array(agg[(prim, t)])
        m = float(np.median(v))
        med.append(m)
        lo.append(m - v.min())
        hi.append(v.max() - m)
    return np.array(med), np.array([lo, hi])


fig, (axL, axR) = plt.subplots(1, 2, figsize=(15, 6))

# Left: mirror Gjengset slide 7 exactly — Mutex vs RwLock, linear.
for prim in ("mutex", "rwlock"):
    med, err = series(prim)
    axL.errorbar(threads, med, yerr=err, marker="o", capsize=4,
                 color=colors[prim], label=labels[prim], linewidth=2)
axL.set_title("Both locks collapse past 1 thread (Apple Silicon)\n"
              "Trivial critical section (read a u64), linear scale")
axL.set_xlabel("Number of threads")
axL.set_ylabel("Million operations / second")
axL.set_xticks(threads)
axL.grid(alpha=0.3)
axL.legend()
axL.set_ylim(bottom=0)

# Right: add the atomic baseline, log scale — the payoff.
for prim in prims:
    med, err = series(prim)
    axR.errorbar(threads, med, yerr=err, marker="o", capsize=4,
                 color=colors[prim], label=labels[prim], linewidth=2)
axR.set_yscale("log")
axR.set_title("Locks collapse, the atomic doesn't\n"
              "Same test + AtomicU64 baseline, log scale")
axR.set_xlabel("Number of threads")
axR.set_ylabel("Million operations / second (log)")
axR.set_xticks(threads)
axR.grid(alpha=0.3, which="both")
axR.legend()

# Headline: lock collapse vs atomic scaling.
mu = {t: float(np.median(agg[("mutex", t)])) for t in threads}
at = {t: float(np.median(agg[("atomic", t)])) for t in threads}
tmax = threads[-1]
worst_t = min(threads, key=lambda t: mu[t])
collapse = mu[threads[0]] / mu[worst_t]
gap = at[tmax] / mu[tmax]
fig.text(0.5, 0.005,
         f"Mutex peaks at 1 thread ({mu[threads[0]]:.0f} M ops/s), bottoms at "
         f"{worst_t} threads ({mu[worst_t]:.0f} M ops/s) — a {collapse:.1f}x collapse from "
         f"ADDING threads.   Same workload on an atomic at {tmax} threads: "
         f"{at[tmax]:.0f} M ops/s ({gap:.0f}x the mutex, and still scaling up).",
         ha="center", fontsize=10.5, style="italic")

plt.tight_layout(rect=[0, 0.04, 1, 1])
out = ROOT / "lock_scaling.png"
plt.savefig(out, dpi=150, bbox_inches="tight")
print(f"wrote {out}")
for prim in prims:
    med, _ = series(prim)
    print(f"{prim:>7}: 1thr={med[0]:8.1f}  2thr={med[1]:8.1f}  "
          f"max@{threads[int(np.argmax(med))]}thr={med.max():8.1f} M ops/s")
print(f"mutex collapse 1->{worst_t}thr: {collapse:.1f}x   "
      f"atomic@{tmax}thr / mutex@{tmax}thr: {gap:.0f}x")
