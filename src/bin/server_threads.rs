// M1-threads: plain OS threads + std::sync::Mutex<Vec<TcpStream>>.
// Same pathology as M1-tokio (server_mutex.rs): the std::sync::Mutex guard
// is held across the blocking write_all() loop, so one slow writer stalls
// every other writer waiting on the lock.
//
// Why this variant exists: tests whether the failure mode is Tokio-specific
// or purely architectural. If this variant collapses at equal or lower N
// than the Tokio variant, it proves the runtime isn't the gating factor —
// the lock-held-during-blocking-IO pattern is.

use socket2::{Domain, Socket, Type};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

fn main() -> std::io::Result<()> {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".into())
        .parse()
        .expect("invalid bind addr");

    // Pin SO_RCVBUF and SO_SNDBUF on the listening socket. Accepted sockets
    // inherit these values, disabling kernel autotuning. Mirrors the Tokio
    // variants (server_mutex.rs, server_broadcast.rs) for like-for-like
    // architectural comparison.
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.set_recv_buffer_size(262144)?;
    // SND buffer pinned to 16 KiB (Phase 11): write_all parks while mutex
    // is held → other threads queue → cascading lock-hold time.
    socket.set_send_buffer_size(16384)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let listener: TcpListener = socket.into();

    let writers: Arc<Mutex<Vec<Option<TcpStream>>>> = Arc::new(Mutex::new(Vec::new()));
    eprintln!("[server_threads] listening on {}", addr);

    for incoming in listener.incoming() {
        let socket = match incoming {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = socket.set_nodelay(true);

        // try_clone() dups the FD so reader thread and writer Vec share one
        // kernel socket. Read on one side, write on the other.
        let write_clone = match socket.try_clone() {
            Ok(s) => s,
            Err(_) => continue,
        };

        let idx = {
            let mut w = writers.lock().unwrap();
            w.push(Some(write_clone));
            w.len() - 1
        };

        let writers = writers.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(socket);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let mut w = writers.lock().unwrap();
                        if let Some(slot) = w.get_mut(idx) {
                            *slot = None;
                        }
                        return;
                    }
                    Ok(_) => {
                        // THE PATHOLOGY: std::sync::Mutex guard held across
                        // every blocking write_all(). One slow writer stalls
                        // all other producers waiting on the lock.
                        let mut w = writers.lock().unwrap();
                        for slot in w.iter_mut() {
                            if let Some(writer) = slot.as_mut() {
                                if writer.write_all(line.as_bytes()).is_err() {
                                    *slot = None;
                                }
                            }
                        }
                    }
                }
            }
        });
    }
    Ok(())
}
