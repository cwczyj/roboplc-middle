// =============================================================================
// RPC Worker - 设备控制请求处理 (Wave 3 重构后保留)
// =============================================================================
// Wave 3 重构后，此模块的功能已合并到 handler.rs 中:
// - handle_device_control_request 函数已废弃
// - 请求处理直接在 send_device_control 中完成
// - 保留此文件仅供向后兼容和测试使用

use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, SyncSender as StdSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use roboplc::prelude::Hub;

use crate::messages::{DeviceResponseData, Message};

use super::types::{DeviceControlRequest, PendingRequest};

/// Handle device control request by forwarding to Hub
///
/// Wave 3 重构说明: 此函数已不再在生产代码中使用。
/// 请求处理现在直接在 handler.rs 的 send_device_control 中完成，
/// 避免了额外的 spawn_blocking 调用，将每请求线程占用从 2 减少到 1。
///
/// 此函数保留用于测试和向后兼容。
#[deprecated(
    since = "0.3.0",
    note = "Use handler.send_device_control instead - merged spawn_blocking calls"
)]
pub fn handle_device_control_request(
    request: DeviceControlRequest,
    hub: Hub<Message>,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    let correlation_id = request.correlation_id;
    let respond_to = request.respond_to;

    {
        let mut pending_lock = pending.lock().unwrap();
        pending_lock.insert(
            correlation_id,
            PendingRequest::new(correlation_id, respond_to),
        );
    }

    let (std_tx, std_rx): (StdSender<DeviceResponseData>, _) =
        sync_channel(crate::MAX_PENDING_RESPONSES);

    let message = Message::DeviceControl {
        device_id: request.device_id,
        operation: request.operation,
        params: request.params,
        correlation_id,
        respond_to: Some(std_tx),
    };

    hub.send(message);

    let hub_for_cleanup = hub.clone();
    let pending_for_cleanup = pending.clone();
    tokio::task::spawn_blocking(move || match std_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(response) => {
            let mut pending_lock = pending_for_cleanup.lock().unwrap();
            if let Some(req) = pending_lock.remove(&correlation_id) {
                let _ = req.respond_to.send(response);
            }
        }
        Err(_) => {
            tracing::warn!(correlation_id, "Request timed out in bridge");
            hub_for_cleanup.send(Message::TimeoutCleanup { correlation_id });

            let mut pending_lock = pending_for_cleanup.lock().unwrap();
            if let Some(req) = pending_lock.remove(&correlation_id) {
                let _ = req.respond_to.send((
                    false,
                    serde_json::json!({}),
                    Some("Request timed out".to_string()),
                ));
            }
        }
    });
}
