// Synthetic microbenchmark — slide 17 replacement.
//
// Isolates the lock-held-across-await pathology by removing the network
// entirely. `write_all().await` is replaced with `sleep(slow_delay).await`
// on designated slow subscribers; everything else mirrors server_mutex.rs
// and server_broadcast.rs structurally.
//
// The architectural claim under test is structural, not substrate-dependent:
// holding `tokio::sync::Mutex` across an `.await` that parks serializes every
// lock-acquiring task behind whatever that await waits for. The lock doesn't
// know whether it's waiting on TCP, disk, a channel, or `sleep` — they're
// all the same to it. This bench demonstrates that directly, with no kernel,
// buffer, or RTT confound to argue about.
//
// Usage:
//   cargo run --release --bin bench_pathology -- \
//       --publishers 8 --subscribers 200 --slow-count 1 \
//       --slow-delay-ms 10 --pub-rate-hz 200 --duration-secs 10
//
// Output (CSV to stdout):
//   variant,p50_us,p95_us,p99_us,p99_9_us,max_us,samples

use hdrhistogram::Histogram;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast};
use tokio::time::{interval, sleep};

#[derive(Clone)]
struct Args {
    variant: String,
    publishers: usize,
    subscribers: usize,
    slow_count: usize,
    slow_delay_ms: u64,
    pub_rate_hz: u64,
    duration_secs: u64,
}

fn parse_args() -> Args {
    let mut a = Args {
        variant: "both".into(),
        publishers: 8,
        subscribers: 200,
        slow_count: 1,
        slow_delay_ms: 10,
        pub_rate_hz: 200,
        duration_secs: 10,
    };
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--variant" => { a.variant = argv[i+1].clone(); i += 2; }
            "--publishers" => { a.publishers = argv[i+1].parse().unwrap(); i += 2; }
            "--subscribers" => { a.subscribers = argv[i+1].parse().unwrap(); i += 2; }
            "--slow-count" => { a.slow_count = argv[i+1].parse().unwrap(); i += 2; }
            "--slow-delay-ms" => { a.slow_delay_ms = argv[i+1].parse().unwrap(); i += 2; }
            "--pub-rate-hz" => { a.pub_rate_hz = argv[i+1].parse().unwrap(); i += 2; }
            "--duration-secs" => { a.duration_secs = argv[i+1].parse().unwrap(); i += 2; }
            _ => panic!("unknown arg: {}", argv[i]),
        }
    }
    a
}

struct Sub { is_slow: bool }

// MUTEX variant — the pathology.
//
//   let s = subs.lock().await;
//   for sub in s.iter() {
//       if sub.is_slow { sleep(slow_delay).await; }  // <-- park under lock
//       else { record_latency(...); }
//   }
//
// Identical shape to server_mutex.rs's broadcast loop. The `sleep` stands in
// for `write_all` parking on a slow client's full SND buffer.
async fn run_mutex(args: &Args) -> Histogram<u64> {
    let subs: Arc<Mutex<Vec<Sub>>> = Arc::new(Mutex::new(
        (0..args.subscribers)
            .map(|i| Sub { is_slow: i < args.slow_count })
            .collect()
    ));
    let hist = Arc::new(Mutex::new(
        Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).unwrap()
    ));
    let slow_delay = Duration::from_millis(args.slow_delay_ms);
    let per_pub_rate = (args.pub_rate_hz as f64) / (args.publishers as f64);
    let stop = Instant::now() + Duration::from_secs(args.duration_secs);

    let mut handles = vec![];
    for _ in 0..args.publishers {
        let subs = subs.clone();
        let hist = hist.clone();
        handles.push(tokio::spawn(async move {
            let period = Duration::from_secs_f64(1.0 / per_pub_rate);
            let mut tick = interval(period);
            let mut local: Vec<u64> = Vec::with_capacity(8192);
            while Instant::now() < stop {
                tick.tick().await;
                if Instant::now() >= stop { break; }
                let send_t = Instant::now();
                let s = subs.lock().await;
                for sub in s.iter() {
                    if sub.is_slow {
                        sleep(slow_delay).await;       // park under lock
                    } else {
                        let lat = send_t.elapsed().as_micros() as u64;
                        local.push(lat.min(60_000_000));
                    }
                }
                drop(s); // release before any histogram flush
            }
            let mut h = hist.lock().await;
            for v in local.drain(..) { let _ = h.record(v); }
        }));
    }
    for h in handles { let _ = h.await; }
    hist.lock().await.clone()
}

// BROADCAST variant — the fix.
//
//   per subscriber:  while let Ok((_, sent)) = rx.recv().await { ... }
//   publisher:       tx.send((seq, Instant::now()));   // O(1), no lock
//
// Each subscriber owns a task and a queue. The slow subscriber's sleep
// is contained to its own task and doesn't block any other subscriber.
// Structurally an actor model: subscriber = task + mailbox.
async fn run_broadcast(args: &Args) -> Histogram<u64> {
    let (tx, _) = broadcast::channel::<(u64, Instant)>(4096);
    let hist = Arc::new(Mutex::new(
        Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).unwrap()
    ));
    let slow_delay = Duration::from_millis(args.slow_delay_ms);

    let mut sub_handles = vec![];
    for i in 0..args.subscribers {
        let is_slow = i < args.slow_count;
        let mut rx = tx.subscribe();
        let hist = hist.clone();
        sub_handles.push(tokio::spawn(async move {
            let mut local: Vec<u64> = Vec::with_capacity(8192);
            loop {
                match rx.recv().await {
                    Ok((_seq, sent_at)) => {
                        if is_slow {
                            sleep(slow_delay).await;
                        } else {
                            let lat = sent_at.elapsed().as_micros() as u64;
                            local.push(lat.min(60_000_000));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            let mut h = hist.lock().await;
            for v in local.drain(..) { let _ = h.record(v); }
        }));
    }

    let stop = Instant::now() + Duration::from_secs(args.duration_secs);
    let per_pub_rate = (args.pub_rate_hz as f64) / (args.publishers as f64);
    let mut pub_handles = vec![];
    for _ in 0..args.publishers {
        let tx = tx.clone();
        pub_handles.push(tokio::spawn(async move {
            let period = Duration::from_secs_f64(1.0 / per_pub_rate);
            let mut tick = interval(period);
            let mut seq = 0u64;
            while Instant::now() < stop {
                tick.tick().await;
                if Instant::now() >= stop { break; }
                let _ = tx.send((seq, Instant::now()));
                seq += 1;
            }
        }));
    }
    for h in pub_handles { let _ = h.await; }
    drop(tx); // signal subscribers to exit
    for h in sub_handles { let _ = h.await; }
    hist.lock().await.clone()
}

fn print_row(label: &str, h: &Histogram<u64>) {
    println!(
        "{},{},{},{},{},{},{}",
        label,
        h.value_at_quantile(0.50),
        h.value_at_quantile(0.95),
        h.value_at_quantile(0.99),
        h.value_at_quantile(0.999),
        h.max(),
        h.len(),
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args = parse_args();
    eprintln!(
        "[bench] publishers={} subscribers={} slow_count={} slow_delay_ms={} pub_rate_hz={} duration_secs={}",
        args.publishers, args.subscribers, args.slow_count, args.slow_delay_ms,
        args.pub_rate_hz, args.duration_secs
    );
    println!("variant,p50_us,p95_us,p99_us,p99_9_us,max_us,samples");

    if args.variant == "mutex" || args.variant == "both" {
        let h = run_mutex(&args).await;
        print_row("mutex", &h);
    }
    if args.variant == "broadcast" || args.variant == "both" {
        let h = run_broadcast(&args).await;
        print_row("broadcast", &h);
    }
}
