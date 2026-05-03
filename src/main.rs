use std::{collections::HashMap, env::args};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc,
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
                let listener = TcpListener::bind("127.0.0.1:8080").await?;
                println!("Server running on 127.0.0.1:8080");
                let (cmd_tx, cmd_rx) = mpsc::channel::<RouterCommand>(128);
                tokio::spawn(router(cmd_rx));
                loop {
                    let (socket, addr) = listener.accept().await?;
                    println!("New connection from: {}", addr);
                    tokio::spawn(handle_connection(socket, cmd_tx.clone()));
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

async fn handle_connection(socket: TcpStream, sender: mpsc::Sender<RouterCommand>) {
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
    let (tx, mut rx) = mpsc::channel::<String>(64);
    if let Err(e) = sender
        .send(RouterCommand::Join {
            name: username.clone(),
            mailbox: tx,
        })
        .await
    {
        println!("{:?}", e);
        return;
    }
    loop {
        tokio::select! {
            n = reader.read_line(&mut line) => {
                match n {
                    Ok(0) => {
                        if let Err(e) = sender.send(RouterCommand::Leave {name: username.clone()}).await {
                            println!("{}", e);
                            return;
                        }
                        return;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed == "/quit" {
                            if let Err(e) = sender.send(RouterCommand::Leave { name: username.clone() }).await {
                                println!("{}", e);
                            }
                            line.clear();
                            return;
                        } else if let Some(rest) = trimmed.strip_prefix("/dm ") {
                            let mut parts = rest.splitn(2, ' ');
                            if let Some((to, msg)) = parts.next().zip(parts.next()) {
                                println!("{}, {}", to, msg);
                                if let Err(e) = sender.send(RouterCommand::Direct {
                                    from: username.clone(),
                                    to: to.to_string(),
                                    line: msg.to_string(),
                                }).await {
                                    println!("{}", e);
                                }
                            }
                        } else {
                            if let Err(e) = sender.send(RouterCommand::Broadcast {
                                from: username.clone(),
                                line: line.clone(),
                            }).await {
                                println!("{}", e);
                                return;
                            }
                        }
                        line.clear();
                    }
                    Err(e) => {
                        println!("Err {:?}", e);
                        if let Err(e) = sender.send(RouterCommand::Leave {name: username.clone()}).await {
                            println!("{}", e);
                            return;
                        }
                        return;
                    }
                }
            }
            msg = rx.recv() => {
                match msg {
                   Some(msg) => {
                        if let Err(_) = writer.write_all(msg.as_bytes()).await {
                            return;
                        }
                    }
                    None => {
                        if let Err(e) = sender.send(RouterCommand::Leave {name: username.clone()}).await {
                            println!("{}", e);
                            return;
                        }
                        return;
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
                    if let Err(e) = v.try_send(format!("{}: {}\n", from, line)) {
                        println!("{:?}", e);
                    }
                }
            }
            RouterCommand::Direct { from, to, line } => {
                println!("{}, {}, {}", from, to, line);
                if let Some(sender) = map.get(&to) {
                    let _ = sender.try_send(format!("{}: {}\n", from, line));
                }
            }
        }
    }
}
