// =============================================================================
// RPC Worker - 设备控制请求处理
// =============================================================================
// 这个模块处理设备控制请求的转发和响应路由

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender as StdSender};
use std::sync::{Arc, Mutex};

use roboplc::prelude::Hub;

use crate::messages::{DeviceResponseData, Message};

use super::types::{DeviceControlRequest, PendingRequest};

/// Handle device control request by forwarding to Hub
pub fn handle_device_control_request(
    request: DeviceControlRequest,
    hub: Hub<Message>,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    let correlation_id = request.correlation_id;
    let respond_to = request.respond_to;

    // Track pending request for cleanup
    {
        let mut pending_lock = pending.lock().unwrap();
        pending_lock.insert(
            correlation_id,
            PendingRequest::new(correlation_id, respond_to),
        );
    }

    // Create std::sync::mpsc channel for Message compatibility
    let (std_tx, std_rx): (StdSender<DeviceResponseData>, _) = channel();

    let message = Message::DeviceControl {
        device_id: request.device_id,
        operation: request.operation,
        params: request.params,
        correlation_id,
        respond_to: Some(std_tx),
    };

    // Send to DeviceManager via Hub
    hub.send(message);

    // Bridge std::sync::mpsc response to tokio::sync::oneshot
    // Use spawn_blocking to wait on the std channel
    let hub_for_cleanup = hub.clone();
    let pending_for_cleanup = pending.clone();
    tokio::task::spawn_blocking(move || {
        match std_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(response) => {
                // Remove from pending and send response
                let mut pending_lock = pending_for_cleanup.lock().unwrap();
                if let Some(req) = pending_lock.remove(&correlation_id) {
                    let _ = req.respond_to.send(response);
                }
            }
            Err(_) => {
                tracing::warn!(correlation_id, "Request timed out in bridge");
                hub_for_cleanup.send(Message::TimeoutCleanup { correlation_id });

                // Remove from pending and send error response
                let mut pending_lock = pending_for_cleanup.lock().unwrap();
                if let Some(req) = pending_lock.remove(&correlation_id) {
                    let _ = req.respond_to.send((
                        false,
                        serde_json::json!({}),
                        Some("Request timed out".to_string()),
                    ));
                }
            }
        }
    });
}