# Benchmark notes — Mutex vs Broadcast

## What was measured

Two server variants of the chat fan-out workload, identical Tokio runtime, identical wire protocol:

- **`server_mutex`**: `Arc<tokio::sync::Mutex<Vec<OwnedWriteHalf>>>`. Lock held across `write_all().await` for every recipient (the canonical anti-pattern).
- **`server_broadcast`**: `tokio::sync::broadcast::Sender<String>` + per-task `subscribe()` + `select!` — what M2 of the project replaced M1 with.

Same load driver (`src/bin/loadtest.rs`) against both, sweeping N ∈ {10, 25, 50, 100, 200, 400}.

## Two sweeps

### 1. Throughput sweep — open-loop saturation
- Each client sends as fast as it can.
- Metric: aggregate `deliveries / second` across all clients (i.e. fan-out work the server actually delivered).
- File: `results/sweep_*.csv` and `results/sweep_*_throughput.png`.

### 2. Latency sweep — sub-saturation
- Each client sends at 0.1 msgs/sec (well below capacity for both variants at all N).
- Metric: `p99 end-to-end latency` (time from send-by-A to receive-by-B).
- File: `results/latency_*.csv` and `results/latency_*_p99_latency.png`.

The reason for two separate sweeps: **open-loop saturation latencies are queue-depth artifacts**, not signal. To measure tail latency cleanly, you need to be safely below saturation so the only delay is propagation cost.

### Latency findings (rate=0.1 msgs/sec/client)

| N   | Mutex p99 | Broadcast p99 |
|-----|-----------|---------------|
| 10  | 1.53 ms   | **1.05 ms**   |
| 25  | **4.09 ms** | 4.92 ms     |
| 50  | **15.02 ms** | 22.19 ms   |
| 100 | 74.43 ms  | **57.70 ms**  |
| 200 | **338 ms**  | 555 ms      |
| 400 | 1027 ms   | **927 ms**    |

At sub-saturation load, **both variants have similar p99 latency that grows with N** — both pay O(N) per fan-out (mutex: N serial writes under lock; broadcast: N receiver-task wakeups). The lock-contention advantage of broadcast only really matters at **saturation**, where the mutex's critical section starves all other senders. That's the throughput chart.

This is honest and worth saying on the slide deck: *the architectural advantage isn't free latency — it's resilience under load.*

## What the data says (throughput sweep)

| N   | Mutex (del/s) | Broadcast (del/s) |
|-----|---------------|-------------------|
| 10  | 283,421       | 79,734            |
| 25  | 121,247       | 131,326           |
| 50  | 183,845       | 165,040           |
| 100 | 180,885       | 194,948           |
| 200 | **33,457**    | 301,840           |
| 400 | 38,751        | 192,376           |

**The story:**

- At low N (10), mutex is **faster** than broadcast — uncontended `lock + tight serial-write loop` is efficient and broadcast machinery (atomic ops, waking N receiver tasks) has overhead.
- At medium N (25–100), they trade — both around 130–200k deliveries/sec.
- At N=200, **mutex collapses** from 181k to 33k — a 5.4× drop. Lock contention dominates as critical-section hold time grows linearly with N.
- At N=400, broadcast slightly degrades too (likely loadtest receiver bottleneck), but is still **5× the mutex's throughput**.

**Headline for the slide (calibrated, defensible):**

> "Mutex variant collapses at N=200 — broadcast sustains 5–9× higher throughput once lock-protected fan-out hold-time dominates."

This is sharper than "10× at 400 clients" because it identifies the **inflection point** (N≈100→200) and explains *why* the curve breaks.

## Honest disclosure beats

The story has nuance worth owning on the slide:

- **Mutex is faster at very low N.** Don't hide this. Frame as: *"uncontended single-writer is fast; the architectural cost only shows up at scale."* This reads as engineering honesty and pre-empts "but won't broadcast always be slower for small N?" from the audience.
- **Loadtest is single-process on the same box** — at very high N, the loadtest's receiver tasks are also competing for the runtime. The chart shape (mutex collapse vs broadcast stability) is the durable signal; the absolute peak numbers should be quoted with this caveat.

## How to drop into the deck

The chart in the slide is a `lineChart` with X = [10, 25, 50, 100, 200, 400] and two series. Two options:

1. **Native chart edit** (preserves PowerPoint editability):
   - Right-click the chart → Edit Data → Edit Data in Excel.
   - Replace the placeholder column values with real numbers from `results/sweep_*.csv` (`recv_per_sec` column).

2. **PNG replace** (faster, less editable):
   - Delete the chart object on the slide.
   - Insert `results/sweep_*_throughput.png` in its place.
   - The PNG is rendered with the same steel-blue accent palette as the deck.

## Limitations to acknowledge in Q&A

If asked about the methodology:

- **Single-machine, loopback TCP.** No real network jitter. Broadcast advantages would be even larger over real network because per-receiver write times vary more.
- **Saturation-style throughput measurement** conflates "server capacity" with "end-to-end pipeline capacity." A more rigorous measurement would instrument the server directly to count `write_all` completions per second, removing the loadtest from the measurement path.
- **No "rogue slow consumer" scenario yet.** That's the killer demo for head-of-line blocking — one slow client + N normal clients should send mutex p99 to seconds while leaving broadcast unaffected. Worth running before the interview if time allows.

## Files

- `src/bin/server_mutex.rs` — M1 variant (Tokio + Mutex)
- `src/bin/server_broadcast.rs` — M2 variant (broadcast)
- `src/bin/loadtest.rs` — N-client driver, hdrhistogram, CSV out
- `scripts/run_sweep.sh` — throughput sweep
- `scripts/run_latency_sweep.sh` — latency sweep
- `scripts/plot.py` — chart rendering with deck palette
- `results/sweep_*_throughput.png` — **the slide chart**
- `results/sweep_*_p99_latency.png` — bonus latency chart
- `results/sweep_*_summary.txt` — text table for slide notes
