// =============================================================================
// RPC Worker - RPC 处理器实现
// =============================================================================
// 这个模块实现了 RpcHandler 和 RpcServerHandler trait

use crate::messages::{Message, Operation};

use roboplc::prelude::Hub;

use serde_json::Value as JsonValue;

use std::net::SocketAddr;

use tokio::sync::mpsc;

use super::types::{RpcMethod, RpcResultType};
use super::types::DeviceControlRequest;

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
    device_control_tx: mpsc::Sender<DeviceControlRequest>,
    hub: Hub<Message>,
}

impl RpcHandler {
    pub fn new(
        device_ids: Vec<String>,
        device_control_tx: mpsc::Sender<DeviceControlRequest>,
        hub: Hub<Message>,
    ) -> Self {
        Self {
            device_ids,
            device_control_tx,
            hub,
        }
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
        match method {
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
        }
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

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = DeviceControlRequest {
            device_id: device_id.to_string(),
            operation,
            params,
            correlation_id,
            respond_to: response_tx,
        };

        // Use blocking_send to send from sync context to async channel
        if let Err(error) = self.device_control_tx.blocking_send(request) {
            tracing::error!(%error, "failed to send DeviceControl request");
            return Ok(RpcResultType::Error {
                error: format!("Internal error: {}", error),
            });
        }

        // Use blocking_recv to wait for response in sync context
        match response_rx.blocking_recv() {
            Ok((success, data, error)) => {
                if success {
                    Ok(RpcResultType::Data { success: true, data })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                }
            }
            Err(_) => {
                tracing::warn!(correlation_id, "Request timed out, sending cleanup");
                self.hub.send(Message::TimeoutCleanup { correlation_id });
                Ok(RpcResultType::Error {
                    error: "Request timed out".to_string(),
                })
            }
        }
    }
}