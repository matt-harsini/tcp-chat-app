// M1: Tokio + Arc<tokio::sync::Mutex<Vec<OwnedWriteHalf>>>
// The pathology being demonstrated: holding a lock across .await on write_all.
// One slow writer stalls every other writer because the lock spans the I/O.

use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, tcp::OwnedWriteHalf},
    sync::Mutex,
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".into());
    let listener = TcpListener::bind(&addr).await?;
    let writers: Arc<Mutex<Vec<Option<OwnedWriteHalf>>>> = Arc::new(Mutex::new(Vec::new()));
    eprintln!("[server_mutex] listening on {}", addr);

    loop {
        let (socket, _) = listener.accept().await?;
        let _ = socket.set_nodelay(true);
        let (reader, writer) = socket.into_split();

        let idx = {
            let mut w = writers.lock().await;
            w.push(Some(writer));
            w.len() - 1
        };

        let writers = writers.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => {
                        let mut w = writers.lock().await;
                        if let Some(slot) = w.get_mut(idx) {
                            *slot = None;
                        }
                        return;
                    }
                    Ok(_) => {
                        // THE PATHOLOGY: lock held across every write_all.
                        let mut w = writers.lock().await;
                        for slot in w.iter_mut() {
                            if let Some(writer) = slot.as_mut() {
                                if writer.write_all(line.as_bytes()).await.is_err() {
                                    *slot = None;
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
