#!/usr/bin/env python3
"""
Plot throughput and tail latency from sweep CSV.

Usage:
    python3 scripts/plot.py results/sweep_YYYYMMDD_HHMMSS.csv
"""
import csv
import sys
from collections import defaultdict
from pathlib import Path

import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

# Conservative defense-industry monochrome with steel-blue accent (matches deck palette).
MUTEX_COLOR = "#9A9A9A"   # neutral gray
BCAST_COLOR = "#2C4A5E"   # steel-blue accent
BG = "white"
GRID = "#D9D9D9"
TXT = "#1A1A1A"


def load(csv_path):
    """The CSV has 13 columns; loadtest's --label embedded a comma, so the
    first two CSV fields are ('mutex', 'N=10') instead of one combined label.
    Read positionally to be robust."""
    data = defaultdict(dict)  # data[variant][N] = row dict
    with open(csv_path) as f:
        next(f)  # skip header
        for line in f:
            cols = line.rstrip().split(",")
            if len(cols) < 13:
                continue
            variant = cols[0]
            n = int(cols[1].split("=")[1])
            row = {
                "clients": int(cols[2]),
                "duration_s": int(cols[3]),
                "rate_per_client": int(cols[4]),
                "sent": int(cols[5]),
                "received": int(cols[6]),
                "recv_per_sec": float(cols[7]),
                "p50_us": int(cols[8]),
                "p95_us": int(cols[9]),
                "p99_us": int(cols[10]),
                "p99_9_us": int(cols[11]),
                "max_us": int(cols[12]),
            }
            data[variant][n] = row
    return data


def style_axes(ax):
    ax.set_facecolor(BG)
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.spines["left"].set_color(TXT)
    ax.spines["bottom"].set_color(TXT)
    ax.tick_params(colors=TXT)
    ax.grid(True, color=GRID, linewidth=0.7, axis="y")
    ax.set_axisbelow(True)


def make_throughput_chart(data, out_path):
    fig, ax = plt.subplots(figsize=(8, 4.5), dpi=160)
    fig.patch.set_facecolor(BG)
    style_axes(ax)

    for variant, color, marker in [
        ("mutex", MUTEX_COLOR, "o"),
        ("broadcast", BCAST_COLOR, "s"),
    ]:
        if variant not in data:
            continue
        ns = sorted(data[variant].keys())
        ys = [float(data[variant][n]["recv_per_sec"]) for n in ns]
        label = "Arc<Mutex<Vec<...>>>" if variant == "mutex" else "tokio::broadcast"
        ax.plot(ns, ys, color=color, marker=marker, linewidth=2.0,
                markersize=7, label=label)

    ax.set_xlabel("Concurrent clients", fontsize=11, color=TXT)
    ax.set_ylabel("Deliveries / second", fontsize=11, color=TXT)
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xticks([10, 25, 50, 100, 200, 400])
    ax.get_xaxis().set_major_formatter(mticker.ScalarFormatter())
    ax.legend(loc="upper left", frameon=False, fontsize=10)
    fig.tight_layout()
    fig.savefig(out_path, dpi=200, bbox_inches="tight")
    print(f"Wrote {out_path}")


def make_latency_chart(data, out_path):
    fig, ax = plt.subplots(figsize=(8, 4.5), dpi=160)
    fig.patch.set_facecolor(BG)
    style_axes(ax)

    for variant, color, marker in [
        ("mutex", MUTEX_COLOR, "o"),
        ("broadcast", BCAST_COLOR, "s"),
    ]:
        if variant not in data:
            continue
        ns = sorted(data[variant].keys())
        ys = [float(data[variant][n]["p99_us"]) / 1000.0 for n in ns]  # ms
        label = "Arc<Mutex<Vec<...>>>" if variant == "mutex" else "tokio::broadcast"
        ax.plot(ns, ys, color=color, marker=marker, linewidth=2.0,
                markersize=7, label=label)

    ax.set_xlabel("Concurrent clients", fontsize=11, color=TXT)
    ax.set_ylabel("p99 end-to-end latency (ms)", fontsize=11, color=TXT)
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xticks([10, 25, 50, 100, 200, 400])
    ax.get_xaxis().set_major_formatter(mticker.ScalarFormatter())
    ax.legend(loc="upper left", frameon=False, fontsize=10)
    fig.tight_layout()
    fig.savefig(out_path, dpi=200, bbox_inches="tight")
    print(f"Wrote {out_path}")


def make_table(data, out_path):
    """Plain-text table summary suitable for slide notes."""
    lines = []
    ns = sorted(set(n for v in data.values() for n in v.keys()))
    lines.append(f"{'N':>5}  {'mutex tput':>14}  {'bcast tput':>14}  {'mutex p99':>14}  {'bcast p99':>14}")
    for n in ns:
        m = data.get("mutex", {}).get(n)
        b = data.get("broadcast", {}).get(n)
        m_t = f"{float(m['recv_per_sec']):,.0f}" if m else "-"
        b_t = f"{float(b['recv_per_sec']):,.0f}" if b else "-"
        m_l = f"{float(m['p99_us'])/1000:.2f} ms" if m else "-"
        b_l = f"{float(b['p99_us'])/1000:.2f} ms" if b else "-"
        lines.append(f"{n:>5}  {m_t:>14}  {b_t:>14}  {m_l:>14}  {b_l:>14}")
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"Wrote {out_path}")
    print()
    print("\n".join(lines))


def main():
    if len(sys.argv) < 2:
        print("usage: plot.py <sweep.csv>", file=sys.stderr)
        sys.exit(1)
    csv_path = Path(sys.argv[1])
    data = load(csv_path)
    out_dir = csv_path.parent
    stem = csv_path.stem
    make_throughput_chart(data, out_dir / f"{stem}_throughput.png")
    make_latency_chart(data, out_dir / f"{stem}_p99_latency.png")
    make_table(data, out_dir / f"{stem}_summary.txt")


if __name__ == "__main__":
    main()
