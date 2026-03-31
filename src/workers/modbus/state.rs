//! ModbusWorker state management and connection handling

use crate::config::Device;
use crate::{DeviceEvent, DeviceEventType, Message, Variables};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;
use std::sync::{Arc, Mutex};
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
    backoff: Backoff,
    timeout_handler: TimeoutHandler,
    operation_queue: Arc<Mutex<OperationQueue<()>>>,
}

impl ModbusWorkerState {
    pub fn new(device: Device) -> Self {
        let max_concurrent_ops = device.max_concurrent_ops as usize;
        Self {
            device,
            connection_pool: None,
            connection_state: ConnectionState::Disconnected,
            backoff: Backoff::new(),
            timeout_handler: TimeoutHandler::new(),
            operation_queue: Arc::new(Mutex::new(OperationQueue::new(max_concurrent_ops))),
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
        let requested_pool_size = self.device.max_concurrent_ops as usize;
        let max_pool_size = self.device.max_pool_size as usize;

        let pool_size = requested_pool_size.min(max_pool_size);

        // Try to establish a single connection first to verify device is reachable
        let test_client = roboplc::comm::tcp::connect(&endpoint, timeout)?;
        test_client.connect()?;

        // If we can connect, create the pool (pool connections are lazy)
        let pool = ModbusConnectionPool::new(endpoint, self.device.unit_id, pool_size);
        self.connection_pool = Some(pool);
        tracing::info!(
            device_id = %self.device.id,
            pool_size = pool_size,
            requested = requested_pool_size,
            max_allowed = max_pool_size,
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

    pub fn record_communication(
        &mut self,
        _context: &Context<Message, Variables>,
        _latency_us: u64,
    ) {
        // Latency is now tracked via HeartbeatWorker -> LatencyMonitor
        // See: DeviceHeartbeat message flow
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

        if let Some(pool) = &self.connection_pool {
            pool.execute_operation(modbus_op)
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Connection pool not available".to_string()),
            }
        }
    }

    /// Get reference to connection pool for async execution.
    /// Returns None if not connected.
    pub fn get_pool(&self) -> Option<&ModbusConnectionPool> {
        self.connection_pool.as_ref()
    }

    /// Try to acquire capacity for a new operation.
    /// Returns true if capacity is available, false if at max concurrent ops.
    pub fn try_acquire_operation(&mut self) -> bool {
        let mut queue = self.operation_queue.lock().unwrap();
        queue.push(());
        queue.pop_if_ready().is_some()
    }

    /// Thread-safe capacity acquisition for async operations.
    /// Atomically checks and increments capacity without requiring mutable access.
    pub fn try_acquire_operation_atomic(&self) -> bool {
        let queue = self.operation_queue.lock().unwrap();
        queue.try_acquire_atomic()
    }

    /// Mark an operation as complete, releasing capacity (thread-safe).
    pub fn complete_operation(&self) {
        let queue = self.operation_queue.lock().unwrap();
        queue.complete();
    }

    /// Get a clone of the operation queue Arc for use in async contexts.
    pub fn operation_queue_arc(&self) -> Arc<Mutex<OperationQueue<()>>> {
        self.operation_queue.clone()
    }

    /// Get current in-flight operation count (for monitoring).
    pub fn in_flight_count(&self) -> usize {
        let queue = self.operation_queue.lock().unwrap();
        queue.in_flight_count()
    }

    /// Execute operation directly on connection pool (bypasses ensure_connected).
    /// Used by async tasks that have already verified connection.
    pub fn execute_operation_direct(&self, modbus_op: &ModbusOp) -> OperationResult {
        if let Some(pool) = &self.connection_pool {
            pool.execute_operation(modbus_op)
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Connection pool not available".to_string()),
            }
        }
    }
}
