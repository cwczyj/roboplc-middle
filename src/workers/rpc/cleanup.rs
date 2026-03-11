// =============================================================================
// RPC Worker - 超时请求清理
// =============================================================================
// 这个模块处理超时请求的清理逻辑

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use roboplc::prelude::Hub;

use tokio::time::{Duration, Instant};

use crate::messages::Message;

use super::types::PendingRequest;

/// Cleanup timed-out requests
pub fn cleanup_timed_out_requests(
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    hub: Hub<Message>,
) {
    let timeout_duration = Duration::from_secs(35); // Slightly longer than request timeout

    let mut pending_lock = pending.lock().unwrap();
    let now = Instant::now();

    let timed_out: Vec<u64> = pending_lock
        .iter()
        .filter(|(_, req)| now.duration_since(req.created_at) > timeout_duration)
        .map(|(&id, _)| id)
        .collect();

    for id in timed_out {
        if let Some(req) = pending_lock.remove(&id) {
            // Send error response
            let _ = req.respond_to.send((
                false,
                serde_json::json!({}),
                Some("Request timed out during cleanup".to_string()),
            ));

            // Notify Hub about timeout
            hub.send(Message::TimeoutCleanup { correlation_id: id });
            tracing::warn!(correlation_id = id, "Cleaned up timed-out request");
        }
    }
}