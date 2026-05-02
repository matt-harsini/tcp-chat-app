use std::env::args;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::broadcast::{self, error::RecvError},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = args().collect();
    if args.len() > 1 {
        let mode = &args[1];
        println!("{} started...", mode);
        match mode.as_str() {
            "server" => {
                let (sender, _) = broadcast::channel::<String>(128);
                let listener = TcpListener::bind("127.0.0.1:8080").await?;
                println!("Server running on 127.0.0.1:8080");
                loop {
                    let (socket, addr) = listener.accept().await?;
                    println!("New connection from: {}", addr);
                    tokio::spawn(handle_connection(socket, sender.clone()));
                }
            }
            "client" => {
                let stream = TcpStream::connect("127.0.0.1:8080").await?;
                let mut line = String::new();
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let message = &args[2];
                writer
                    .write_all(format!("{}\n", message).as_bytes())
                    .await?;
                loop {
                    reader.read_line(&mut line).await?;
                    println!("{:?}", line.trim());
                    line.clear();
                }
            }
            _ => (),
        }
    }
    Ok(())
}

async fn handle_connection(socket: TcpStream, sender: broadcast::Sender<String>) {
    let mut receiver = sender.subscribe();
    let (reader, mut writer) = socket.into_split();
    let mut line = String::new();
    let mut reader = BufReader::new(reader);
    loop {
        tokio::select! {
            n = reader.read_line(&mut line) => {
                match n {
                    Ok(0) => {
                        return;
                    }
                    Ok(_) => {
                        let _ = sender.send(line.clone());
                        line.clear();
                    }
                    Err(e) => {
                        println!("Err {:?}", e);
                        return;
                    }
                }
            }
            msg = receiver.recv() => {
                match msg {
                    Ok(msg) => {
                        if let Err(_) = writer.write_all(msg.as_bytes()).await {
                            return;
                        }
                    }
                    Err(n) => {
                        match n {
                            RecvError::Lagged(n) => {
                                println!("Lagged by {} messages", n);
                            },
                            RecvError::Closed => return,
                        }
                    }
                }
            }
        }
    }
}
