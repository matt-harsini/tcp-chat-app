#!/usr/bin/env python3
"""3-way cross-region p99 chart: server eastus2, clients on LOCAL+MEXICO+CENTRAL."""
import sys
from pathlib import Path
import matplotlib.pyplot as plt
import numpy as np

CSV_PATH = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent.parent / "results" / "xregion3.csv"
OUT_DIR = CSV_PATH.parent

MUTEX = "#9A9A9A"
BCAST = "#2C4A5E"
TXT = "#1A1A1A"
GRID = "#D9D9D9"

rows = []
with open(CSV_PATH) as f:
    next(f)
    for line in f:
        c = line.rstrip().split(",")
        if len(c) < 14:
            continue
        # label0 = "mutex-LOCAL" etc; c[1]="N=X"; c[2]="R=Y"; c[3]=clients...
        if not c[1].startswith("N=") or not c[2].startswith("R="):
            continue
        try:
            variant, location = c[0].split("-")
            n = int(c[1].split("=")[1])
            r = int(c[2].split("=")[1])
            # Columns: clients(3), dur(4), rate_milli(5), sent(6), recv(7), recv_per_sec(8), p50(9), p95(10), p99(11), p99_9(12), max(13)
            row = {
                "variant": variant,
                "location": location,
                "n": n, "r": r,
                "p50_us": int(c[9]),
                "p95_us": int(c[10]),
                "p99_us": int(c[11]),
            }
            rows.append(row)
        except (ValueError, IndexError):
            continue

print(f"loaded {len(rows)} rows")


def plot_grouped():
    conds = sorted(set((row["n"], row["r"]) for row in rows))
    fig, ax = plt.subplots(figsize=(11, 5.5), dpi=160)
    fig.patch.set_facecolor("white"); ax.set_facecolor("white")
    for s in ("top", "right"): ax.spines[s].set_visible(False)
    ax.spines["left"].set_color(TXT); ax.spines["bottom"].set_color(TXT)

    x = np.arange(len(conds))
    w = 0.13
    series = [
        ("mutex", "LOCAL", MUTEX, "", "Mutex · LOCAL (loopback)"),
        ("mutex", "CENTRAL", MUTEX, "..", "Mutex · CENTRAL (~30ms RTT)"),
        ("mutex", "MEXICO", MUTEX, "//", "Mutex · MEXICO (~45ms RTT)"),
        ("broadcast", "LOCAL", BCAST, "", "Broadcast · LOCAL"),
        ("broadcast", "CENTRAL", BCAST, "..", "Broadcast · CENTRAL"),
        ("broadcast", "MEXICO", BCAST, "//", "Broadcast · MEXICO"),
    ]
    def get(variant, loc, n, r):
        for row in rows:
            if row["variant"] == variant and row["location"] == loc and row["n"] == n and row["r"] == r:
                return row["p99_us"] / 1000.0
        return None
    for i, (variant, loc, color, hatch, label) in enumerate(series):
        ys = [get(variant, loc, n, r) or 0 for n, r in conds]
        ax.bar(x + (i - 2.5) * w, ys, w, label=label, color=color, hatch=hatch, edgecolor=TXT, linewidth=0.6)

    ax.set_xticks(x)
    ax.set_xticklabels([f"N={n}/region\nR={r}/s" for n, r in conds], color=TXT, fontsize=10)
    ax.set_ylabel("p99 end-to-end latency (ms)", color=TXT, fontsize=11)
    ax.set_yscale("log")
    ax.grid(True, color=GRID, axis="y", linewidth=0.7); ax.set_axisbelow(True)
    ax.tick_params(colors=TXT)
    ax.legend(loc="upper left", frameon=False, fontsize=9, ncol=2)
    ax.set_title("3-way cross-region p99: server eastus2, clients in eastus2/northcentralus/mexicocentral",
                 color=TXT, fontsize=11, pad=10)
    fig.tight_layout()
    out = OUT_DIR / "xregion3_p99_grouped.png"
    fig.savefig(out, dpi=200, bbox_inches="tight")
    print(f"Wrote {out}")


def summary():
    print()
    print(f"{'N':>4} {'R':>4} | {'mutex-LOCAL':>13} {'mutex-CENTRAL':>15} {'mutex-MEXICO':>14} | {'bcast-LOCAL':>13} {'bcast-CENTRAL':>15} {'bcast-MEXICO':>14}")
    conds = sorted(set((row["n"], row["r"]) for row in rows))
    for n_, r_ in conds:
        def fmt(v, l):
            for row in rows:
                if row["variant"] == v and row["location"] == l and row["n"] == n_ and row["r"] == r_:
                    return f"{row['p99_us']/1000:.1f} ms"
            return "—"
        print(f"{n_:>4} {r_:>4} | {fmt('mutex','LOCAL'):>13} {fmt('mutex','CENTRAL'):>15} {fmt('mutex','MEXICO'):>14} | "
              f"{fmt('broadcast','LOCAL'):>13} {fmt('broadcast','CENTRAL'):>15} {fmt('broadcast','MEXICO'):>14}")


if __name__ == "__main__":
    plot_grouped()
    summary()
