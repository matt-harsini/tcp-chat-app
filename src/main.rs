use std::env::args;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");
    let args: Vec<String> = args().collect();
    if args.len() > 1 {
        let mode = &args[1];
        println!("{} started...", mode);
        match mode.as_str() {
            "server" => {
                let (sender, _) = broadcast::channel(128);
                let listener = TcpListener::bind("127.0.0.1:8080").await?;
                println!("Server running on 127.0.0.1:8080");
                loop {
                    let (socket, addr) = listener.accept().await?;
                    println!("New connection from: {}", addr);
                    let sender = sender.clone();
                    let mut receiver = sender.subscribe();
                    tokio::spawn(async move {
                        let mut buf = [0; 1024];
                        let (mut reader, mut writer) = socket.into_split();
                        loop {
                            tokio::select! {
                                n = reader.read(&mut buf) => {
                                    match n {
                                        Ok(0) => {
                                            return;
                                        }
                                        Ok(n) => {
                                            let _ = match sender.send(buf[..n].to_vec()) {
                                                Ok(n) => n,
                                                Err(_) => 0,
                                            };
                                        }
                                        Err(e) => {
                                            println!("Err {:?}", e);
                                            return;
                                        }
                                    }
                                }
                                msg = receiver.recv() => {
                                    writer.write_all(&msg.unwrap()).await;
                                }
                            }
                        }
                    });
                }
            }
            "client" => {
                let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
                let message = &args[2];
                stream.write_all(String::as_bytes(message)).await?;
                let mut buf = [0; 1024];
                loop {
                    stream.read(&mut buf).await?;
                    println!("{:?}", String::from_utf8(buf.to_vec()));
                }
            }
            _ => (),
        }
    }
    Ok(())
}
