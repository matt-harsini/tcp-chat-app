// Reproduces Gjengset's "Cost of Concurrency Coordination" lock-scaling
// benchmark (Jane Street talk: slide 3 "read a shared counter", slide 7
// "RwLock barely beats Mutex"). Trivial critical section — read a u64 — under
// std::sync::Mutex / std::sync::RwLock (read) / AtomicU64 (load). Sweeps thread
// counts and reports millions of operations per second.
//
// This measures FAILURE MODE 1: coordination cost. The critical section does
// essentially nothing, yet throughput collapses going from 1 -> 2 threads
// because the lock's own cache line ping-pongs between cores. Distinct from
// slide 17's chat-server collapse, which is FAILURE MODE 2: a lock HELD across
// a blocking socket write. Same primitive, different mechanism.
//
// Output: CSV rows to stdout
//   primitive,threads,trial,mops_per_sec
//
// Usage:
//   cargo run --release --bin bench_lock_scaling
//   cargo run --release --bin bench_lock_scaling -- --secs 2 --trials 5

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Primitive {
    Mutex,
    RwLock,
    Atomic,
}

impl Primitive {
    fn name(self) -> &'static str {
        match self {
            Primitive::Mutex => "mutex",
            Primitive::RwLock => "rwlock",
            Primitive::Atomic => "atomic",
        }
    }
}

// All three primitives live here; exactly one is exercised per run. A unified
// struct keeps the worker monomorphic-free without generics gymnastics — the
// two unused primitives just sit idle and cost nothing.
struct Shared {
    m: Mutex<u64>,
    rw: RwLock<u64>,
    a: AtomicU64,
}

fn run_one(prim: Primitive, n_threads: usize, secs: u64) -> f64 {
    let shared = Arc::new(Shared {
        m: Mutex::new(0),
        rw: RwLock::new(0),
        a: AtomicU64::new(0),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    // +1: the main thread waits on the same barrier so every worker is
    // released at the same instant (removes thread-spawn skew).
    let barrier = Arc::new(Barrier::new(n_threads + 1));

    let mut handles = Vec::with_capacity(n_threads);
    for _ in 0..n_threads {
        let shared = shared.clone();
        let stop = stop.clone();
        let total = total.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            // Count thread-locally and sum once at the end, so the measurement
            // harness is not itself a point of contention during the run.
            let mut count: u64 = 0;
            // Check stop only every BATCH iterations so measured work stays
            // dominated by the primitive under test. Critical for the atomic
            // baseline: a per-iteration stop-load would itself double the work.
            const BATCH: u64 = 256;
            loop {
                for _ in 0..BATCH {
                    // prim is loop-invariant and Copy; the branch is perfectly
                    // predicted and the release optimizer hoists it.
                    match prim {
                        Primitive::Mutex => {
                            let g = shared.m.lock().unwrap();
                            black_box(*g);
                        }
                        Primitive::RwLock => {
                            let g = shared.rw.read().unwrap();
                            black_box(*g);
                        }
                        Primitive::Atomic => {
                            black_box(shared.a.load(Ordering::Relaxed));
                        }
                    }
                }
                count += BATCH;
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            total.fetch_add(count, Ordering::Relaxed);
        }));
    }

    barrier.wait();
    let start = Instant::now();
    thread::sleep(Duration::from_secs(secs));
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }
    let elapsed = start.elapsed().as_secs_f64();
    let ops = total.load(Ordering::Relaxed) as f64;
    ops / elapsed / 1e6
}

fn main() {
    let mut secs: u64 = 2;
    let mut trials: usize = 5;
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--secs" => {
                secs = argv[i + 1].parse().unwrap();
                i += 2;
            }
            "--trials" => {
                trials = argv[i + 1].parse().unwrap();
                i += 2;
            }
            _ => panic!("unknown arg: {}", argv[i]),
        }
    }

    let thread_counts = [1usize, 2, 3, 4, 6, 8, 10];
    let prims = [Primitive::Mutex, Primitive::RwLock, Primitive::Atomic];

    eprintln!(
        "[bench_lock_scaling] secs={} trials={} threads={:?}",
        secs, trials, thread_counts
    );
    println!("primitive,threads,trial,mops_per_sec");
    for prim in prims {
        for &n in &thread_counts {
            for trial in 0..trials {
                let mops = run_one(prim, n, secs);
                println!("{},{},{},{:.2}", prim.name(), n, trial, mops);
                eprintln!(
                    "  {:>6} threads={:>2} trial={} -> {:>10.2} M ops/s",
                    prim.name(),
                    n,
                    trial,
                    mops
                );
            }
        }
    }
}
