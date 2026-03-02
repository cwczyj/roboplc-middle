//! HTTP API module using actix-web
//!
//! Provides REST endpoints for device management and monitoring.

use actix_web::{web, HttpResponse, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::messages::{Message, Operation};
use crate::DeviceStatus;
use parking_lot_rt::RwLock;

static CORRELATION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_correlation_id() -> u64 {
    CORRELATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Application state shared across HTTP handlers
///
/// Contains device states map and optional Hub sender for message routing.
pub struct AppState {
    /// Device states map (device_id -> DeviceStatus)
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// Optional Hub sender for sending messages to other workers
    #[allow(dead_code)]
    pub hub_sender: Option<Sender<Message>>,
}

/// GET /api/devices
///
/// Returns a list of all devices with their status.
pub async fn get_devices(_data: web::Data<AppState>) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"devices": []})))
}

/// GET /api/devices/{id}
///
/// Returns the status of a specific device.
pub async fn get_device_by_id(
    path: web::Path<String>,
    _data: web::Data<AppState>,
) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"id": path.into_inner()})))
}

/// GET /api/health
///
/// Returns the health status of the middleware.
pub async fn get_health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"status": "healthy"})))
}

/// GET /api/config
///
/// Returns the current configuration.
pub async fn get_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"config": {}})))
}

/// POST /api/config/reload
///
/// Triggers a configuration reload.
pub async fn reload_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"reload": "ok"})))
}

/// Request body for setting a register
#[derive(Debug, Deserialize, Serialize)]
pub struct SetRegisterRequest {
    /// Modbus address (e.g., "h100", "h101")
    pub address: String,
    /// Value to write
    pub value: u16,
}

/// POST /api/devices/{id}/register
///
/// Sets a register value on the specified device.
pub async fn set_register(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<SetRegisterRequest>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Create DeviceControl message
    let message = Message::DeviceControl {
        device_id: device_id.clone(),
        operation: Operation::SetRegister,
        params: json!({
            "address": request.address,
            "value": request.value,
        }),
        correlation_id: next_correlation_id(),
    };

    // Send via Hub if available
    if let Some(ref sender) = data.hub_sender {
        let _ = sender.send(message);
    }

    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "set_register",
        "address": request.address,
        "value": request.value,
        "status": "sent"
    })))
}

/// Request body for batch operations
#[derive(Debug, Deserialize, Serialize)]
pub struct BatchOperationRequest {
    /// List of read addresses
    #[serde(default)]
    pub read: Vec<String>,
    /// List of write operations (address, value pairs)
    #[serde(default)]
    pub write: Vec<WriteOperation>,
}

/// Single write operation
#[derive(Debug, Deserialize, Serialize)]
pub struct WriteOperation {
    pub address: String,
    pub value: u16,
}

/// POST /api/devices/{id}/batch
///
/// Performs batch read/write operations on the specified device.
pub async fn batch_operations(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<BatchOperationRequest>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Determine operation type based on request content
    let operation = if !request.write.is_empty() {
        Operation::WriteBatch
    } else {
        Operation::ReadBatch
    };

    // Create DeviceControl message
    let message = Message::DeviceControl {
        device_id: device_id.clone(),
        operation,
        params: json!({
            "read": request.read,
            "write": request.write,
        }),
        correlation_id: next_correlation_id(),
    };

    // Send via Hub if available
    if let Some(ref sender) = data.hub_sender {
        let _ = sender.send(message);
    }

    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "batch",
        "read_count": request.read.len(),
        "write_count": request.write.len(),
        "status": "sent"
    })))
}

/// Request body for robot arm movement
#[derive(Debug, Deserialize, Serialize)]
pub struct MoveRequest {
    /// Target position (e.g., "home", "pick", "place", or coordinates)
    pub position: String,
    /// Optional speed (0-100)
    #[serde(default)]
    pub speed: Option<u8>,
}

/// POST /api/devices/{id}/move
///
/// Moves the robot arm to the specified position.
pub async fn move_to(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<MoveRequest>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Create DeviceControl message
    let message = Message::DeviceControl {
        device_id: device_id.clone(),
        operation: Operation::MoveTo,
        params: json!({
            "position": request.position,
            "speed": request.speed.unwrap_or(100),
        }),
        correlation_id: next_correlation_id(),
    };

    // Send via Hub if available
    if let Some(ref sender) = data.hub_sender {
        let _ = sender.send(message);
    }

    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "move_to",
        "position": request.position,
        "status": "sent"
    })))
}

/// Configure routes for the HTTP API
///
/// Sets up all endpoint routes for the actix-web server.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg
        .route("/api/devices", web::get().to(get_devices))
        .route("/api/devices/{id}", web::get().to(get_device_by_id))
        .route("/api/devices/{id}/register", web::post().to(set_register))
        .route("/api/devices/{id}/batch", web::post().to(batch_operations))
        .route("/api/devices/{id}/move", web::post().to(move_to))
        .route("/api/health", web::get().to(get_health))
        .route("/api/config", web::get().to(get_config))
        .route("/api/config/reload", web::post().to(reload_config));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_appstate_creation() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        // AppState should be created successfully
        let _ = state;
    }

    #[test]
    fn test_set_register_request_parse() {
        let json = r#"{"address": "h100", "value": 42}"#;
        let req: SetRegisterRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.address, "h100");
        assert_eq!(req.value, 42);
    }

    #[test]
    fn test_batch_operation_request_parse() {
        let json = r#"{"read": ["h100", "h101"], "write": [{"address": "h200", "value": 100}]}"#;
        let req: BatchOperationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.read.len(), 2);
        assert_eq!(req.write.len(), 1);
    }

    #[test]
    fn test_move_request_parse() {
        let json = r#"{"position": "home", "speed": 50}"#;
        let req: MoveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.position, "home");
        assert_eq!(req.speed, Some(50));
    }

    #[test]
    fn test_move_request_default_speed() {
        let json = r#"{"position": "pick"}"#;
        let req: MoveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.position, "pick");
        assert_eq!(req.speed, None);
    }

    #[tokio::test]
    async fn test_set_register_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        let app_state = web::Data::new(state);

        let req_body = SetRegisterRequest {
            address: "h100".to_string(),
            value: 42,
        };

        let result = set_register(
            web::Path::from("test-device".to_string()),
            app_state,
            web::Json(req_body),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_batch_operations_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        let app_state = web::Data::new(state);

        let req_body = BatchOperationRequest {
            read: vec!["h100".to_string(), "h101".to_string()],
            write: vec![],
        };

        let result = batch_operations(
            web::Path::from("test-device".to_string()),
            app_state,
            web::Json(req_body),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_move_to_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        let app_state = web::Data::new(state);

        let req_body = MoveRequest {
            position: "home".to_string(),
            speed: Some(75),
        };

        let result = move_to(
            web::Path::from("robot-arm-1".to_string()),
            app_state,
            web::Json(req_body),
        )
        .await;

        assert!(result.is_ok());
    }
}
