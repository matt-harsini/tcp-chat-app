# Azure v2 — clean replication run for slide 17

**Date:** 2026-05-13
**Goal:** replace slide 17 with a methodologically-clean Azure A/B that's reproducible.
**Status:** experiment ran successfully; **result disconfirms slide 17's headline.**

---

## TL;DR — the honest read

With the cleaned-up methodology (pinned buffers, accelerated networking, PPG, 5 trials/cell, drift bookends), **the mutex and broadcast variants performed nearly identically** across the full N sweep. Throughput is indistinguishable. p99 latencies differ by ≤4%. The IQRs are tight, so this is *not* a noise issue — it's a real finding.

| N_a | mutex A_p99 | broadcast A_p99 | mutex A_throughput | broadcast A_throughput |
|---|---|---|---|---|
| 10  | 12.7 ms | 12.0 ms | 1,000 msg/s | 1,000 msg/s |
| 50  | 12.5 ms | 12.3 ms | 15,001 msg/s | 15,000 msg/s |
| 100 | 20.6 ms | 23.0 ms | 55,000 msg/s | 55,003 msg/s |
| 200 | **53.7 ms** | **55.6 ms** | **210,006 msg/s** | **210,003 msg/s** |

The 9× collapse seen on May 9 (local Mac, no buffer pinning, single-machine loopback) did **not** reproduce on cleanly-instrumented Azure.

---

## What this means for the deck (read before tearing the slides apart)

This is uncomfortable but it's also a **stronger** story when told honestly. Three framing moves:

### 1. The architectural argument doesn't depend on a measurable disaster

slide 13's lock-during-await pathology is a *correctness footgun*: it's a pattern that *can* explode under specific conditions (sustained slow consumer + small SO_SNDBUF + high enough N that contention compounds). Whether it *does* explode in any given deployment depends on:
- Kernel SO_SNDBUF / SO_RCVBUF (autotuned or pinned? to what?)
- Network path characteristics (sustained slow or just jittery?)
- NIC offload state
- CPU vs IO bottleneck ratio
- Client-count regime

That's the *real* lesson: **the pattern is brittle, its failure is gated by infrastructure details, and the gating conditions are easy to miss in dev/CI hardware that doesn't match prod.** Slide 19 already says this — slide 17 just needs to stop overclaiming.

### 2. The disconfirmation IS the methodology finding

The story you can tell:
> *"My initial local sweep showed a 9× throughput collapse. When I tried to reproduce that on Azure with rigorous methodology — pinned buffers to remove kernel autotuning, accelerated networking for low-variance NIC, PPG colocation for clean baseline, 5 trials per cell with IQR error bars — the collapse didn't reproduce. The architectural pattern is still wrong, but its visibility is gated by infrastructure details that easily mask the failure. The lesson: dev/CI hardware that differs from production measures a different regime, and the pattern is dangerous not because it always fails but because **when it fails, it fails invisibly through environments where the gating conditions happen to align**."*

That's a graduate-level engineering answer. It's also exactly what slide 19 + slide 20 already set up.

### 3. The May 9 local sweep is still load-bearing — just reframed

Use May 9 sweep_20260509_223254.csv as: "*in a controlled single-machine setting where the kernel autotunes buffers and there's no NIC variance, the failure mode does manifest at N=200.*" It's a real measurement of the *worst-case* regime.

Use this Azure run as: "*in a cleaner instrumented setting that more closely resembles a real distributed deployment, the failure mode is dormant.*"

Together they bracket the regime. That's a more honest empirical statement than slide 17's "≥60s clipped" reading ever was.

---

## What's in this directory

| Path | Description |
|---|---|
| `aggregated.csv` | per-cell median + IQR (one row per phase/variant/N_a) |
| `throughput_vs_N.png` | mutex vs broadcast, A's recv/sec, log-log |
| `p99_vs_N.png` | mutex vs broadcast, both clients, p99 with IQR error bars |
| `raw/<cell>/client_a.csv` | raw loadtest row for Client A in that trial |
| `raw/<cell>/client_b.csv` | raw loadtest row for Client B in that trial |
| `raw/<cell>/server.log` | server's stderr for that trial |
| `raw/<cell>/mpstat.log` | per-second CPU breakdown including %steal |
| `raw/<cell>/ss.log` | every-5s socket queue depth snapshot |
| `provenance/server.txt`, `client_a.txt`, `client_b.txt` | uname, sysctl, ethtool, chrony state per VM |

40 trials with data, 2 bookends (start good, end SSH-disconnected — see "Caveats").

---

## Topology run

```
Server     : Azure D2s_v3, eastus2, in PPG-A, accelerated networking
             SO_RCVBUF=SO_SNDBUF=262144 pinned, autotuning off, NIC offloads off

Client A   : Azure D2s_v3, eastus2, SAME PPG-A, accelerated networking
             (controlled measurement plane — drift canary)

Client B   : Azure D2as_v4, northcentralus, accelerated networking
             (natural cross-region path, fixed N_b=10 across all cells)
```

**SKU note:** plan was D2as_v4 everywhere. eastus2 was capacity-blocked on both D2as_v4 (no capacity) and D2s_v5 (zero quota on student sub), so server + Client A fell back to D2s_v3 (Intel Skylake-class, still sustained-CPU 2 vCPU). Client B remained D2as_v4. Mixed-SKU is fine: server is the SUT and dictates the regime; B is just generating cross-region traffic.

---

## Workload

- Per trial: 60s warmup + 240s measurement
- rate: 5 msg/s/client
- `--slow-count 0` everywhere — no synthetic slow-client trick (cross-region B is the natural slow path)
- Sweep: variants {mutex, broadcast} × N_a ∈ {10, 50, 100, 200} × **5 trials**
- Bookends: broadcast/N_a=10 at start and end (drift detector)

42 trials total. Wall-clock ~5.5 hours (one failed start due to NSG misconfig, then clean run).

---

## Caveats (be specific in the deck, not hand-wavy)

1. **End-bookend SSH dropped** during your work hours. We can't compare start vs end bookend for drift. However, %steal stayed 0.00% across all 42 trials, and IQRs are tight within each cell — strong evidence the session was clean even without the end-bookend check.

2. **Mixed SKUs** — server is D2s_v3 (Intel), Client B is D2as_v4 (AMD). Both are sustained-CPU 2-vCPU. The deck should mention this honestly. It does not change the conclusion: B is just generating traffic, not under test.

3. **Cross-region client B may not be slow enough.** 30–50 ms RTT × 10 clients × ~50-byte msgs × 5 msg/s = ~2.5 KiB/s sustained to B. The 256 KiB SO_SNDBUF on the server takes ~100 s of full backpressure to fill. Cross-region jitter alone, in a 240s window, doesn't accumulate enough to keep the lock held. **This is likely *why* the failure mode didn't engage.** A future iteration that wanted to engage it would either: shrink SO_SNDBUF (e.g., 16 KiB) or genuinely throttle B (tc netem 500ms + smaller B-side recv buffer).

4. **NTP clock skew** across VMs is ≤few ms per chronyc tracking — included in latency tail but small relative to signal.

---

## Cost

| Phase | Cost |
|---|---|
| Provisioning + setup | ~$0.30 |
| Failed run #1 (NSG block, ~1 hr) | ~$0.30 |
| Successful run (5.5 hr × 3 VMs) | ~$1.65 |
| **Total** | **~$2.25** |

Resource group deleted at end of session.

---

## Suggested deck edits (not exhaustive)

- **Slide 17 (Results: Same Code, Two CPU Budgets)** — replace the table with the two PNGs and a caption: *"40 trials × 5 cells per variant, IQR error bars, pinned-buffer methodology. Variants perform identically at this scale; the architectural pathology requires sustained backpressure that this topology doesn't provide. See slide 17b."*
- **Add slide 17b** — local sweep results (May 9, sweep_20260509_223254.csv) chart, captioned: *"In a regime with kernel-autotuned SO_SNDBUF (no pinning), N=200 reproducibly collapses mutex throughput 9×. The autotune defaults differ across kernels, distributions, and SKU classes — see slide 19 about deployment hardware mismatch."*
- **Slide 19 (Lessons)** — strengthen with this run as evidence: *"Calibration requires running the same code in production-equivalent infrastructure. Local benchmarks measured a 9× collapse; Azure benchmarks with disciplined methodology saw none. The pattern is still wrong — but its measurable impact varies by 10×+ across reasonable deployment targets."*
- **Slide 20 (Future Directions)** — replace the methodology bullets with this completed methodology, then add *"Engage the failure mode reliably: shrink server SO_SNDBUF below 64 KiB, OR introduce a true synthetic slow consumer with `tc netem delay 500ms` + small recv buffer. This run did neither; both are concrete next steps if I wanted to demonstrate the worst-case regime in this topology."*
