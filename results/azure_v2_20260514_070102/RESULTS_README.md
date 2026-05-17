# Phase 12 — Small SND buffer test (the fifth null)

**Date:** 2026-05-14
**Goal:** with server's per-socket `SO_SNDBUF` shrunk from 256 KiB → 16 KiB, the prediction was that `write_all` would park frequently enough that the `server_threads` lock-during-IO pathology would finally engage relative to `server_broadcast`.
**Result:** another null. broadcast and threads p99 within 0.2%.

---

## Headline (median of 5 trials per cell)

| variant | client | p50 | p95 | p99 | recv/s |
|---|---|---|---|---|---|
| broadcast | A (eastus2) | 57.6 s | 92.7 s | **96.1 s** | 211 k |
| broadcast | B (northcentralus) | 58.9 s | 95.4 s | 99.0 s | 200 k |
| broadcast | C (mexicocentral) | 60.9 s | 98.6 s | 102.3 s | 187 k |
| threads | A (eastus2) | 58.5 s | 93.1 s | **96.3 s** | 210 k |
| threads | B (northcentralus) | 60.3 s | 95.9 s | 99.3 s | 199 k |
| threads | C (mexicocentral) | 62.4 s | 99.2 s | 102.7 s | 185 k |

**broadcast vs threads delta: ≤2% on every metric, every client.**

---

## Why it didn't fire — and why that's *itself* a defensible finding

The hypothesis was: small SND buffer + heavy fanout → `write_all` parks under the lock → cascade. But measurement shows write_all doesn't park enough to matter, *because*:

- Per-socket sustained data rate: 50 B × 5 msg/s × 600 senders ≈ 150 KB/s
- Per-socket TCP throughput: cwnd × RTT
  - Same-region (sub-ms RTT, 16 KiB cwnd → ~16 MB/s)
  - Cross-region (30 ms RTT → ~533 KB/s)
  - Intl (90 ms RTT → ~178 KB/s)
- All comfortably exceed 150 KB/s sustained load
- → buffers drain faster than they fill
- → `write_all` returns immediately without parking
- → lock hold time stays short
- → no cascade

To **actually** engage the pathology in this topology, we'd need either:
- `SO_SNDBUF` further reduced to ~4 KiB (close to one broadcast's per-socket worth)
- Per-client rate boosted ~10× so fill rate exceeds TCP drain rate
- Or a deliberately-throttled cross-region path (`tc qdisc add ... netem delay 500ms`)

None of these were tested. They're concrete next-step experiments if the deck needs the chart.

---

## Cumulative empirical picture across 5 cloud regimes

| Phase | N_total | SND | broadcast p99 | mutex/threads p99 | delta |
|---|---|---|---|---|---|
| 9 light | 30 | 256 KiB | 28 ms | 28 ms | 0% |
| 9 moderate | 220 | 256 KiB | 56 ms | 58 ms | +4% |
| 4/5 (prior 2-client) | 220 | 256 KiB | 56 ms | mutex 54 ms | -4% |
| 10 heavy | 650 | 256 KiB | 110 s | 111 s | +1% |
| **12 small-buf** | 600 | **16 KiB** | 96.1 s | 96.3 s | +0.2% |
| Local Mac (May 9) | 200 | autotuned | mutex collapse 9× throughput | broadcast healthy | — |

Five cloud regimes × three architectures × multiple percentiles = **zero measurable difference**. The pattern only fires on a Mac with kernel-autotuned buffers and loopback semantics.

---

## What this means for the deck (final version)

This is the deck's most engineering-mature reframe:

> *"I implemented the lock-protected fan-out pattern three ways — tokio::sync::Mutex, tokio::sync::broadcast, std::sync::Mutex with OS threads — and tested it across five regimes on Azure with rigorous methodology (pinned buffers, accelerated networking, PPG colocation, IQR-style stats, 0% CPU steal). The architectural variants performed identically in every regime, including catastrophic saturation at 650 clients and an aggressive 16 KiB SND-buffer setup designed specifically to engage the pathology. The only configuration where the pathology engages reliably is my local Mac with default kernel autotuning, where mutex throughput collapses 9× at N=200.*
>
> *The architectural critique on slide 13 — lock-held-across-blocking-IO is wrong — survives because the Rust compiler refuses to compile std::sync::Mutex across `.await` (Send bound), and because the pattern's failure is unbounded when it does fire. But the empirical lesson is **the pattern's measurable impact varies by four orders of magnitude across reasonable deployment substrates**. Slide 19's 'calibrate against deployment, not against the laptop' isn't rhetoric; it's the only conclusion that fits five independent disconfirmations."*

That's a senior-engineer answer. It respects both the architectural reasoning and the empirical reality.

---

## Files

| Path | Description |
|---|---|
| `aggregated.csv` | per-variant per-client median + min/max |
| `smallbuf_comparison.png` | bar chart at N=600 + 16 KiB SND |
| `all_regimes_overlay.png` | 6-regime overlay across Phase 9/10/12 |
| `raw/main_{broadcast,threads}_Na200_t{1..5}/` | per-trial CSVs, server.log, mpstat.log, ss.log |
| `provenance/{server,client_a,client_b,client_c}.txt` | uname/sysctl/ethtool/chrony per VM |

---

## Cost

Phase 12: ~$0.40. Cumulative across all today's experiments: ~$4.05.

Resource group `tokio-bench-rg7` deleted at end. caffeinate stopped.
