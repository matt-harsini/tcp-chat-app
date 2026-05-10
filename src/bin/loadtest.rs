// Load driver. Spawns N clients against a chat server, sends timestamped messages,
// records aggregate throughput and end-to-end latency percentiles.
//
// Usage:
//   cargo run --release --bin loadtest -- \
//       --addr 127.0.0.1:8080 \
//       --clients 100 \
//       --duration-secs 20 \
//       --warmup-secs 3 \
//       --rate-per-client 50 \
//       --label mutex,N=100
//
// Output (one CSV row to stdout):
//   label,clients,duration_s,rate_per_client,
//   sent,received,recv_per_sec,
//   p50_us,p95_us,p99_us,p99_9_us,max_us
//
// Message wire format: "<client_id>:<seq>:<send_micros>\n"

use hdrhistogram::Histogram;
use std::{
    sync::{Arc, atomic::{AtomicU64, Ordering}},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::Mutex,
    time::{interval, sleep},
};

#[derive(Clone, Debug)]
struct Args {
    addr: String,
    clients: usize,
    duration_secs: u64,
    warmup_secs: u64,
    rate_per_client_milli: u64, // 0 = saturate; otherwise rate in milli-msgs/sec (1000 = 1/sec, 100 = 0.1/sec)
    label: String,
}

fn parse_args() -> Args {
    let mut a = Args {
        addr: "127.0.0.1:8080".into(),
        clients: 50,
        duration_secs: 15,
        warmup_secs: 3,
        rate_per_client_milli: 0,
        label: "run".into(),
    };
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" => { a.addr = argv[i+1].clone(); i += 2; }
            "--clients" => { a.clients = argv[i+1].parse().unwrap(); i += 2; }
            "--duration-secs" => { a.duration_secs = argv[i+1].parse().unwrap(); i += 2; }
            "--warmup-secs" => { a.warmup_secs = argv[i+1].parse().unwrap(); i += 2; }
            "--rate-per-client" => {
                // accept floats like 0.1 or integers like 50
                let v: f64 = argv[i+1].parse().unwrap();
                a.rate_per_client_milli = (v * 1000.0).round() as u64;
                i += 2;
            }
            "--label" => { a.label = argv[i+1].clone(); i += 2; }
            _ => panic!("unknown arg: {}", argv[i]),
        }
    }
    a
}

fn now_micros() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as u64
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    let args = parse_args();
    let total_secs = args.warmup_secs + args.duration_secs;
    eprintln!(
        "[loadtest] {} clients={} duration={}s warmup={}s rate/client={}",
        args.label, args.clients, args.duration_secs, args.warmup_secs,
        if args.rate_per_client_milli == 0 {
            "saturate".to_string()
        } else {
            format!("{}m/s", args.rate_per_client_milli)
        }
    );

    // T0 = the moment the warmup window begins. measurement window starts at T0 + warmup.
    let t0 = Instant::now();
    let measure_start = t0 + Duration::from_secs(args.warmup_secs);
    let stop_at = t0 + Duration::from_secs(total_secs);

    let sent = Arc::new(AtomicU64::new(0));
    let received = Arc::new(AtomicU64::new(0));
    let hist: Arc<Mutex<Histogram<u64>>> = Arc::new(Mutex::new(
        Histogram::new_with_bounds(1, 60_000_000, 3).unwrap()
    ));

    let mut handles = Vec::with_capacity(args.clients);
    for cid in 0..args.clients as u32 {
        let addr = args.addr.clone();
        let sent = sent.clone();
        let received = received.clone();
        let hist = hist.clone();
        let rate_milli = args.rate_per_client_milli;
        let h = tokio::spawn(async move {
            let stream = match TcpStream::connect(&addr).await {
                Ok(s) => s,
                Err(e) => { eprintln!("client {} connect failed: {}", cid, e); return; }
            };
            let _ = stream.set_nodelay(true);
            let (rd, mut wr) = stream.into_split();
            let mut rd = BufReader::new(rd);

            // Reader task — measures latency on every received message.
            // Hold its handle so we can ensure it flushes before main collects stats.
            let received_r = received.clone();
            let hist_r = hist.clone();
            let reader_handle = tokio::spawn(async move {
                let mut line = String::new();
                let mut local: Vec<u64> = Vec::with_capacity(8192);
                loop {
                    line.clear();
                    match rd.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            // Parse "<cid>:<seq>:<send_micros>"
                            let trimmed = line.trim_end();
                            let mut parts = trimmed.splitn(3, ':');
                            let _ = parts.next();
                            let _ = parts.next();
                            let send_us: u64 = match parts.next().and_then(|s| s.parse().ok()) {
                                Some(v) => v,
                                None => continue,
                            };
                            let recv_us = now_micros();
                            // Only record after measurement window has begun.
                            if Instant::now() >= measure_start {
                                let lat = recv_us.saturating_sub(send_us);
                                received_r.fetch_add(1, Ordering::Relaxed);
                                local.push(lat.min(60_000_000));
                                // Flush often so low-rate runs still produce histogram data.
                                if local.len() >= 8 {
                                    let mut h = hist_r.lock().await;
                                    for v in local.drain(..) { let _ = h.record(v); }
                                }
                            }
                        }
                    }
                }
                if !local.is_empty() {
                    let mut h = hist_r.lock().await;
                    for v in local.drain(..) { let _ = h.record(v); }
                }
            });

            // Sender loop.
            let mut seq: u32 = 0;
            let mut tick = if rate_milli > 0 {
                // rate_milli = milli-msgs/sec, e.g. 100 = 0.1 msg/sec; period = 1000/rate_milli sec
                let period = Duration::from_secs_f64(1000.0 / rate_milli as f64);
                Some(interval(period))
            } else {
                None
            };
            while Instant::now() < stop_at {
                if let Some(t) = tick.as_mut() {
                    t.tick().await;
                }
                let msg = format!("{}:{}:{}\n", cid, seq, now_micros());
                if wr.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                seq += 1;
                if Instant::now() >= measure_start {
                    sent.fetch_add(1, Ordering::Relaxed);
                }
                // Yield-friendly busy loop in saturate mode
                if rate_milli == 0 && seq % 1024 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            let _ = wr.shutdown().await;
            // Wait for reader to drain (EOF arrives shortly after our shutdown)
            // and flush its remaining samples to the global histogram.
            let _ = reader_handle.await;
        });
        handles.push(h);
    }

    // Wait for the test window to elapse, then for all client tasks to finish
    // (which includes their reader-flush epilogues).
    sleep(Duration::from_secs(total_secs)).await;
    for h in handles {
        let _ = tokio::time::timeout(Duration::from_secs(3), h).await;
    }

    // Collect.
    let sent_n = sent.load(Ordering::Relaxed);
    let recv_n = received.load(Ordering::Relaxed);
    let measure_secs = args.duration_secs as f64;
    let recv_per_sec = recv_n as f64 / measure_secs;
    let h = hist.lock().await;
    let p50 = h.value_at_quantile(0.50);
    let p95 = h.value_at_quantile(0.95);
    let p99 = h.value_at_quantile(0.99);
    let p999 = h.value_at_quantile(0.999);
    let pmax = h.max();

    println!(
        "{},{},{},{},{},{},{:.1},{},{},{},{},{}",
        args.label, args.clients, args.duration_secs, args.rate_per_client_milli,
        sent_n, recv_n, recv_per_sec,
        p50, p95, p99, p999, pmax
    );
    Ok(())
}
