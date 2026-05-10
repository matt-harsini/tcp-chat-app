#!/usr/bin/env python3
"""Plot cross-region experiment results.

CSV format: label has variant-LOCATION,N=X,R=Y embedded (commas inside the label).
Parse positionally as before.
"""
import csv, sys
from pathlib import Path
import matplotlib.pyplot as plt
import numpy as np

CSV_PATH = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent.parent / "results" / "xregion.csv"
OUT_DIR = CSV_PATH.parent

MUTEX = "#9A9A9A"
BCAST = "#2C4A5E"
LOCAL_HATCH = ""
REMOTE_HATCH = "//"
TXT = "#1A1A1A"
GRID = "#D9D9D9"

# Parse positionally — label is split into variant-LOCATION + N + R fields
rows = []
with open(CSV_PATH) as f:
    next(f)
    for line in f:
        c = line.rstrip().split(",")
        if len(c) < 13:
            continue
        # label is "mutex-LOCAL" or "broadcast-LOCAL" etc.
        label = c[0]
        # Parse N from c[1] like "N=50"
        n_field = c[1]
        if not n_field.startswith("N="):
            continue
        n = int(n_field.split("=")[1])
        # R may or may not be present in c[2] (older runs lacked R label)
        if c[2].startswith("R="):
            r = int(c[2].split("=")[1])
            offset = 3
        else:
            r = 1
            offset = 2
        # Now from offset: clients, duration_s, rate_per_client, sent, received, recv_per_sec, p50, p95, p99, p99_9, max
        # Columns from offset: clients, dur, rate_milli, sent, received, recv_per_sec, p50, p95, p99, p99_9, max
        try:
            row = {
                "variant": label.split("-")[0],
                "location": label.split("-")[1],
                "n": n,
                "r": r,
                "p50_us": int(c[offset + 6]),
                "p95_us": int(c[offset + 7]),
                "p99_us": int(c[offset + 8]),
            }
            rows.append(row)
        except (ValueError, IndexError):
            continue


def plot_p99_grouped():
    """Grouped bar chart: x=condition, bars=variant×location, y=p99 (log)."""
    # Conditions: (N, R) combos that have all 4 variants
    conds = sorted(set((r["n"], r["r"]) for r in rows))
    valid = []
    for n, r in conds:
        present = {(row["variant"], row["location"]) for row in rows if row["n"] == n and row["r"] == r}
        if {("mutex", "LOCAL"), ("mutex", "REMOTE"), ("broadcast", "LOCAL"), ("broadcast", "REMOTE")}.issubset(present):
            valid.append((n, r))

    fig, ax = plt.subplots(figsize=(10, 5), dpi=160)
    fig.patch.set_facecolor("white")
    ax.set_facecolor("white")
    for s in ("top", "right"): ax.spines[s].set_visible(False)
    ax.spines["left"].set_color(TXT); ax.spines["bottom"].set_color(TXT)

    x = np.arange(len(valid))
    w = 0.2
    def get(variant, loc, n, r):
        for row in rows:
            if row["variant"] == variant and row["location"] == loc and row["n"] == n and row["r"] == r:
                return row["p99_us"] / 1000.0  # ms
        return None
    series = [
        ("mutex", "LOCAL", MUTEX, "", "Mutex · LOCAL clients"),
        ("mutex", "REMOTE", MUTEX, "//", "Mutex · REMOTE clients"),
        ("broadcast", "LOCAL", BCAST, "", "Broadcast · LOCAL clients"),
        ("broadcast", "REMOTE", BCAST, "//", "Broadcast · REMOTE clients"),
    ]
    for i, (variant, loc, color, hatch, label) in enumerate(series):
        ys = [get(variant, loc, n, r) for n, r in valid]
        ax.bar(x + (i - 1.5) * w, ys, w, label=label, color=color, hatch=hatch, edgecolor=TXT, linewidth=0.7)

    ax.set_xticks(x)
    ax.set_xticklabels([f"N={n}\nR={r}/s" for n, r in valid], color=TXT, fontsize=10)
    ax.set_ylabel("p99 end-to-end latency (ms)", color=TXT, fontsize=11)
    ax.set_yscale("log")
    ax.grid(True, color=GRID, axis="y", linewidth=0.7)
    ax.set_axisbelow(True)
    ax.tick_params(colors=TXT)
    ax.legend(loc="upper left", frameon=False, fontsize=9)
    ax.set_title("Cross-region p99 latency (eastus2 server, mexicocentral remote clients)",
                 color=TXT, fontsize=12, pad=12)
    fig.tight_layout()
    out = OUT_DIR / "xregion_p99_grouped.png"
    fig.savefig(out, dpi=200, bbox_inches="tight")
    print(f"Wrote {out}")


def make_summary_table():
    print("\n--- summary table ---")
    print(f"{'N':>4} {'R':>4} {'mutex-LOCAL':>14} {'mutex-REMOTE':>14} {'bcast-LOCAL':>14} {'bcast-REMOTE':>14}")
    conds = sorted(set((row["n"], row["r"]) for row in rows))
    for n_, r_ in conds:
        def fmt(v, l):
            for row in rows:
                if row["variant"] == v and row["location"] == l and row["n"] == n_ and row["r"] == r_:
                    return f"{row['p99_us']/1000:.1f} ms"
            return "—"
        print(f"{n_:>4} {r_:>4} {fmt('mutex','LOCAL'):>14} {fmt('mutex','REMOTE'):>14} "
              f"{fmt('broadcast','LOCAL'):>14} {fmt('broadcast','REMOTE'):>14}")


if __name__ == "__main__":
    plot_p99_grouped()
    make_summary_table()
