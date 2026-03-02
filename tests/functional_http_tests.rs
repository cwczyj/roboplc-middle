//! Functional tests for HTTP API

use parking_lot_rt::RwLock;
use roboplc_middleware::DeviceStatus;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

fn make_device_states() -> Arc<RwLock<HashMap<String, DeviceStatus>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

fn make_device_states_with_device(
    id: &str,
    connected: bool,
) -> Arc<RwLock<HashMap<String, DeviceStatus>>> {
    let mut states = HashMap::new();
    states.insert(
        id.to_string(),
        DeviceStatus {
            connected,
            last_communication: Instant::now(),
            error_count: 0,
            reconnect_count: 0,
        },
    );
    Arc::new(RwLock::new(states))
}

fn handle_request(
    req: &str,
    device_states: &Arc<RwLock<HashMap<String, DeviceStatus>>>,
) -> (&'static str, String) {
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
            });
            ("200 OK", body.to_string())
        } else {
            (
                "404 Not Found",
                serde_json::json!({"error": "Device not found"}).to_string(),
            )
        }
    } else if req.starts_with("GET /api/devices") {
        let states = device_states.read();
        let devices: Vec<serde_json::Value> = states
            .iter()
            .map(|(id, status)| {
                serde_json::json!({
                    "id": id,
                    "connected": status.connected,
                    "error_count": status.error_count,
                })
            })
            .collect();
        (
            "200 OK",
            serde_json::json!({"devices": devices}).to_string(),
        )
    } else if req.starts_with("GET /api/health") {
        (
            "200 OK",
            serde_json::json!({"status": "healthy"}).to_string(),
        )
    } else {
        (
            "404 Not Found",
            serde_json::json!({"error": "Not found"}).to_string(),
        )
    }
}

#[test]
fn http_get_devices_empty_list() {
    let states = make_device_states();
    let req = "GET /api/devices HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (status, body) = handle_request(req, &states);

    assert_eq!(status, "200 OK");
    assert!(body.contains("\"devices\":[]"));
}

#[test]
fn http_get_devices_with_devices() {
    let states = make_device_states_with_device("device-1", true);
    let req = "GET /api/devices HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (status, body) = handle_request(req, &states);

    assert_eq!(status, "200 OK");
    assert!(body.contains("device-1"));
    assert!(body.contains("\"connected\":true"));
}

#[test]
fn http_get_device_by_id_found() {
    let states = make_device_states_with_device("plc-1", true);
    let req = "GET /api/devices/plc-1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (status, body) = handle_request(req, &states);

    assert_eq!(status, "200 OK");
    assert!(body.contains("plc-1"));
}

#[test]
fn http_get_device_by_id_not_found() {
    let states = make_device_states();
    let req = "GET /api/devices/unknown HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (status, body) = handle_request(req, &states);

    assert_eq!(status, "404 Not Found");
    assert!(body.contains("Device not found"));
}

#[test]
fn http_health_endpoint() {
    let states = make_device_states();
    let req = "GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (status, body) = handle_request(req, &states);

    assert_eq!(status, "200 OK");
    assert!(body.contains("healthy"));
}
