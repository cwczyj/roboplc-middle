// =============================================================================
// RPC Worker - 类型定义
// =============================================================================
// 这个模块包含 RPC Worker 相关的类型定义

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::oneshot::Sender as OneshotSender;
use tokio::time::Instant;

// ---------------------------------------------------------------------------
// RPC 方法枚举定义
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "m",
    content = "p",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RpcMethod<'a> {
    Ping {},
    GetVersion {},
    GetDeviceList {},
    GetStatus {
        device_id: &'a str,
    },
    ReadSignalGroup {
        device_id: &'a str,
        group_name: String,
    },
    WriteSignalGroup {
        device_id: &'a str,
        group_name: String,
        data: JsonValue,
    },
}

// ---------------------------------------------------------------------------
// RPC 响应结果枚举
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcResultType {
    Success {
        success: bool,
    },
    Version {
        version: String,
    },
    DeviceList {
        devices: Vec<String>,
    },
    Data {
        success: bool,
        data: serde_json::Value,
    },
    Status {
        connected: bool,
        last_communication_ms: u64,
        error_count: u32,
    },
    Error {
        error: String,
    },
}

// ---------------------------------------------------------------------------
// 设备控制请求相关类型
// ---------------------------------------------------------------------------

/// ResponseSender uses tokio oneshot channel for async-safe response handling
pub type ResponseSender = OneshotSender<crate::messages::DeviceResponseData>;

/// Device control request sent from RpcHandler to the main loop
pub struct DeviceControlRequest {
    pub device_id: String,
    pub operation: crate::messages::Operation,
    pub params: JsonValue,
    pub correlation_id: u64,
    pub respond_to: ResponseSender,
}

/// Pending request tracking for cleanup
pub struct PendingRequest {
    pub correlation_id: u64,
    pub created_at: Instant,
    pub respond_to: ResponseSender,
}

impl PendingRequest {
    pub fn new(correlation_id: u64, respond_to: ResponseSender) -> Self {
        Self {
            correlation_id,
            created_at: Instant::now(),
            respond_to,
        }
    }
}