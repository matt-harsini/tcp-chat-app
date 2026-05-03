use std::{collections::HashMap, env::args};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self, error::RecvError},
        mpsc,
    },
};

enum RouterCommand {
    Join {
        name: String,
        mailbox: mpsc::Sender<String>,
    },
    Leave {
        name: String,
    },
    Broadcast {
        from: String,
        line: String,
    },
    Direct {
        from: String,
        to: String,
        line: String,
    },
}

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
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let stdin = tokio::io::stdin();
                let mut stdin_reader = BufReader::new(stdin);
                let mut stdin_line = String::new();
                let mut net_line = String::new();
                loop {
                    tokio::select! {
                        _ = stdin_reader.read_line(&mut stdin_line) => {
                            if let Err(_) = writer.write_all(stdin_line.as_bytes()).await {
                                return Ok(());
                            }
                            stdin_line.clear();
                        }
                        n = reader.read_line(&mut net_line) => {
                            match n {
                                Ok(0) => return Ok(()),
                                Ok(_) => {
                                    print!("{}", net_line);
                                    net_line.clear();
                                },
                                Err(_) => return Ok(())
                            }
                        }
                    }
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
    if let Err(_) = writer.write_all(b"Username: ").await {
        return;
    }
    if let Err(_) = reader.read_line(&mut line).await {
        return;
    }
    let username = line.trim().to_string();
    line.clear();
    loop {
        tokio::select! {
            n = reader.read_line(&mut line) => {
                match n {
                    Ok(0) => {
                        return;
                    }
                    Ok(_) => {
                        if line.trim() == "/quit" {
                            return;
                        }
                        let _ = sender.send(format!("{}: {}", username, line));
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

async fn router(mut rx: mpsc::Receiver<RouterCommand>) {
    let mut map: HashMap<String, mpsc::Sender<String>> = HashMap::new();
    while let Some(cmd) = rx.recv().await {
        match cmd {
            RouterCommand::Join { name, mailbox } => {
                map.insert(name, mailbox);
            }
            RouterCommand::Leave { name } => {
                map.remove(&name);
            }
            RouterCommand::Broadcast { from, line } => {
                for (name, v) in &map {
                    if name == &from {
                        continue;
                    }
                    if let Err(e) = v.try_send(format!("{}: {}", from, line)) {
                        println!("{:?}", e);
                    }
                }
            }
            RouterCommand::Direct { from, to, line } => {
                if let Some(sender) = map.get(&to) {
                    if let Err(e) = sender.send(format!("{}: {}", from, line)).await {
                        println!("{:?}", e);
                    }
                }
            }
        }
    }
}
