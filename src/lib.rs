pub mod api;
pub mod config;
pub mod messages;
pub mod profiles;
pub mod workers;

pub use messages::{Message, Operation, SystemStatusResponse};

use parking_lot_rt::RwLock;
use rtsc::buf::DataBuffer;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Instant;

/// Device status tracking
#[derive(Debug)]
pub struct DeviceStatus {
    pub connected: bool,
    pub last_communication: Instant,
    pub error_count: u32,
    pub reconnect_count: u32,
}

/// Modbus transaction log entry for DataBuffer
#[derive(Clone, Debug)]
pub struct ModbusLogEntry {
    pub device_id: String,
    pub timestamp_ms: u64,
    pub operation: String,
    pub address: String,
    pub success: bool,
    pub latency_us: u64,
}

/// Device event for DataBuffer
#[derive(Clone, Debug)]
pub struct DeviceEvent {
    pub device_id: String,
    pub event_type: DeviceEventType,
    pub timestamp_ms: u64,
    pub details: String,
}

#[derive(Clone, Debug)]
pub enum DeviceEventType {
    Connected,
    Disconnected,
    Reconnecting,
    Error,
    HeartbeatMissed,
}

/// Latency sample for DataBuffer
#[derive(Clone, Debug, Copy)]
pub struct LatencySample {
    pub device_id: u32,
    pub latency_us: u64,
    pub timestamp_ms: u64,
}

/// Shared state for all workers
pub struct Variables {
    /// Active device count
    pub devices_count: AtomicU32,
    /// System health flag
    pub system_healthy: AtomicBool,
    /// Per-device status (random access, concurrent reads)
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// Latency monitoring samples (bulk statistics)
    pub latency_samples: DataBuffer<LatencySample>,
    /// Recent Modbus transactions (ring buffer for logging)
    pub modbus_transactions: DataBuffer<ModbusLogEntry>,
    /// Device event stream (event streaming)
    pub device_events: DataBuffer<DeviceEvent>,
}

impl std::fmt::Debug for Variables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Variables")
            .field("devices_count", &self.devices_count)
            .field("system_healthy", &self.system_healthy)
            .field("device_states", &self.device_states)
            .field("latency_samples_len", &self.latency_samples.len())
            .field("modbus_transactions_len", &self.modbus_transactions.len())
            .field("device_events_len", &self.device_events.len())
            .finish()
    }
}

impl Default for Variables {
    fn default() -> Self {
        Self {
            devices_count: AtomicU32::new(0),
            system_healthy: AtomicBool::new(true),
            device_states: Arc::new(RwLock::new(HashMap::new())),
            latency_samples: DataBuffer::bounded(100),
            modbus_transactions: DataBuffer::bounded(100),
            device_events: DataBuffer::bounded(100),
        }
    }
}
