//! # Modbus Worker
//!
//! 管理 Modbus TCP 设备连接和通信的核心 worker。
//!
//! ## 功能
//!
//! - 维护与 Modbus 设备的 TCP 连接
//! - 处理来自 RpcWorker 的设备控制请求
//! - 实现指数退避重连机制
//! - 监控设备延迟和状态
//! - 发送周期性心跳
//!
//! ## 架构
//!
//! ModbusWorker 作为 RoboPLC worker 运行在独立的线程中。
//! 连接断开后会自动重连，使用指数退避策略避免雷群效应。
//!
//! ## 使用方式
//!
//! ModbusWorker 由 DeviceManager 创建和管理，无需手动实例化。

// ==================== 导入依赖 ====================

use crate::config::Device;
use crate::messages::Operation;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::comm::Client;
use roboplc::controller::prelude::*;
use roboplc::io::modbus::prelude::*;
use roboplc::io::IoMapping;
use roboplc::event_matches;
use roboplc::comm::tcp;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ==================== 常量定义 ====================

const BASE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const BACKOFF_BASE_MS: u64 = 100;
const BACKOFF_MAX_MS: u64 = 30000;

// 全局事务计数器

static TRANSACTION_COUNTER: AtomicU16 = AtomicU16::new(0);

// ==================== TransactionId ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionId {
    pub id: u16,
    pub created_at: SystemTime,
}

impl TransactionId {
    pub fn new() -> Self {
        Self {
            id: TRANSACTION_COUNTER.fetch_add(1, Ordering::SeqCst),
            created_at: SystemTime::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.created_at.elapsed().unwrap_or(Duration::ZERO)
    }
}

// ==================== ConnectionState ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

// ==================== Backoff 指数退避 ====================

#[derive(Debug, Clone, Copy)]
struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

impl Backoff {
    fn new() -> Self {
        Self {
            attempts: 0,
            next_delay_ms: BACKOFF_BASE_MS,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let jitter = (self.next_delay_ms / 10) * (self.attempts as u64 % 3);
        let delay = self.next_delay_ms + jitter;

        self.attempts += 1;
        self.next_delay_ms = (self.next_delay_ms * 2).min(BACKOFF_MAX_MS);

        Duration::from_millis(delay)
    }

    fn reset(&mut self) {
        self.attempts = 0;
        self.next_delay_ms = BACKOFF_BASE_MS;
    }
}

// ==================== TimeoutHandler ====================

#[derive(Debug, Clone, Copy)]
struct TimeoutHandler {
    current: Duration,
    base: Duration,
    max: Duration,
}

impl TimeoutHandler {
    fn new() -> Self {
        Self {
            current: BASE_TIMEOUT,
            base: BASE_TIMEOUT,
            max: MAX_TIMEOUT,
        }
    }

    fn timeout(&self) -> Duration {
        self.current
    }

    fn on_timeout(&mut self) {
        self.current = (self.current * 2).min(self.max);
    }

    fn on_success(&mut self) {
        self.current = self.base;
    }

    fn is_at_max(&self) -> bool {
        self.current >= self.max
    }
}

// ==================== OperationQueue ====================

struct OperationQueue<T> {
    pending: VecDeque<T>,
    in_flight: usize,
    max_in_flight: usize,
}

impl<T> OperationQueue<T> {
    fn new(max_in_flight: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: 0,
            max_in_flight,
        }
    }

    fn push(&mut self, op: T) {
        self.pending.push_back(op);
    }

    fn can_start(&self) -> bool {
        self.in_flight < self.max_in_flight
    }

    fn start_next(&mut self) -> Option<T> {
        if self.can_start() {
            if let Some(op) = self.pending.pop_front() {
                self.in_flight += 1;
                return Some(op);
            }
        }
        None
    }

    fn complete(&mut self) {
        if self.in_flight > 0 {
            self.in_flight -= 1;
        }
    }

    fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn in_flight_count(&self) -> usize {
        self.in_flight
    }
}

// ==================== ModbusOp ====================

#[derive(Debug, Clone)]
pub enum ModbusOp {
    ReadHolding { address: u16, count: u16 },
    WriteSingle { address: u16, value: u16 },
    WriteMultiple { address: u16, values: Vec<u16> },
}

/// Result of a Modbus operation
#[derive(Debug)]
struct OperationResult {
    success: bool,
    data: JsonValue,
    error: Option<String>,
}

/// Queued operation with tracking information
struct QueuedOperation {
    operation: ModbusOp,
    correlation_id: u64,
}

// ==================== ModbusClient ====================

struct ModbusClient {
    endpoint: String,
    connection: Option<Client>,
    unit_id: u8,
}

impl ModbusClient {
    fn new(endpoint: String, unit_id: u8) -> Self {
        Self {
            endpoint,
            connection: None,
            unit_id,
        }
    }

    fn connect(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let client = tcp::connect(&self.endpoint, timeout)?;
        client.connect()?;
        self.connection = Some(client);
        Ok(())
    }

    fn reconnect(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = &self.connection {
            client.reconnect();
        }
        self.connection = None;
        self.connect(timeout)
    }

    fn ensure_connected(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        match &self.connection {
            Some(client) => {
                if client.connect().is_err() {
                    self.reconnect(timeout)?;
                }
            }
            None => {
                self.connect(timeout)?;
            }
        }
        Ok(())
    }

    fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
        let client = match &self.connection {
            Some(c) => c.clone(),
            None => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Not connected".to_string()),
                }
            }
        };

        match op {
            ModbusOp::ReadHolding { address, count } => {
                self.read_holding(&client, *address, *count)
            }
            ModbusOp::WriteSingle { address, value } => {
                self.write_single(&client, *address, *value)
            }
            ModbusOp::WriteMultiple { address, values } => {
                self.write_multiple(&client, *address, values)
            }
        }
    }

    fn read_holding(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let mut mapping = mapping;
        let start = SystemTime::now();
        // Read raw register values
        let mut values = Vec::with_capacity(count as usize);
        let mut all_success = true;
        for i in 0..count {
            let reg = ModbusRegister::new(ModbusRegisterKind::Holding, address + i);
            if let Ok(m) = ModbusMapping::create(client, self.unit_id, reg, 1) {
                let mut m = m;
                match m.read::<u16>() {
                    Ok(v) => values.push(v),
                    Err(_) => all_success = false,
                }
            }
        }

        if all_success && values.len() == count as usize {
            let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
            OperationResult {
                success: true,
                data: serde_json::json!({
                    "values": values,
                    "latency_us": latency
                }),
                error: None,
            }
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to read all registers".to_string()),
            }
        }
    }

    fn write_single(&self, client: &Client, address: u16, value: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, 1) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let mut mapping = mapping;
        let start = SystemTime::now();

        // Write single u16 value
        match mapping.write(value) {
            Ok(()) => {
                let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "value": value,
                        "latency_us": latency
                    }),
                    error: None,
                }
            }
            Err(e) => OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Write failed: {}", e)),
            },
        }
    }

    fn write_multiple(&self, client: &Client, address: u16, values: &[u16]) -> OperationResult {
        let count = values.len() as u16;
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let mut mapping = mapping;
        let start = SystemTime::now();

        // Write the values as Vec<u16>
        match mapping.write(values.to_vec()) {
            Ok(()) => {
                let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "count": count,
                        "latency_us": latency
                    }),
                    error: None,
                }
            }
            Err(e) => OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Write multiple failed: {}", e)),
            },
        }
    }
}

// ==================== ModbusWorker ====================

#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    device: Device,
    client: Option<ModbusClient>,
    connection_state: ConnectionState,
    last_communication: Option<SystemTime>,
    last_heartbeat: SystemTime,
    pending_transactions: HashMap<u16, TransactionId>,
    operation_queue: OperationQueue<QueuedOperation>,
    backoff: Backoff,
    timeout_handler: TimeoutHandler,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        let max_in_flight = device.max_concurrent_ops as usize;

        Self {
            device,
            client: None,
            connection_state: ConnectionState::Disconnected,
            last_communication: None,
            last_heartbeat: SystemTime::UNIX_EPOCH,
            pending_transactions: HashMap::new(),
            operation_queue: OperationQueue::new(max_in_flight),
            backoff: Backoff::new(),
            timeout_handler: TimeoutHandler::new(),
        }
    }

    fn track_transaction(&mut self) -> TransactionId {
        let tx = TransactionId::new();
        self.pending_transactions.insert(tx.id, tx);
        tx
    }

    fn prune_stale_transactions(&mut self, max_age: Duration) {
        self.pending_transactions
            .retain(|_, tx| tx.elapsed() <= max_age);
    }

    fn connect(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = format!("{}:{}", self.device.address, self.device.port);
        let mut client = ModbusClient::new(endpoint, self.device.unit_id);
        client.connect(timeout)?;
        self.client = Some(client);
        tracing::info!(device_id = %self.device.id, "Connected to Modbus device");
        Ok(())
    }

    fn update_connection_state_with<F>(&mut self, new_state: ConnectionState, mut emit: F)
    where
        F: FnMut(DeviceEvent),
    {
        if self.connection_state != new_state {
            let event_type = match new_state {
                ConnectionState::Connected => DeviceEventType::Connected,
                ConnectionState::Disconnected => DeviceEventType::Disconnected,
                ConnectionState::Connecting => DeviceEventType::Reconnecting,
            };

            emit(DeviceEvent {
                device_id: self.device.id.clone(),
                event_type,
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                details: format!("Connection state: {:?}", new_state),
            });

            self.connection_state = new_state;
        }
    }

    fn update_connection_state(
        &mut self,
        new_state: ConnectionState,
        context: &Context<Message, Variables>,
    ) {
        self.update_connection_state_with(new_state, |event| {
            let _ = context.variables().device_events.force_push(event);
        });
    }

    fn record_communication_with<F>(&mut self, latency_us: u64, mut emit: F)
    where
        F: FnMut(LatencySample),
    {
        let now = SystemTime::now();
        self.last_communication = Some(now);

        let sample = LatencySample {
            device_id: 0,
            latency_us,
            timestamp_ms: now
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        emit(sample);
    }

    fn record_communication(&mut self, context: &Context<Message, Variables>, latency_us: u64) {
        self.record_communication_with(latency_us, |sample| {
            let _ = context.variables().latency_samples.force_push(sample);
        });
    }

    fn ensure_connected(&mut self, context: &Context<Message, Variables>) -> bool {
        let timeout = self.timeout_handler.timeout();

        if self.client.is_none() {
            self.update_connection_state(ConnectionState::Connecting, context);
            if let Err(e) = self.connect(timeout) {
                tracing::warn!(device_id = %self.device.id, error = %e, "Connection failed");
                self.timeout_handler.on_timeout();
                if self.timeout_handler.is_at_max() {
                    tracing::warn!(
                        device_id = %self.device.id,
                        timeout_s = self.timeout_handler.timeout().as_secs(),
                        "Adaptive Modbus timeout reached max"
                    );
                }
                self.update_connection_state(ConnectionState::Disconnected, context);
                return false;
            }
        }

        let reconnect_failed = if let Some(client) = &mut self.client {
            client.ensure_connected(timeout).is_err()
        } else {
            false
        };

        if reconnect_failed {
            self.client = None;
            self.update_connection_state(ConnectionState::Connecting, context);
            if let Err(e) = self.connect(timeout) {
                tracing::warn!(device_id = %self.device.id, error = %e, "Reconnection failed");
                self.timeout_handler.on_timeout();
                if self.timeout_handler.is_at_max() {
                    tracing::warn!(
                        device_id = %self.device.id,
                        timeout_s = self.timeout_handler.timeout().as_secs(),
                        "Adaptive Modbus timeout reached max"
                    );
                }
                self.update_connection_state(ConnectionState::Disconnected, context);
                return false;
            }
        }

        self.update_connection_state(ConnectionState::Connected, context);
        self.timeout_handler.on_success();
        self.backoff.reset();
        true
    }

    /// Convert Operation and params to ModbusOp
    fn operation_to_modbus_op(
        &self,
        operation: &Operation,
        params: &JsonValue,
    ) -> Option<ModbusOp> {
        match operation {
            Operation::SetRegister => {
                let address = params.get("address")?.as_str()?;
                let value = params.get("value")?.as_u64()? as u16;
                let addr = self.parse_address(address)?;
                Some(ModbusOp::WriteSingle {
                    address: addr,
                    value,
                })
            }
            Operation::GetRegister => {
                let address = params.get("address")?.as_str()?;
                let addr = self.parse_address(address)?;
                Some(ModbusOp::ReadHolding {
                    address: addr,
                    count: 1,
                })
            }
            Operation::WriteBatch => {
                let values = params.get("values")?.as_array()?;
                let ops: Vec<(u16, u16)> = values
                    .iter()
                    .filter_map(|v| {
                        let addr = v.get(0)?.as_str().and_then(|a| self.parse_address(a))?;
                        let val = v.get(1)?.as_u64()? as u16;
                        Some((addr, val))
                    })
                    .collect();

                // For simplicity, batch writes are handled as multiple single writes
                // The first write is returned for queueing
                if let Some((addr, val)) = ops.first() {
                    Some(ModbusOp::WriteSingle {
                        address: *addr,
                        value: *val,
                    })
                } else {
                    None
                }
            }
            Operation::ReadBatch => {
                let addresses = params.get("addresses")?.as_array()?;
                if let Some(addr_str) = addresses.first().and_then(|v| v.as_str()) {
                    let addr = self.parse_address(addr_str)?;
                    Some(ModbusOp::ReadHolding {
                        address: addr,
                        count: addresses.len() as u16,
                    })
                } else {
                    None
                }
            }
            Operation::MoveTo | Operation::GetStatus => {
                // These operations are not directly Modbus operations
                // They might be handled differently or return an error
                None
            }
        }
    }

    /// Parse register address string (e.g., "h100" -> 100)
    fn parse_address(&self, addr_str: &str) -> Option<u16> {
        let addr_str = addr_str.trim();
        if addr_str.is_empty() {
            return None;
        }

        // Check for prefix (h, i, c, d)
        let (prefix, num_part) = if addr_str.starts_with('h') || addr_str.starts_with('H') {
            ('h', &addr_str[1..])
        } else if addr_str.starts_with('i') || addr_str.starts_with('I') {
            ('i', &addr_str[1..])
        } else if addr_str.starts_with('c') || addr_str.starts_with('C') {
            ('c', &addr_str[1..])
        } else if addr_str.starts_with('d') || addr_str.starts_with('D') {
            ('d', &addr_str[1..])
        } else {
            // No prefix, assume holding register
            ('h', addr_str)
        };

        num_part.parse::<u16>().ok()
    }

    /// Handle ReadBatch operation: read multiple addresses
    fn handle_read_batch(&mut self, params: &JsonValue, _correlation_id: u64) -> OperationResult {
        let start = SystemTime::now();
        
        let addresses = match params.get("addresses").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Missing or invalid 'addresses' array".to_string()),
                }
            }
        };

        if addresses.is_empty() {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Empty addresses array".to_string()),
            };
        }

        let mut results = Vec::new();
        let mut all_success = true;

        for addr_value in addresses {
            let addr_str = match addr_value.as_str() {
                Some(s) => s,
                None => {
                    all_success = false;
                    results.push(serde_json::json!({
                        "address": addr_value.to_string(),
                        "success": false,
                        "error": "Invalid address format"
                    }));
                    continue;
                }
            };

            let addr = match self.parse_address(addr_str) {
                Some(a) => a,
                None => {
                    all_success = false;
                    results.push(serde_json::json!({
                        "address": addr_str,
                        "success": false,
                        "error": "Failed to parse address"
                    }));
                    continue;
                }
            };

            // Execute read operation
            let op = ModbusOp::ReadHolding { address: addr, count: 1 };
            let result = if let Some(client) = &mut self.client {
                client.execute_operation(&op)
            } else {
                OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Client not connected".to_string()),
                }
            };

            if result.success {
                let value = result.data.get("values").and_then(|v| v.as_array()).and_then(|arr| arr.first());
                results.push(serde_json::json!({
                    "address": addr_str,
                    "success": true,
                    "value": value
                }));
            } else {
                all_success = false;
                results.push(serde_json::json!({
                    "address": addr_str,
                    "success": false,
                    "error": result.error.unwrap_or_else(|| "Unknown error".to_string())
                }));
            }
        }

        let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
        OperationResult {
            success: all_success,
            data: serde_json::json!({
                "results": results,
                "total": addresses.len(),
                "successful": results.iter().filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count(),
                "latency_us": latency
            }),
            error: if all_success { None } else { Some("Some reads failed".to_string()) },
        }
    }

    /// Handle WriteBatch operation: write multiple (address, value) pairs
    fn handle_write_batch(&mut self, params: &JsonValue, _correlation_id: u64) -> OperationResult {
        let start = SystemTime::now();
        
        let values = match params.get("values").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Missing or invalid 'values' array".to_string()),
                }
            }
        };

        if values.is_empty() {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Empty values array".to_string()),
            };
        }

        let mut results = Vec::new();
        let mut all_success = true;

        for pair in values {
            let addr_str = match pair.get(0).and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    all_success = false;
                    results.push(serde_json::json!({
                        "address": "unknown",
                        "success": false,
                        "error": "Invalid address in pair"
                    }));
                    continue;
                }
            };

            let value = match pair.get(1).and_then(|v| v.as_u64()) {
                Some(v) => v as u16,
                None => {
                    all_success = false;
                    results.push(serde_json::json!({
                        "address": addr_str,
                        "success": false,
                        "error": "Invalid value in pair"
                    }));
                    continue;
                }
            };

            let addr = match self.parse_address(addr_str) {
                Some(a) => a,
                None => {
                    all_success = false;
                    results.push(serde_json::json!({
                        "address": addr_str,
                        "success": false,
                        "error": "Failed to parse address"
                    }));
                    continue;
                }
            };

            // Execute write operation
            let op = ModbusOp::WriteSingle { address: addr, value };
            let result = if let Some(client) = &mut self.client {
                client.execute_operation(&op)
            } else {
                OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Client not connected".to_string()),
                }
            };

            if result.success {
                results.push(serde_json::json!({
                    "address": addr_str,
                    "value": value,
                    "success": true
                }));
            } else {
                all_success = false;
                results.push(serde_json::json!({
                    "address": addr_str,
                    "value": value,
                    "success": false,
                    "error": result.error.unwrap_or_else(|| "Unknown error".to_string())
                }));
            }
        }

        let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
        OperationResult {
            success: all_success,
            data: serde_json::json!({
                "results": results,
                "total": values.len(),
                "successful": results.iter().filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count(),
                "latency_us": latency
            }),
            error: if all_success { None } else { Some("Some writes failed".to_string()) },
        }
    }
}

// ==================== Worker trait 实现 ====================

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let worker_name = format!("modbus_worker_{}", self.device.id);

        let hub_client = context
            .hub()
            .register(&worker_name, event_matches!(Message::DeviceControl { .. }))?;

        tracing::info!(device_id = %self.device.id, "ModbusWorker started");

        for msg in hub_client {
            if !context.is_online() {
                break;
            }

            self.prune_stale_transactions(Duration::from_secs(5));

            // Handle DeviceControl messages
            if let Message::DeviceControl {
                device_id,
                operation,
                params,
                correlation_id,
                respond_to: _,
            } = msg
            {
                // Verify this message is for our device
                if device_id != self.device.id {
                    tracing::warn!(
                        received_device_id = %device_id,
                        expected_device_id = %self.device.id,
                        "Received DeviceControl for wrong device"
                    );
                    continue;
                }

                tracing::debug!(
                    device_id = %device_id,
                    operation = ?operation,
                    correlation_id = correlation_id,
                    "Received DeviceControl"
                );

                // Ensure we're connected before processing
                if !self.ensure_connected(context) {
                    // Send error response
                    context.hub().send(Message::DeviceResponse {
                        device_id: self.device.id.clone(),
                        success: false,
                        data: JsonValue::Null,
                        error: Some("Device not connected".to_string()),
                        correlation_id,
                    });
                    continue;
                }

                // Handle batch operations specially
                match operation {
                    Operation::ReadBatch => {
                        let result = self.handle_read_batch(&params, correlation_id);
                        if let Some(latency) =
                            result.data.get("latency_us").and_then(|v| v.as_u64())
                        {
                            self.record_communication(context, latency);
                        }
                        context.hub().send(Message::DeviceResponse {
                            device_id: self.device.id.clone(),
                            success: result.success,
                            data: result.data,
                            error: result.error,
                            correlation_id,
                        });
                    }
                    Operation::WriteBatch => {
                        let result = self.handle_write_batch(&params, correlation_id);
                        if let Some(latency) =
                            result.data.get("latency_us").and_then(|v| v.as_u64())
                        {
                            self.record_communication(context, latency);
                        }
                        context.hub().send(Message::DeviceResponse {
                            device_id: self.device.id.clone(),
                            success: result.success,
                            data: result.data,
                            error: result.error,
                            correlation_id,
                        });
                    }
                    _ => {
                        // Non-batch operations: use the queue
                        if let Some(modbus_op) = self.operation_to_modbus_op(&operation, &params) {
                            let queued_op = QueuedOperation {
                                operation: modbus_op,
                                correlation_id,
                            };
                            self.operation_queue.push(queued_op);
                        } else {
                            // Unsupported operation
                            context.hub().send(Message::DeviceResponse {
                                device_id: self.device.id.clone(),
                                success: false,
                                data: JsonValue::Null,
                                error: Some(format!("Unsupported operation: {:?}", operation)),
                                correlation_id,
                            });
                            continue;
                        }
                        // Process operations from the queue
                        while self.operation_queue.can_start() {
                            if let Some(queued_op) = self.operation_queue.start_next() {
                                let result = if let Some(client) = &mut self.client {
                                    client.execute_operation(&queued_op.operation)
                                } else {
                                    OperationResult {
                                        success: false,
                                        data: JsonValue::Null,
                                        error: Some("Client not connected".to_string()),
                                    }
                                };
                                // Record latency if available
                                if let Some(latency) =
                                    result.data.get("latency_us").and_then(|v| v.as_u64())
                                {
                                    self.record_communication(context, latency);
                                }
                                // Send response
                                context.hub().send(Message::DeviceResponse {
                                    device_id: self.device.id.clone(),
                                    success: result.success,
                                    data: result.data,
                                    error: result.error,
                                    correlation_id: queued_op.correlation_id,
                                });
                                self.operation_queue.complete();
                            }
                        }
                    }
                }
            }

            // Handle heartbeat
            let now = SystemTime::now();
            if now
                .duration_since(self.last_heartbeat)
                .unwrap_or(Duration::ZERO)
                >= Duration::from_secs(self.device.heartbeat_interval_sec as u64)
            {
                let _tx_id = self.track_transaction();
                let timestamp_ms = now
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                context.hub().send(Message::DeviceHeartbeat {
                    device_id: self.device.id.clone(),
                    timestamp_ms,
                    latency_us: 0,
                });
                self.record_communication(context, 0);
                self.last_heartbeat = now;
            }
        }

        tracing::info!(device_id = %self.device.id, "ModbusWorker stopped");
        Ok(())
    }
}

// ==================== 单元测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceType, RegisterMapping};
    use crate::{DeviceEventType, LatencySample};

    #[test]
    fn transaction_id_increments() {
        let id1 = TransactionId::new();
        let id2 = TransactionId::new();
        assert_ne!(id1.id, id2.id);
    }

    #[test]
    fn transaction_id_has_timestamp() {
        let id = TransactionId::new();
        assert!(id.elapsed() < Duration::from_secs(1));
    }

    fn test_device() -> Device {
        Device {
            id: "test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: Default::default(),
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            heartbeat_interval_sec: 30,
            register_mappings: Vec::<RegisterMapping>::new(),
        }
    }

    #[test]
    fn modbus_client_new_starts_disconnected() {
        let client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        assert!(client.connection.is_none());
        assert_eq!(client.endpoint, "127.0.0.1:502");
        assert_eq!(client.unit_id, 1);
    }

    #[test]
    fn worker_new_initializes_without_client() {
        let worker = ModbusWorker::new(test_device());

        assert!(worker.client.is_none());
        assert_eq!(worker.connection_state, ConnectionState::Disconnected);
        assert!(worker.last_communication.is_none());
        assert!(worker.pending_transactions.is_empty());
    }

    #[test]
    fn update_connection_state_emits_event_on_transition_only() {
        let mut worker = ModbusWorker::new(test_device());
        let mut emitted = Vec::new();

        worker
            .update_connection_state_with(ConnectionState::Connected, |event| emitted.push(event));
        worker
            .update_connection_state_with(ConnectionState::Connected, |event| emitted.push(event));
        worker
            .update_connection_state_with(ConnectionState::Connecting, |event| emitted.push(event));

        assert_eq!(emitted.len(), 2);
        assert!(matches!(emitted[0].event_type, DeviceEventType::Connected));
        assert!(matches!(
            emitted[1].event_type,
            DeviceEventType::Reconnecting
        ));
        assert_eq!(worker.connection_state, ConnectionState::Connecting);
    }

    #[test]
    fn record_communication_updates_timestamp_and_latency_sample() {
        let mut worker = ModbusWorker::new(test_device());
        let before = SystemTime::now();
        let mut emitted_sample: Option<LatencySample> = None;

        worker.record_communication_with(250, |sample| emitted_sample = Some(sample));

        assert!(worker.last_communication.is_some());
        assert!(worker.last_communication.unwrap() >= before);

        let sample = emitted_sample.expect("latency sample should be emitted");
        assert_eq!(sample.latency_us, 250);
        assert_eq!(sample.device_id, 0);
        assert!(sample.timestamp_ms > 0);
    }

    #[test]
    fn backoff_new_starts_at_base_delay() {
        let backoff = Backoff::new();

        assert_eq!(backoff.attempts, 0);
        assert_eq!(backoff.next_delay_ms, BACKOFF_BASE_MS);
    }

    #[test]
    fn backoff_next_delay_is_exponential_and_capped() {
        let mut backoff = Backoff::new();

        let d1 = backoff.next_delay();
        let d2 = backoff.next_delay();
        let d3 = backoff.next_delay();

        assert_eq!(d1, Duration::from_millis(100));
        assert_eq!(d2, Duration::from_millis(220));
        assert_eq!(d3, Duration::from_millis(480));

        for _ in 0..20 {
            backoff.next_delay();
        }

        assert!(backoff.next_delay_ms <= BACKOFF_MAX_MS);
    }

    #[test]
    fn backoff_reset_restores_initial_state() {
        let mut backoff = Backoff::new();
        let _ = backoff.next_delay();
        let _ = backoff.next_delay();

        backoff.reset();

        assert_eq!(backoff.attempts, 0);
        assert_eq!(backoff.next_delay_ms, BACKOFF_BASE_MS);
    }

    #[test]
    fn operation_queue_limits_concurrency_and_tracks_in_flight() {
        let mut queue = OperationQueue::new(2);
        queue.push(QueuedOperation {
            operation: ModbusOp::ReadHolding {
                address: 100,
                count: 2,
            },
            correlation_id: 1,
        });
        queue.push(QueuedOperation {
            operation: ModbusOp::WriteSingle {
                address: 101,
                value: 42,
            },
            correlation_id: 2,
        });
        queue.push(QueuedOperation {
            operation: ModbusOp::WriteMultiple {
                address: 102,
                values: vec![1, 2, 3],
            },
            correlation_id: 3,
        });

        assert_eq!(queue.pending_count(), 3);
        assert_eq!(queue.in_flight_count(), 0);
        assert!(queue.can_start());

        let op1 = queue.start_next();
        let op2 = queue.start_next();
        let op3 = queue.start_next();

        assert!(op1.is_some());
        assert!(op2.is_some());
        assert!(op3.is_none());
        assert_eq!(queue.in_flight_count(), 2);
        assert_eq!(queue.pending_count(), 1);
        assert!(!queue.can_start());
    }

    #[test]
    fn operation_queue_complete_allows_next_queued_operation() {
        let mut queue = OperationQueue::new(1);
        queue.push(QueuedOperation {
            operation: ModbusOp::ReadHolding {
                address: 200,
                count: 1,
            },
            correlation_id: 1,
        });
        queue.push(QueuedOperation {
            operation: ModbusOp::WriteSingle {
                address: 201,
                value: 7,
            },
            correlation_id: 2,
        });

        let first = queue.start_next();
        let blocked = queue.start_next();

        assert!(first.is_some());
        assert!(blocked.is_none());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 1);

        queue.complete();
        let second = queue.start_next();

        assert!(second.is_some());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 0);
    }

    #[test]
    fn operation_queue_complete_is_saturating_at_zero() {
        let mut queue: OperationQueue<QueuedOperation> = OperationQueue::new(1);

        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);

        queue.push(QueuedOperation {
            operation: ModbusOp::ReadHolding {
                address: 300,
                count: 1,
            },
            correlation_id: 1,
        });
        let _ = queue.start_next();
        assert_eq!(queue.in_flight_count(), 1);

        queue.complete();
        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);
    }

    #[test]
    fn timeout_handler_doubles_on_timeout_until_max() {
        let mut handler = TimeoutHandler::new();

        assert_eq!(handler.timeout(), BASE_TIMEOUT);

        for _ in 0..10 {
            handler.on_timeout();
        }

        assert_eq!(handler.timeout(), MAX_TIMEOUT);
        assert!(handler.is_at_max());
    }

    #[test]
    fn timeout_handler_resets_to_base_after_success() {
        let mut handler = TimeoutHandler::new();

        handler.on_timeout();
        handler.on_timeout();
        assert!(handler.timeout() > BASE_TIMEOUT);

        handler.on_success();

        assert_eq!(handler.timeout(), BASE_TIMEOUT);
        assert!(!handler.is_at_max());
    }

    #[test]
    fn parse_address_handles_prefixes() {
        let worker = ModbusWorker::new(test_device());

        assert_eq!(worker.parse_address("h100"), Some(100));
        assert_eq!(worker.parse_address("H200"), Some(200));
        assert_eq!(worker.parse_address("i50"), Some(50));
        assert_eq!(worker.parse_address("c10"), Some(10));
        assert_eq!(worker.parse_address("d5"), Some(5));
        assert_eq!(worker.parse_address("100"), Some(100)); // No prefix = holding
    }

    #[test]
    fn operation_to_modbus_op_set_register() {
        let worker = ModbusWorker::new(test_device());
        let params = serde_json::json!({ "address": "h100", "value": 42 });

        let result = worker.operation_to_modbus_op(&Operation::SetRegister, &params);

        assert!(matches!(
            result,
            Some(ModbusOp::WriteSingle {
                address: 100,
                value: 42
            })
        ));
    }

    #[test]
    fn operation_to_modbus_op_get_register() {
        let worker = ModbusWorker::new(test_device());
        let params = serde_json::json!({ "address": "h200" });

        let result = worker.operation_to_modbus_op(&Operation::GetRegister, &params);

        assert!(matches!(
            result,
            Some(ModbusOp::ReadHolding {
                address: 200,
                count: 1
            })
        ));
    }
}
