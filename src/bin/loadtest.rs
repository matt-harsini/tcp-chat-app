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
    net::TcpSocket,
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
    slow_count: usize, // first K clients connect but never read — exposes head-of-line blocking
    label: String,
}

fn parse_args() -> Args {
    let mut a = Args {
        addr: "127.0.0.1:8080".into(),
        clients: 50,
        duration_secs: 15,
        warmup_secs: 3,
        rate_per_client_milli: 0,
        slow_count: 0,
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
            "--slow-count" => { a.slow_count = argv[i+1].parse().unwrap(); i += 2; }
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
    // Bounds: 1 µs to 300 s. Earlier 60s ceiling clipped real mutex-collapse tails
    // (slide 17's "≥60 s" reading was the clip, not the truth).
    let hist: Arc<Mutex<Histogram<u64>>> = Arc::new(Mutex::new(
        Histogram::new_with_bounds(1, 3_600_000_000, 3).unwrap()
    ));

    let mut handles = Vec::with_capacity(args.clients);
    let slow_count = args.slow_count;
    for cid in 0..args.clients as u32 {
        let addr = args.addr.clone();
        let sent = sent.clone();
        let received = received.clone();
        let hist = hist.clone();
        let rate_milli = args.rate_per_client_milli;
        let is_slow = (cid as usize) < slow_count;
        let h = tokio::spawn(async move {
            // For slow clients, shrink the recv buffer so it fills almost
            // immediately at any test rate. Active clients use the default.
            let stream = if is_slow {
                let sock = match TcpSocket::new_v4() {
                    Ok(s) => s,
                    Err(e) => { eprintln!("client {} new_v4: {}", cid, e); return; }
                };
                let _ = sock.set_recv_buffer_size(4096);
                let parsed: std::net::SocketAddr = addr.parse().unwrap();
                match sock.connect(parsed).await {
                    Ok(s) => s,
                    Err(e) => { eprintln!("slow client {} connect failed: {}", cid, e); return; }
                }
            } else {
                // Active clients: pin SO_RCVBUF and SO_SNDBUF to remove kernel
                // autotuning as a confounding variable. 256 KiB is large enough
                // that fast clients never see backpressure under normal rates.
                let sock = match TcpSocket::new_v4() {
                    Ok(s) => s,
                    Err(e) => { eprintln!("client {} new_v4: {}", cid, e); return; }
                };
                let _ = sock.set_recv_buffer_size(262144);
                let _ = sock.set_send_buffer_size(262144);
                let parsed: std::net::SocketAddr = addr.parse().unwrap();
                match sock.connect(parsed).await {
                    Ok(s) => s,
                    Err(e) => { eprintln!("client {} connect failed: {}", cid, e); return; }
                }
            };
            let _ = stream.set_nodelay(true);
            let (rd, mut wr) = stream.into_split();
            let mut rd = BufReader::new(rd);

            // Slow client (Option B, slow-read variant):
            //
            // Bypass BufReader entirely and drain the kernel RCV buffer at a
            // throttled rate using raw async read(). Hardcoded slow-rate: ~10
            // reads/sec. Each read pulls up to 128 bytes (~2 msgs) from kernel.
            //
            // Effect: client's kernel RCV buffer fills (small SO_RCVBUF=4 KiB),
            // rwnd → 0 advertised to server, server's per-socket SND fills,
            // server's write_all parks. Under server_mutex / server_threads
            // that park happens while the lock is held → cascading delay for
            // all other broadcasts queued on the lock. Under server_broadcast
            // each subscriber parks independently — no shared lock to gate.
            //
            // BufReader was a footgun: it prefetches up to 8 KiB per syscall,
            // which drains the kernel RCV in one shot regardless of how slowly
            // the application then consumes lines. Real backpressure requires
            // throttling the read() syscall itself, not the line consumption.
            if is_slow {
                use tokio::io::AsyncReadExt;
                let mut raw_rd = rd.into_inner();
                let slow_period = Duration::from_millis(100); // ~10 reads/sec
                let mut buf = [0u8; 128];
                // Bug fix: check stop_at on every iteration so the slow client
                // doesn't infinite-loop past the measurement window and stall
                // the loadtest main thread's handle-join phase.
                while Instant::now() < stop_at {
                    match raw_rd.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => { sleep(slow_period).await; }
                    }
                }
                let _ = wr.shutdown().await;
                return;
            }

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
                                local.push(lat.min(3_600_000_000));
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
        // Reader epilogue can take many seconds to drain under mutex-collapse.
        // Killing it early would drop exactly the tail samples we want to record.
        let _ = tokio::time::timeout(Duration::from_secs(60), h).await;
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
