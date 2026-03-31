// =============================================================================
// RPC Worker - RPC 处理器实现
// =============================================================================
// 这个模块实现了 RpcHandler 和 RpcServerHandler trait
//
// Wave 3 重构: 合并 spawn_blocking 调用
// - 移除中间 mpsc 通道 (device_control_tx/rx)
// - 移除 oneshot 通道
// - 直接在 send_device_control 中发送到 Hub 并等待响应
// - 每个请求只占用 1 个 blocking thread (之前是 2 个)

use crate::messages::{DeviceResponseData, Message, Operation};

use roboplc::prelude::Hub;

use serde_json::Value as JsonValue;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::types::{RpcMethod, RpcResultType};

// 导入 roboplc_rpc 相关类型
use roboplc_rpc::{server::RpcServerHandler, RpcResult};

// ---------------------------------------------------------------------------
// 关联函数
// ---------------------------------------------------------------------------

static CORRELATION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn next_correlation_id() -> u64 {
    CORRELATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// RPC 处理器结构体
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RpcHandler {
    device_ids: Vec<String>,
    hub: Hub<Message>,
}

impl RpcHandler {
    pub fn new(device_ids: Vec<String>, hub: Hub<Message>) -> Self {
        Self { device_ids, hub }
    }

    /// Send message to Hub with deadlock prevention.
    ///
    /// RoboPLC's Hub.send() is blocking and uses bounded internal channels
    /// (default capacity 1024). When Hub's queue is full, send() blocks.
    /// If RPC Worker blocks in send(), it cannot enter recv_timeout(),
    /// creating a deadlock with ModbusWorker waiting for response channel.
    ///
    /// This method spawns a separate thread for the blocking send operation,
    /// allowing the main thread to timeout if send doesn't complete quickly.
    /// This prevents indefinite blocking and allows graceful error return.
    fn send_to_hub_with_protection(
        &self,
        message: Message,
        timeout: Duration,
    ) -> Result<(), String> {
        let hub = self.hub.clone();
        let send_completed = Arc::new(AtomicBool::new(false));
        let send_completed_clone = send_completed.clone();

        let send_thread = std::thread::spawn(move || {
            hub.send(message);
            send_completed_clone.store(true, Ordering::SeqCst);
        });

        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if send_completed.load(Ordering::SeqCst) {
                // Send completed successfully
                // Join the thread to ensure proper cleanup
                let _ = send_thread.join();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        tracing::warn!(
            timeout_ms = timeout.as_millis(),
            "Hub send timeout - system may be overloaded"
        );

        Err("Hub send timeout - system overloaded".to_string())
    }
}

// ---------------------------------------------------------------------------
// 实现 RpcServerHandler trait
// ---------------------------------------------------------------------------

impl<'a> RpcServerHandler<'a> for RpcHandler {
    type Method = RpcMethod<'a>;
    type Result = RpcResultType;
    type Source = SocketAddr;

    fn handle_call(
        &'a self,
        method: Self::Method,
        _source: Self::Source,
    ) -> RpcResult<Self::Result> {
        let start_time = SystemTime::now();

        let method_name = match &method {
            RpcMethod::Ping { .. } => "ping",
            RpcMethod::GetVersion { .. } => "get_version",
            RpcMethod::GetDeviceList { .. } => "get_device_list",
            RpcMethod::GetStatus { .. } => "get_status",
            RpcMethod::ReadSignalGroup { .. } => "read_signal_group",
            RpcMethod::WriteSignalGroup { .. } => "write_signal_group",
        };

        let result = match method {
            RpcMethod::Ping {} => Ok(RpcResultType::Success { success: true }),
            RpcMethod::GetVersion {} => Ok(RpcResultType::Version {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            RpcMethod::GetDeviceList {} => Ok(RpcResultType::DeviceList {
                devices: self.device_ids.clone(),
            }),
            RpcMethod::GetStatus { device_id } => {
                self.send_device_control(device_id, Operation::GetStatus, serde_json::json!({}))
            }
            RpcMethod::ReadSignalGroup {
                device_id,
                group_name,
            } => {
                let params = serde_json::json!({ "group_name": group_name });
                self.send_device_control(device_id, Operation::ReadSignalGroup, params)
            }
            RpcMethod::WriteSignalGroup {
                device_id,
                group_name,
                data,
            } => {
                let params = serde_json::json!({ "group_name": group_name, "data": data });
                self.send_device_control(device_id, Operation::WriteSignalGroup, params)
            }
        };

        // Calculate and log total RPC handling time
        let elapsed = start_time.elapsed().unwrap_or(Duration::ZERO);
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        tracing::info!("RPC {} completed in {:.3} ms", method_name, elapsed_ms);

        result
    }
}

// ---------------------------------------------------------------------------
// RpcHandler 辅助方法实现
// ---------------------------------------------------------------------------

impl RpcHandler {
    fn send_device_control(
        &self,
        device_id: &str,
        operation: Operation,
        params: JsonValue,
    ) -> RpcResult<RpcResultType> {
        let correlation_id = next_correlation_id();

        let (response_tx, response_rx) =
            sync_channel::<DeviceResponseData>(crate::MAX_PENDING_RESPONSES);

        let message = Message::DeviceControl {
            device_id: device_id.to_string(),
            operation,
            params,
            correlation_id,
            respond_to: Some(response_tx),
        };

        // DEADLOCK FIX: Use protected send with timeout
        // Prevents blocking indefinitely if Hub's internal queue is full
        if let Err(e) = self.send_to_hub_with_protection(message, Duration::from_millis(500)) {
            tracing::warn!(
                correlation_id,
                error = %e,
                "Hub send failed - system overloaded"
            );
            return Ok(RpcResultType::Error {
                error: format!("System overloaded: {}", e),
            });
        }

        // Now safe to wait on response - RPC Worker is not blocked in send
        match response_rx.recv_timeout(Duration::from_secs(5)) {
            Ok((success, data, error)) => {
                if success {
                    Ok(RpcResultType::Data {
                        success: true,
                        data,
                    })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                }
            }
            Err(_) => {
                tracing::warn!(correlation_id, "Request timed out");
                Ok(RpcResultType::Error {
                    error: "Request timed out".to_string(),
                })
            }
        }
    }
}
