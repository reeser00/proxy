use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, Error};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct Proxy {
    pub home_addr: String,
    pub server_addr: String,
    pub tcp_listener: Option<TcpListener>,
    //Client => Server
    pub client_sender: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
    //Client => Server
    pub server_receiver: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    //Client => Server
    pub server_sender: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
    //Client => Server
    pub client_receiver: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    pub client_stream: Arc<Mutex<Option<TcpStream>>>,
    pub server_stream: Arc<Mutex<Option<TcpStream>>>,
}

impl Proxy {
    pub async fn from(home_addr: String, server_addr: String) -> Result<Self, Error> {
        let listener = TcpListener::bind(&home_addr).await;
        let (c_tx, c_rx) = mpsc::channel(100);
        let (s_tx, s_rx) = mpsc::channel(100);
        match listener {
            Ok(tcp_listener) => {
                println!("Proxy server running on {}", home_addr);

                let proxy = Self {
                    home_addr,
                    server_addr,
                    tcp_listener: Some(tcp_listener),
                    client_sender: Arc::new(Mutex::new(c_tx)),
                    server_receiver: Arc::new(Mutex::new(c_rx)),
                    server_sender: Arc::new(Mutex::new(s_tx)),
                    client_receiver: Arc::new(Mutex::new(s_rx)),
                    client_stream: Arc::new(Mutex::new(None)),
                    server_stream: Arc::new(Mutex::new(None)),
                };
                Ok(proxy)
            },
            Err(e) => {
                eprintln!("Error binding TcpListener: {}", e);
                Err(e)
            },
        }
    }

    pub async fn start(&mut self) {
        if let Some(tcp_listener) = &self.tcp_listener {
            match tcp_listener.accept().await {
                Ok((client_stream, _)) => {
                    let server_stream = TcpStream::connect(&self.server_addr).await.expect("Error connecting to server address");
                    self.client_stream = Arc::new(Mutex::new(Some(client_stream)));
                    self.server_stream = Arc::new(Mutex::new(Some(server_stream)));
                    self.handle_client().await;
                    self.handle_server().await;
                    self.receive_on_client().await;
                    self.receive_on_server().await;
                },
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
                }
            }
        }
    }

    pub async fn handle_client(&mut self) {
        match self.client_stream.lock() {
            Ok(mut guard) => {
                match guard.as_mut() {
                    Some(_) => {
                        let client_stream = self.client_stream.clone();
                        let client_sender = self.client_sender.clone();
                        tokio::spawn( async move {
                            loop{
                                let mut guard = match client_stream.lock() {
                                    Ok(guard) => {
                                        guard
                                    },
                                    Err(_) => {
                                        eprintln!("Error locking self.client_stream");
                                        continue;
                                    }
                                };

                                let c_stream = match guard.as_mut() {
                                    Some(stream) => stream,
                                    None => {
                                        eprintln!("Client stream is closed or uninitialized");
                                        return;
                                    }
                                };

                                let mut buffer = [0; 1024];
                                match c_stream.try_read(&mut buffer) {
                                    Ok(0) => {
                                        //Connection closed by client
                                        println!("Client disconnected");
                                        *guard = None;
                                        return;
                                    },
                                    Ok(bytes_read) => {
                                        println!("Received {} bytes: {:?}", bytes_read, &buffer[..bytes_read]);
                                        match client_sender.lock() {
                                            Ok(sender) => {
                                                match sender.try_send(buffer[..bytes_read].to_vec()) {
                                                    Ok(_) => { println!("Send a package to server") },
                                                    Err(_) => {eprintln!("error converting array to vector");}
                                                }
                                            }
                                            Err(_) => {
                                                eprintln!("Failed to acquire lock on client sender");
                                                continue;
                                            }
                                        }
                                    },
                                    Err(ref e) if e.kind() == tokio::io::ErrorKind::WouldBlock => {
                                        continue;
                                    },
                                    Err(e) => {
                                        eprintln!("Error reading from the client: {:?}", e);
                                        return;
                                    }
                                }
                            }
                        });
                    },
                    None => {
                        eprintln!("handle client called before client_stream exists")
                    }
                }
            },
            Err(_) => { eprintln!("Error locking self.client_stream")},
        }
    }

    pub async fn handle_server(&mut self) {
        match self.server_stream.lock() {
            Ok(mut guard) => {
                match guard.as_mut() {
                    Some(_) => {
                        let server_stream = self.server_stream.clone();
                        let server_sender = self.server_sender.clone();
                        tokio::spawn( async move {
                            loop{
                                let mut guard = match server_stream.lock() {
                                    Ok(guard) => guard,
                                    Err(_) => {
                                        eprintln!("Error locking self.server_stream");
                                        continue;
                                    }
                                };

                                let s_stream = match guard.as_mut() {
                                    Some(stream) => stream,
                                    None => {
                                        eprintln!("Server stream is closed or uninitialized");
                                        return;
                                    }
                                };

                                let mut buffer = [0; 1024];
                                match s_stream.try_read(&mut buffer) {
                                    Ok(0) => {
                                        //Connection closed by client
                                        println!("Destination Server closed the connection");
                                        *guard = None;
                                        return;
                                    },
                                    Ok(bytes_read) => {
                                        println!("Received {} bytes: {:?}", bytes_read, &buffer[..bytes_read]);
                                        match server_sender.lock() {
                                            Ok(sender) => {
                                                match sender.try_send(buffer[..bytes_read].to_vec()) {
                                                    Ok(_) => { println!("Send a package to client") },
                                                    Err(_) => {eprintln!("error converting array to vector");}
                                                }
                                            }
                                            Err(_) => {
                                                eprintln!("Failed to acquire lock on client sender");
                                                continue;
                                            }
                                        }
                                    },
                                    Err(ref e) if e.kind() == tokio::io::ErrorKind::WouldBlock => {
                                        continue;
                                    },
                                    Err(e) => {
                                        eprintln!("Error reading from the client: {:?}", e);
                                        return;
                                    }
                                }
                            }
                        });
                    },
                    None => {
                        eprintln!("handle client called before client_stream exists")
                    }
                }
            },
            Err(_) => { eprintln!("Error locking self.client_stream")},
        }
    }

    pub async fn receive_on_client(&mut self) {
        let client_stream = self.client_stream.clone();
        let rx = self.client_receiver.clone();
        tokio::spawn(async move {
            loop{
                let mut guard = match rx.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        eprintln!("Failed to acquire lock on additional client rx.");
                        continue;
                    }
                };
                let _rx = match guard.try_recv() {
                    Ok(buffer) => {
                        println!("[ClientRX]: I got: {:?}", buffer);

                        let mut guard = match client_stream.lock() {
                            Ok(guard) => {
                                guard
                            },
                            Err(_) => {
                                eprintln!("Failed to acquire lock on additional client rx.");
                                continue;
                            }
                        };

                        let _stream = match guard.as_mut() {
                            Some(stream) => {
                                println!("Mutex acquired... sending buffer to Client");
                                stream.try_write(&buffer[..])
                            },
                            None => {
                                eprintln!("Client stream is closed or uninitialized");
                                return;
                            }
                        };

                        println!("Result sending to Server {:?}", _stream);

                    },
                    Err(e) => {
                        if e == mpsc::error::TryRecvError::Empty {
                            continue;
                        }
                        eprintln!("Error reading from additional client rx: {}", e);
                        continue;
                    }
                };
            }
        });
    }

    pub async fn receive_on_server(&mut self) {
        let server_stream = self.server_stream.clone();
        let rx = self.server_receiver.clone();
        tokio::spawn(async move {

            loop{
                let mut guard = match rx.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        eprintln!("Failed to acquire lock on additional server rx.");
                        continue;
                    }
                };

                let _rx = match guard.try_recv() {
                    Ok(buffer) => {
                        println!("[ServerRX]: I got: {:?}", buffer);

                        let mut guard = match server_stream.lock() {
                            Ok(guard) => {
                                guard
                            },
                            Err(_) => {
                                eprintln!("Failed to acquire lock on additional server rx.");
                                continue;
                            }
                        };

                        let _stream = match guard.as_mut() {
                            Some(stream) => {
                                println!("Mutex acquired... sending buffer to Server");
                                stream.try_write(&buffer[..])
                            },
                            None => {
                                eprintln!("Client stream is closed or uninitialized");
                                return;
                            }
                        };

                        println!("Result sending to Server {:?}", _stream);
                    },
                    Err(e) => {
                        if e == mpsc::error::TryRecvError::Empty {
                            continue;
                        }
                        eprintln!("Error reading from additional server rx: {}", e);
                        continue;
                    }
                };
            }
        });
    }


}

