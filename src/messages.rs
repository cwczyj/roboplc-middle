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
