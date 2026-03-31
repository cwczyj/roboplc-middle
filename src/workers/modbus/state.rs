//! ModbusWorker state management and connection handling

use crate::config::Device;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{
    Backoff, ConnectionState, ModbusConnectionPool, ModbusOp, OperationQueue, OperationResult,
    TimeoutHandler,
};

/// ModbusWorker state struct
pub struct ModbusWorkerState {
    device: Device,
    connection_pool: Option<ModbusConnectionPool>,
    connection_state: ConnectionState,
    last_communication: Option<SystemTime>,
    backoff: Backoff,
    timeout_handler: TimeoutHandler,
    /// Operation queue for controlling concurrent operations
    operation_queue: OperationQueue<()>,
}

impl ModbusWorkerState {
    pub fn new(device: Device) -> Self {
        let max_concurrent_ops = device.max_concurrent_ops as usize;
        Self {
            device,
            connection_pool: None,
            connection_state: ConnectionState::Disconnected,
            last_communication: None,
            backoff: Backoff::new(),
            timeout_handler: TimeoutHandler::new(),
            operation_queue: OperationQueue::new(max_concurrent_ops),
        }
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Update device configuration (signal_groups) at runtime
    pub fn update_device_config(&mut self, new_device: Device) {
        // Only update signal_groups, keep connection state
        tracing::info!(
            device_id = %self.device.id,
            old_signal_groups = self.device.signal_groups.len(),
            new_signal_groups = new_device.signal_groups.len(),
            "Updating device signal_groups"
        );
        self.device.signal_groups = new_device.signal_groups;
        // Keep other device properties like id, address, etc. unchanged
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.connection_state
    }

    fn connect_pool(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = format!("{}:{}", self.device.address, self.device.port);
        let pool_size = self.device.max_concurrent_ops as usize;
        // Cap pool size at 5 to avoid too many connections
        let capped_pool_size = pool_size.min(5);

        // Try to establish a single connection first to verify device is reachable
        // This uses the same timeout approach as before for validation
        let test_client = roboplc::comm::tcp::connect(&endpoint, timeout)?;
        test_client.connect()?;

        // If we can connect, create the pool (pool connections are lazy)
        let pool = ModbusConnectionPool::new(endpoint, self.device.unit_id, capped_pool_size);
        self.connection_pool = Some(pool);
        tracing::info!(
            device_id = %self.device.id,
            pool_size = capped_pool_size,
            "Created Modbus connection pool"
        );
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

    pub fn update_connection_state(
        &mut self,
        new_state: ConnectionState,
        context: &Context<Message, Variables>,
    ) {
        let device_id = self.device.id.clone();
        self.update_connection_state_with(new_state, |event| {
            if !context.variables().device_events.force_push(event) {
                tracing::warn!(
                    device_id = %device_id,
                    "Device event buffer full, oldest event dropped"
                );
            }
        });
    }

    fn record_communication_with<F>(&mut self, latency_us: u64, mut emit: F)
    where
        F: FnMut(LatencySample),
    {
        let now = SystemTime::now();
        self.last_communication = Some(now);

        let sample = LatencySample {
            device_id: self.device.id.clone(),
            latency_us,
            timestamp_ms: now
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        emit(sample);
    }

    pub fn record_communication(&mut self, context: &Context<Message, Variables>, latency_us: u64) {
        let device_id = self.device.id.clone();
        self.record_communication_with(latency_us, |sample: LatencySample| {
            // if !context.variables().latency_samples.force_push(sample) {
            //     tracing::warn!(
            //         device_id = %device_id,
            //         "Latency samples buffer full, oldest sample dropped"
            //     );
            // }
        });
    }

    pub fn ensure_connected(&mut self, context: &Context<Message, Variables>) -> bool {
        let timeout = self.timeout_handler.timeout();

        if self.connection_pool.is_none() {
            self.update_connection_state(ConnectionState::Connecting, context);
            if let Err(e) = self.connect_pool(timeout) {
                tracing::warn!(device_id = %self.device.id, error = %e, "Connection pool creation failed");
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

    /// Execute operation with connection pool
    ///
    /// Delegates to ModbusConnectionPool's execute_operation which handles:
    /// - Connection acquisition from pool
    /// - Health checking (age, is_healthy flag)
    /// - Connection release back to pool
    pub fn execute_operation(
        &mut self,
        context: &Context<Message, Variables>,
        modbus_op: &ModbusOp,
    ) -> OperationResult {
        if !self.ensure_connected(context) {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to establish connection pool".to_string()),
            };
        }

        if let Some(pool) = &mut self.connection_pool {
            pool.execute_operation(modbus_op)
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Connection pool not available".to_string()),
            }
        }
    }

    /// Try to acquire capacity for a new operation.
    /// Returns true if capacity is available, false if at max concurrent ops.
    pub fn try_acquire_operation(&mut self) -> bool {
        self.operation_queue.push(());
        self.operation_queue.pop_if_ready().is_some()
    }

    /// Mark an operation as complete, releasing capacity.
    pub fn complete_operation(&mut self) {
        self.operation_queue.complete();
    }
}
