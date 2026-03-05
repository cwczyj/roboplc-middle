//! Modbus worker modules

pub mod types;
pub mod client;
pub mod operations;
pub mod parsing;

// Re-export types from submodules for convenient access
pub use types::{ConnectionState, OperationQueue, TransactionId};

pub use client::{ModbusClient, ModbusOp, OperationResult, QueuedOperation};
pub use operations::{parse_register_address, RegisterType};
pub use parsing::{parse_signal_group_fields, ParsedField};

// ==================== 导入依赖 ====================

use crate::config::Device;
use crate::messages::Operation;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::event_matches;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Internal types (not re-exported)
use types::{Backoff, TimeoutHandler};

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
    #[allow(dead_code)]
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
        self.record_communication_with(latency_us, |sample: LatencySample| {
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
    /// Returns None for operations that don't map to Modbus directly
    fn operation_to_modbus_op(
        &self,
        operation: &Operation,
        params: &JsonValue,
    ) -> Option<ModbusOp> {
        match operation {
            Operation::ReadSignalGroup => {
                // ReadSignalGroup maps to a Modbus read based on signal group config
                let group_name = params.get("group_name")?.as_str()?;
                let group = self.device.signal_groups.iter().find(|g| g.name == group_name)?;
                let (_reg_type, addr) = parse_register_address(&group.register_address)?;
                Some(ModbusOp::ReadHolding {
                    address: addr,
                    count: group.register_count,
                })
            }
            Operation::WriteSignalGroup => {
                // WriteSignalGroup maps to a Modbus write based on signal group config
                let group_name = params.get("group_name")?.as_str()?;
                let data = params.get("values")?.as_array()?;
                let group = self.device.signal_groups.iter().find(|g| g.name == group_name)?;
                let addr = self.parse_address(&group.register_address)?;
                let values: Vec<u16> = data.iter().filter_map(|v| v.as_u64().map(|n| n as u16)).collect();
                Some(ModbusOp::WriteMultiple {
                    address: addr,
                    values,
                })
            }
            Operation::MoveTo | Operation::GetStatus => {
                // These operations are not directly Modbus operations
                // They are handled separately in the run() method
                None
            }
        }
    }

    /// Parse register address string (e.g., "h100" -> 100)
    fn parse_address(&self, addr_str: &str) -> Option<u16> {
        let (_, addr) = parse_register_address(addr_str)?;
        Some(addr)
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

                // Handle operations
                match operation {
                    Operation::GetStatus => {
                        // Return device connection status
                        let status = serde_json::json!({
                            "device_id": self.device.id,
                            "connected": self.connection_state == ConnectionState::Connected,
                            "connection_state": format!("{:?}", self.connection_state),
                            "last_communication": self.last_communication.map(|t| {
                                t.duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64
                            }),
                        });
                        context.hub().send(Message::DeviceResponse {
                            device_id: self.device.id.clone(),
                            success: true,
                            data: status,
                            error: None,
                            correlation_id,
                        });
                    }
                    Operation::MoveTo => {
                        // MoveTo is not supported for Modbus devices (PLC)
                        context.hub().send(Message::DeviceResponse {
                            device_id: self.device.id.clone(),
                            success: false,
                            data: JsonValue::Null,
                            error: Some("MoveTo operation not implemented for this device type".to_string()),
                            correlation_id,
                        });
                    }
                    Operation::ReadSignalGroup => {
                        let group_name = params.get("group_name").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(modbus_op) = self.operation_to_modbus_op(&operation, &params) {
                            let result = if let Some(client) = &mut self.client {
                                client.execute_operation(&modbus_op)
                            } else {
                                OperationResult {
                                    success: false,
                                    data: JsonValue::Null,
                                    error: Some("Client not connected".to_string()),
                                }
                            };
                            // Record latency if available
                            if let Some(latency) = result.data.get("latency_us").and_then(|v| v.as_u64()) {
                                self.record_communication(context, latency);
                            }
                            context.hub().send(Message::DeviceResponse {
                                device_id: self.device.id.clone(),
                                success: result.success,
                                data: serde_json::json!({
                                    "group_name": group_name,
                                    "result": result.data
                                }),
                                error: result.error,
                                correlation_id,
                            });
                        } else {
                            context.hub().send(Message::DeviceResponse {
                                device_id: self.device.id.clone(),
                                success: false,
                                data: JsonValue::Null,
                                error: Some(format!("Invalid signal group: {}", group_name)),
                                correlation_id,
                            });
                        }
                    }
                    Operation::WriteSignalGroup => {
                        let group_name = params.get("group_name").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(modbus_op) = self.operation_to_modbus_op(&operation, &params) {
                            let result = if let Some(client) = &mut self.client {
                                client.execute_operation(&modbus_op)
                            } else {
                                OperationResult {
                                    success: false,
                                    data: JsonValue::Null,
                                    error: Some("Client not connected".to_string()),
                                }
                            };
                            // Record latency if available
                            if let Some(latency) = result.data.get("latency_us").and_then(|v| v.as_u64()) {
                                self.record_communication(context, latency);
                            }
                            context.hub().send(Message::DeviceResponse {
                                device_id: self.device.id.clone(),
                                success: result.success,
                                data: serde_json::json!({
                                    "group_name": group_name,
                                    "result": result.data
                                }),
                                error: result.error,
                                correlation_id,
                            });
                        } else {
                            context.hub().send(Message::DeviceResponse {
                                device_id: self.device.id.clone(),
                                success: false,
                                data: JsonValue::Null,
                                error: Some(format!("Invalid signal group: {}", group_name)),
                                correlation_id,
                            });
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
    use crate::config::{DeviceType, SignalGroup};
    use crate::{DeviceEventType, LatencySample};

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
            signal_groups: Vec::<SignalGroup>::new(),
        }
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
    fn parse_address_handles_prefixes() {
        let worker = ModbusWorker::new(test_device());

        assert_eq!(worker.parse_address("h100"), Some(100));
        assert_eq!(worker.parse_address("H200"), Some(200));
        assert_eq!(worker.parse_address("i50"), Some(50));
        assert_eq!(worker.parse_address("c10"), Some(10));
        assert_eq!(worker.parse_address("d5"), Some(5));
        assert_eq!(worker.parse_address("100"), Some(100)); // No prefix = holding
    }
}