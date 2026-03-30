// =============================================================================
// RPC Worker - 超时请求清理 (Wave 3 重构后保留)
// =============================================================================
// Wave 3 重构后，此模块不再在生产代码中使用:
// - 请求在同一 blocking thread 中完成，无需 pending 超时清理
// - 保留此文件仅供向后兼容和测试使用

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use roboplc::prelude::Hub;

use tokio::time::{Duration, Instant};

use crate::messages::Message;

use super::types::PendingRequest;

#[deprecated(
    since = "0.3.0",
    note = "No longer needed - requests complete in single blocking thread"
)]
pub fn cleanup_timed_out_requests(
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    hub: Hub<Message>,
) {
    let timeout_duration = Duration::from_secs(35);

    let mut pending_lock = pending.lock().unwrap();
    let now = Instant::now();

    let timed_out: Vec<u64> = pending_lock
        .iter()
        .filter(|(_, req)| now.duration_since(req.created_at) > timeout_duration)
        .map(|(&id, _)| id)
        .collect();

    for id in timed_out {
        if let Some(req) = pending_lock.remove(&id) {
            let _ = req.respond_to.send((
                false,
                serde_json::json!({}),
                Some("Request timed out during cleanup".to_string()),
            ));

            hub.send(Message::TimeoutCleanup { correlation_id: id });
            tracing::warn!(correlation_id = id, "Cleaned up timed-out request");
        }
    }
}
