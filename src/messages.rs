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
        correlation_id: u64,
        respond_to: Option<Sender<DeviceResponseData>>,
    },
    DeviceResponse {
        device_id: String,
        success: bool,
        data: JsonValue,
        error: Option<String>,
        correlation_id: u64,
    },
    #[data_delivery(always)]
    DeviceHeartbeat {
        device_id: String,
        timestamp_ms: u64,
        latency_us: u64,
    },
    #[data_delivery(always)]
    ConfigUpdate {
        config: String,
    },
    TimeoutCleanup {
        correlation_id: u64,
    },
    SystemStatus {
        requester: String,
        respond_to: Sender<SystemStatusResponse>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    ReadSignalGroup,
    WriteSignalGroup,
    GetStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemStatusResponse {
    pub devices_count: u32,
    pub system_healthy: bool,
    pub uptime_secs: u64,
}

pub type DeviceResponseData = (bool, JsonValue, Option<String>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_read_signal_group_serialization() {
        let op = Operation::ReadSignalGroup;
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("ReadSignalGroup"));
        let decoded: Operation = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, Operation::ReadSignalGroup));
    }

    #[test]
    fn test_operation_write_signal_group_serialization() {
        let op = Operation::WriteSignalGroup;
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("WriteSignalGroup"));
        let decoded: Operation = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, Operation::WriteSignalGroup));
    }

    #[test]
    fn test_operation_get_status_serialization() {
        let op = Operation::GetStatus;
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("GetStatus"));
        let decoded: Operation = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, Operation::GetStatus));
    }

    #[test]
    fn test_device_control_clone_with_read_signal_group() {
        let msg = Message::DeviceControl {
            device_id: "plc-1".to_string(),
            operation: Operation::ReadSignalGroup,
            params: serde_json::json!({ "group_name": "sensors" }),
            correlation_id: 123,
            respond_to: None,
        };
        let cloned = msg.clone();
        assert!(matches!(cloned, Message::DeviceControl { .. }));
        if let Message::DeviceControl {
            ref device_id,
            ref operation,
            ..
        } = cloned
        {
            assert_eq!(device_id, "plc-1");
            assert!(matches!(operation, Operation::ReadSignalGroup));
        }
    }

    #[test]
    fn test_device_control_clone_with_write_signal_group() {
        let msg = Message::DeviceControl {
            device_id: "robot-1".to_string(),
            operation: Operation::WriteSignalGroup,
            params: serde_json::json!({ "group_name": "actuators", "values": [1, 2, 3] }),
            correlation_id: 456,
            respond_to: None,
        };
        let cloned = msg.clone();
        assert!(matches!(cloned, Message::DeviceControl { .. }));
    }
}
