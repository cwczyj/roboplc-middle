//! # 消息模块
//!
//! 定义在 RoboPLC Hub 中传递的所有消息类型。
//!
//! ## 消息传递机制
//!
//! RoboPLC 使用 Hub 模式在 workers 之间传递消息：
//! - RpcWorker 接收 JSON-RPC 请求，发送 DeviceControl 消息
//! - ModbusWorker 接收 DeviceControl 消息，执行 Modbus 操作，返回 DeviceResponse 消息
//! - HttpWorker 查询系统状态，发送 SystemStatus 消息
//!
//! ## 消息类型
//!
//! - `DeviceControl`: 设备控制请求
//! - `DeviceResponse`: 设备响应
//! - `DeviceHeartbeat`: 心跳消息（总是传递）
//! - `ConfigUpdate`: 配置更新通知（总是传递）
//! - `SystemStatus`: 系统状态查询
//!

use roboplc::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::mpsc::Sender;

#[derive(Clone, Debug, DataPolicy)]
pub enum Message {
    #[data_delivery(single)]
    DeviceControl {
        device_id: String,
        operation: Operation,
        params: JsonValue,
    },
    DeviceResponse {
        device_id: String,
        success: bool,
        data: JsonValue,
        error: Option<String>,
    },
    #[data_delivery(always)]
    DeviceHeartbeat {
        device_id: String,
        timestamp_ms: u64,
        latency_us: u64,
    },
    #[data_delivery(always)]
    ConfigUpdate { config: String },
    SystemStatus {
        requester: String,
        respond_to: Sender<SystemStatusResponse>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    SetRegister,
    GetRegister,
    WriteBatch,
    ReadBatch,
    MoveTo,
    GetStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemStatusResponse {
    pub devices_count: u32,
    pub system_healthy: bool,
    pub uptime_secs: u64,
}
