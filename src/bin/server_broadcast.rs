// M2: Tokio + tokio::sync::broadcast.
// Each connection task subscribes to one shared bus.
// Race: read line from socket vs receive line from bus, in select!.
// No locks, no shared mutable state, no head-of-line blocking across writers.

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpSocket},
    sync::broadcast::{self, error::RecvError},
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".into());
    // Pin SO_RCVBUF and SO_SNDBUF on the listening socket. Accepted connections
    // inherit these values, disabling kernel autotuning per-socket. Mirrors the
    // server_mutex.rs buffer pinning so the architectural comparison is on
    // identical kernel-buffer footing.
    let listener: TcpListener = {
        let sock = TcpSocket::new_v4()?;
        sock.set_recv_buffer_size(262144)?;
        // SND buffer pinned to 16 KiB (Phase 11): matches server_mutex /
        // server_threads. Per-subscriber writer task parks independently;
        // no shared lock to cascade through.
        sock.set_send_buffer_size(16384)?;
        let parsed: std::net::SocketAddr = addr.parse().expect("invalid bind addr");
        sock.bind(parsed)?;
        sock.listen(1024)?
    };
    // Large buffer (1M) so transient receiver lag doesn't trigger RecvError::Lagged
    // — that would silently drop messages and confound the architectural comparison.
    let (tx, _) = broadcast::channel::<String>(1_000_000);
    eprintln!("[server_broadcast] listening on {}", addr);

    loop {
        let (socket, _) = listener.accept().await?;
        let _ = socket.set_nodelay(true);
        let (reader, mut writer) = socket.into_split();
        let mut reader = BufReader::new(reader);

        let tx = tx.clone();
        let mut rx = tx.subscribe();

        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                tokio::select! {
                    n = reader.read_line(&mut line) => {
                        match n {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {
                                let _ = tx.send(line.clone());
                                line.clear();
                            }
                        }
                    }
                    msg = rx.recv() => {
                        match msg {
                            Ok(m) => {
                                if writer.write_all(m.as_bytes()).await.is_err() {
                                    return;
                                }
                            }
                            Err(RecvError::Lagged(_)) => continue,
                            Err(RecvError::Closed) => return,
                        }
                    }
                }
            }
        });
    }
}
