use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use crate::{Variables, config::Config, Message};
use std::thread;

#[derive(WorkerOpts)]
#[worker_opts(name = "http_server", blocking = true)]
pub struct HttpWorker {
    config: Config,
}

impl HttpWorker {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Worker<Message, Variables> for HttpWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let http_port = self.config.server.http_port;
        let addr = format!("0.0.0.0:{}", http_port);
        
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("HttpWorker: failed to create Tokio runtime");
                
            rt.block_on(async move {
                use tokio::net::TcpListener;
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                
                let listener = match TcpListener::bind(&addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("HttpWorker: failed to bind {}: {}", addr, e);
                        return;
                    }
                };
                println!("HttpWorker: listening on http://{}", addr);
                loop {
                    let (mut socket, _) = match listener.accept().await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("HttpWorker: accept error: {}", e);
                            continue;
                        }
                    };
                    
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let n = match socket.read(&mut buf).await {
                            Ok(n) if n > 0 => n,
                            _ => return,
                        };
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let (status, body) = if req.starts_with("GET /api/devices/") {
                            ("200 OK", "{\"devices\":[{\"id\":1}]}")
                        } else if req.starts_with("GET /api/devices") {
                            ("200 OK", "{\"devices\":[]}")
                        } else if req.starts_with("GET /api/health") {
                            ("200 OK", "{\"status\":\"healthy\"}")
                        } else if req.starts_with("GET /api/config") {
                            ("200 OK", "{\"config\":{}}")
                        } else if req.starts_with("POST /api/config/reload") {
                            ("200 OK", "{\"reload\":\"ok\"}")
                        } else {
                            ("404 Not Found", "Not Found")
                        };
                        let response = format!(
                            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            status, body.len(), body
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            });
        });

        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Ok(())
    }
}