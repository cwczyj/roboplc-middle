use roboplc::controller::prelude::*;
use crate::{Variables, config::Config, Message};
use std::thread;
use std::sync::Arc;

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
        let device_states = context.variables().device_states.clone();
        
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
                    
                    let states = device_states.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let n = match socket.read(&mut buf).await {
                            Ok(n) if n > 0 => n,
                            _ => return,
                        };
                        let req = String::from_utf8_lossy(&buf[..n]);
                        
                        let (status, body) = handle_request(&req, &states);
                        
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

fn handle_request(req: &str, device_states: &Arc<parking_lot_rt::RwLock<std::collections::HashMap<String, crate::DeviceStatus>>>) -> (&'static str, String) {
    if req.starts_with("GET /api/devices/") {
        let path = req.lines().next().unwrap_or("");
        let device_id = path
            .trim_start_matches("GET /api/devices/")
            .split_whitespace()
            .next()
            .unwrap_or("");
        
        let states = device_states.read();
        if let Some(status) = states.get(device_id) {
            let body = serde_json::json!({
                "id": device_id,
                "connected": status.connected,
                "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
                "error_count": status.error_count,
                "reconnect_count": status.reconnect_count,
            });
            ("200 OK", body.to_string())
        } else {
            ("404 Not Found", serde_json::json!({"error": "Device not found"}).to_string())
        }
    } else if req.starts_with("GET /api/devices") {
        let states = device_states.read();
        let devices: Vec<serde_json::Value> = states.iter().map(|(id, status)| {
            serde_json::json!({
                "id": id,
                "connected": status.connected,
                "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
                "error_count": status.error_count,
            })
        }).collect();
        ("200 OK", serde_json::json!({"devices": devices}).to_string())
    } else if req.starts_with("GET /api/health") {
        ("200 OK", serde_json::json!({"status": "healthy"}).to_string())
    } else if req.starts_with("GET /api/config") {
        ("200 OK", serde_json::json!({"config": {}}).to_string())
    } else if req.starts_with("POST /api/config/reload") {
        ("200 OK", serde_json::json!({"reload": "ok"}).to_string())
    } else {
        ("404 Not Found", serde_json::json!({"error": "Not found"}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceStatus;
    use std::collections::HashMap;
    use std::time::Instant;

    fn make_device_states() -> Arc<parking_lot_rt::RwLock<HashMap<String, DeviceStatus>>> {
        Arc::new(parking_lot_rt::RwLock::new(HashMap::new()))
    }

    fn make_device_states_with_device(id: &str, connected: bool) -> Arc<parking_lot_rt::RwLock<HashMap<String, DeviceStatus>>> {
        let mut states = HashMap::new();
        states.insert(id.to_string(), DeviceStatus {
            connected,
            last_communication: Instant::now(),
            error_count: 0,
            reconnect_count: 0,
        });
        Arc::new(parking_lot_rt::RwLock::new(states))
    }

    #[test]
    fn test_get_devices_empty() {
        let states = make_device_states();
        let req = "GET /api/devices HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"devices\":[]"));
    }

    #[test]
    fn test_get_devices_with_device() {
        let states = make_device_states_with_device("device-1", true);
        let req = "GET /api/devices HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"device-1\""));
        assert!(body.contains("\"connected\":true"));
    }

    #[test]
    fn test_get_device_by_id_found() {
        let states = make_device_states_with_device("device-1", true);
        let req = "GET /api/devices/device-1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"device-1\""));
        assert!(body.contains("\"connected\":true"));
        assert!(body.contains("\"error_count\":0"));
    }

    #[test]
    fn test_get_device_by_id_not_found() {
        let states = make_device_states();
        let req = "GET /api/devices/nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "404 Not Found");
        assert!(body.contains("Device not found"));
    }

    #[test]
    fn test_get_health() {
        let states = make_device_states();
        let req = "GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"status\":\"healthy\""));
    }

    #[test]
    fn test_get_config() {
        let states = make_device_states();
        let req = "GET /api/config HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"config\""));
    }

    #[test]
    fn test_post_config_reload() {
        let states = make_device_states();
        let req = "POST /api/config/reload HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "200 OK");
        assert!(body.contains("\"reload\":\"ok\""));
    }

    #[test]
    fn test_unknown_path() {
        let states = make_device_states();
        let req = "GET /unknown HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, body) = handle_request(req, &states);
        assert_eq!(status, "404 Not Found");
        assert!(body.contains("Not found"));
    }
}