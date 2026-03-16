//! ModbusWorker state management and connection handling

use crate::config::Device;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Backoff, ConnectionState, ModbusClient, ModbusOp, OperationResult, TimeoutHandler};

/// ModbusWorker state struct
pub struct ModbusWorkerState {
    device: Device,
    client: Option<ModbusClient>,
    connection_state: ConnectionState,
    last_communication: Option<SystemTime>,
    backoff: Backoff,
    timeout_handler: TimeoutHandler,
}

impl ModbusWorkerState {
    pub fn new(device: Device) -> Self {
        Self {
            device,
            client: None,
            connection_state: ConnectionState::Disconnected,
            last_communication: None,
            backoff: Backoff::new(),
            timeout_handler: TimeoutHandler::new(),
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

    pub fn client(&self) -> Option<&ModbusClient> {
        self.client.as_ref()
    }

    pub fn client_mut(&mut self) -> Option<&mut ModbusClient> {
        self.client.as_mut()
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.connection_state
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

    pub fn update_connection_state(
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
        self.record_communication_with(latency_us, |sample: LatencySample| {
            let _ = context.variables().latency_samples.force_push(sample);
        });
    }

    pub fn ensure_connected(&mut self, context: &Context<Message, Variables>) -> bool {
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

        self.update_connection_state(ConnectionState::Connected, context);
        self.timeout_handler.on_success();
        self.backoff.reset();
        true
    }

    /// Execute operation with connection probe
    /// 
    /// Delegates to ModbusClient's execute_operation which handles:
    /// - Connection probing to detect stale connections
    /// - Automatic reconnection on failure
    pub fn execute_operation(
        &mut self,
        context: &Context<Message, Variables>,
        modbus_op: &ModbusOp,
    ) -> OperationResult {
        // Ensure we have a connection before executing
        if !self.ensure_connected(context) {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to establish connection".to_string()),
            };
        }

        // Client's execute_operation will probe connection and reconnect if needed
        if let Some(client) = &mut self.client {
            client.execute_operation(modbus_op)
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Client not connected".to_string()),
            }
        }
    }
}