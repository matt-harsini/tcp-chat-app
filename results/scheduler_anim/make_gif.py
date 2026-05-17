#!/usr/bin/env python3
"""Detailed Tokio work-stealing scheduler animation (scheduler layer only).
Models, per https://tokio.rs/blog/2019-10-scheduler :
  - per-worker fixed-size LOCAL run queue (push/pop at HEAD)
  - per-worker single LIFO SLOT (a just-woken task runs next: hot cache)
  - shared GLOBAL injection queue (external spawns / overflow)
  - work STEALING: an idle worker becomes a 'searcher' and steals HALF of a
    victim's local queue from the TAIL
Output: scheduler.gif (white bg, deck palette).
"""
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch
from matplotlib.animation import FuncAnimation, PillowWriter

SLATE, ACCENT, INK, GRAY = "#2C4A5E", "#5A7A8E", "#111111", "#6B6B6B"
LIGHT, RED, GREEN = "#EEF2F4", "#C0392B", "#2E7D52"
plt.rcParams["font.family"] = "DejaVu Sans"

W = [18.0, 50.0, 82.0]          # worker column centres
TASK_W, TASK_H = 6.4, 4.2


def rrect(ax, cx, cy, w, h, fc, ec, lw=1.2, txt="", tc="white", fs=10,
          bold=False, style="round,pad=0.02"):
    ax.add_patch(FancyBboxPatch((cx - w / 2, cy - h / 2), w, h,
                                boxstyle=style, fc=fc, ec=ec, lw=lw,
                                mutation_scale=6, zorder=3))
    if txt:
        ax.text(cx, cy, txt, ha="center", va="center", color=tc,
                fontsize=fs, fontweight="bold" if bold else "normal",
                zorder=4)


def arrow(ax, p0, p1, color=ACCENT, ls="-", lw=2.0):
    ax.add_patch(FancyArrowPatch(p0, p1, arrowstyle="-|>", mutation_scale=16,
                                 color=color, lw=lw, ls=ls,
                                 connectionstyle="arc3,rad=0.0", zorder=5))


# ---- scripted timeline: each entry is one fully-specified frame ----
def base():
    return {
        "workers": [
            {"state": "Running", "run": None, "lifo": None, "q": []},
            {"state": "Running", "run": None, "lifo": None, "q": []},
            {"state": "Running", "run": None, "lifo": None, "q": []},
        ],
        "glob": [], "moving": [], "note": "", "arrows": [],
    }


def clone(s):
    import copy
    return copy.deepcopy(s)


frames = []


def hold(s, n=7):
    for _ in range(n):
        frames.append(clone(s))


s = base()
s["workers"][0].update(q=[1, 2, 3, 4])
s["workers"][1].update(q=[5, 6])
s["note"] = "Each worker owns a fixed-size LOCAL run queue.  Worker 2 is idle."
hold(s, 8)

s = clone(s)
s["workers"][0].update(run=1, q=[2, 3, 4])
s["workers"][1].update(run=5, q=[6])
s["note"] = "A worker pops the HEAD of its own queue and runs it."
hold(s, 8)

s = clone(s)
s["workers"][0].update(run=1, q=[7, 2, 3, 4])
s["note"] = "Running task spawns task 7  →  pushed to its OWN queue head."
s["arrows"] = [("spawn0",)]
hold(s, 9)

s = clone(s)
s["workers"][1].update(run=5, lifo=8)
s["note"] = "Task 5 wakes task 8  →  LIFO SLOT: runs next on this worker (hot cache)."
s["arrows"] = [("lifo1",)]
hold(s, 10)

s = clone(s)
s["workers"][2].update(state="Searching")
s["note"] = "Worker 2's queue is empty  →  it becomes a SEARCHER."
hold(s, 9)

# steal: W2 takes HALF of W0's queue (tasks 3,4) from the TAIL
src = clone(s)
src["note"] = "Searcher checks global queue (empty), picks a victim, steals HALF from the TAIL."
x0, y0 = W[0] + 1.6 * (TASK_W + 1.4), 17.0   # ~ W0 tail
x1, y1 = W[2] - 0.5 * (TASK_W + 1.4), 17.0   # ~ W2 queue
for k in range(9):
    f = clone(src)
    f["workers"][0].update(run=1, q=[7, 2] + ([] if k > 2 else [3, 4][: max(0, 2 - 0)]))
    f["workers"][0]["q"] = [7, 2] if k >= 2 else [7, 2, 3, 4]
    t = k / 8.0
    f["moving"] = [
        {"id": 3, "x": x0 + (x1 - x0) * t, "y": y0 + 6 * (t * (1 - t)) * 4},
        {"id": 4, "x": x0 + (x1 - x0) * t + 7.6, "y": y0 + 6 * (t * (1 - t)) * 4},
    ]
    f["arrows"] = [("steal",)]
    frames.append(f)

s = clone(src)
s["workers"][0].update(run=1, q=[7, 2])
s["workers"][2].update(state="Running", run=3, q=[4])
s["note"] = "Stolen: Worker 2 now runs task 3, with task 4 queued.  Load rebalanced."
hold(s, 10)

s = clone(s)
s["glob"] = [9]
s["note"] = "An external thread spawns task 9  →  GLOBAL injection queue."
s["arrows"] = [("ext",)]
hold(s, 9)

s = clone(s)
s["workers"][1].update(run=8, lifo=None, q=[6])
s["note"] = "LIFO slot drains first: Worker 1 runs task 8 next (cache-hot)."
hold(s, 9)

s = clone(s)
s["glob"] = []
s["workers"][2].update(state="Running", run=9, q=[4])
s["note"] = "Workers periodically check the global queue: Worker 2 takes task 9."
s["arrows"] = [("glob2",)]
hold(s, 10)

s = clone(s)
s["note"] = "Steady state: local queues for speed, LIFO for locality, stealing for balance."
hold(s, 12)


def draw(i):
    ax.clear()
    ax.set_xlim(0, 100)
    ax.set_ylim(0, 44)
    ax.axis("off")
    st = frames[i]
    ax.text(50, 42.4, "Tokio Work-Stealing Scheduler", ha="center",
            va="center", fontsize=15, fontweight="bold", color=INK)

    # global injection queue
    rrect(ax, 50, 36.3, 70, 4.8, LIGHT, SLATE, 1.3, "", fs=9)
    ax.text(15.5, 36.3, "GLOBAL\ninjection", ha="center", va="center",
            fontsize=8.5, color=SLATE, fontweight="bold")
    for j, tid in enumerate(st["glob"]):
        rrect(ax, 35 + j * (TASK_W + 1.6), 36.3, TASK_W, TASK_H,
              INK, INK, 1.0, str(tid), "white", 10, True)

    for wi, w in enumerate(st["workers"]):
        cx = W[wi]
        scol = {"Running": GREEN, "Searching": RED,
                "Parked": GRAY}[w["state"]]
        rrect(ax, cx, 30.0, 26, 3.6, "white", SLATE, 1.3,
              f"Worker {wi}", INK, 11, True)
        ax.text(cx, 27.4, w["state"], ha="center", va="center",
                fontsize=9, color=scol, fontweight="bold")
        # LIFO slot
        ax.text(cx - 14.8, 23.0, "LIFO", ha="right", va="center",
                fontsize=8, color=GRAY, fontweight="bold")
        rrect(ax, cx, 23.0, TASK_W + 0.6, TASK_H + 0.4,
              SLATE if w["lifo"] else "white",
              ACCENT if w["lifo"] else "#CCCCCC",
              1.3 if w["lifo"] else 1.0,
              str(w["lifo"]) if w["lifo"] else "", "white", 10, True)
        # local queue
        ax.text(cx - 14.8, 17.0, "queue", ha="right", va="center",
                fontsize=8, color=GRAY, fontweight="bold")
        ax.text(cx - 11.4, 13.6, "HEAD", ha="center", va="center",
                fontsize=7, color=GRAY)
        ax.text(cx + 12.0, 13.6, "TAIL", ha="center", va="center",
                fontsize=7, color=GRAY)
        for j in range(4):
            occ = j < len(w["q"])
            rrect(ax, cx - 11.4 + j * (TASK_W + 1.4), 17.0, TASK_W, TASK_H,
                  INK if occ else "white", INK if occ else "#CCCCCC",
                  1.0, str(w["q"][j]) if occ else "", "white", 10, True)
        # running cell
        ax.text(cx - 14.8, 9.4, "runs", ha="right", va="center",
                fontsize=8, color=GRAY, fontweight="bold")
        rrect(ax, cx, 9.4, TASK_W + 1.4, TASK_H + 0.6,
              ACCENT if w["run"] else "white",
              ACCENT if w["run"] else "#CCCCCC",
              1.4 if w["run"] else 1.0,
              str(w["run"]) if w["run"] else "idle",
              "white" if w["run"] else "#AAAAAA", 11, True)

    for m in st["moving"]:
        rrect(ax, m["x"], m["y"], TASK_W, TASK_H, "white", ACCENT, 1.6,
              str(m["id"]), ACCENT, 10, True, style="round,pad=0.02")

    for a in st["arrows"]:
        kind = a[0]
        if kind == "spawn0":
            arrow(ax, (W[0], 11.6), (W[0] - 11.4, 15.0), GREEN)
        elif kind == "lifo1":
            arrow(ax, (W[1], 11.6), (W[1], 21.0), SLATE)
        elif kind == "steal":
            ax.add_patch(FancyArrowPatch((W[0] + 12, 19.4),
                         (W[2] - 12, 19.4), arrowstyle="-|>",
                         mutation_scale=18, color=ACCENT, lw=2.2,
                         connectionstyle="arc3,rad=-0.32", zorder=6))
            ax.text(50, 24.4, "steal half", ha="center", va="center",
                    fontsize=9, color=ACCENT, fontweight="bold")
        elif kind == "ext":
            ax.text(50, 40.3, "external spawn", ha="center", va="center",
                    fontsize=8.5, color=GRAY)
            arrow(ax, (50, 39.4), (50, 38.8), GRAY)
        elif kind == "glob2":
            ax.add_patch(FancyArrowPatch((50, 34.0), (W[2], 31.9),
                         arrowstyle="-|>", mutation_scale=16, color=SLATE,
                         lw=2.0, connectionstyle="arc3,rad=0.2", zorder=6))

    ax.text(50, 2.4, st["note"], ha="center", va="center", fontsize=10.5,
            color=INK)
    ax.text(99, 0.4, "model: tokio.rs/blog/2019-10-scheduler", ha="right",
            va="center", fontsize=7.5, color=GRAY, style="italic")


fig, ax = plt.subplots(figsize=(10.6, 4.66), dpi=100)
fig.patch.set_facecolor("white")
anim = FuncAnimation(fig, draw, frames=len(frames), interval=140)
out = "/Users/matthewkim/Developer/tokio-chat/results/scheduler_anim/scheduler.gif"
anim.save(out, writer=PillowWriter(fps=7))
print(f"wrote {out}  ({len(frames)} frames)")
