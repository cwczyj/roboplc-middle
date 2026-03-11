//! ModbusWorker - RoboPLC worker implementation

use crate::config::Device;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::event_matches;

use super::handler::DeviceControlHandler;

/// ModbusWorker wrapper with WorkerOpts for RoboPLC scheduling
#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    handler: DeviceControlHandler,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        Self {
            handler: DeviceControlHandler::new(device),
        }
    }
}

// ==================== Worker trait implementation ====================

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let worker_name = format!("modbus_worker_{}", self.handler.device().id);

        let hub_client = context
            .hub()
            .register(&worker_name, event_matches!(Message::DeviceControl { .. }))?;

        tracing::info!(device_id = %self.handler.device().id, "ModbusWorker started");

        for msg in hub_client {
            if !context.is_online() {
                break;
            }

            if let Message::DeviceControl {
                device_id,
                operation,
                params,
                correlation_id,
                respond_to,
            } = msg
            {
                if device_id != self.handler.device().id {
                    tracing::warn!(
                        received_device_id = %device_id,
                        expected_device_id = %self.handler.device().id,
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

                self.handler.handle_device_control(
                    device_id,
                    operation,
                    params,
                    correlation_id,
                    respond_to,
                    context,
                );
            }
        }

        tracing::info!(device_id = %self.handler.device().id, "ModbusWorker stopped");
        Ok(())
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceType, SignalGroup};
    use crate::ConnectionState;
    use crate::workers::modbus::{parse_register_address, RegisterType};

    fn test_device() -> crate::config::Device {
        crate::config::Device {
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

        assert_eq!(worker.handler.connection_state(), ConnectionState::Disconnected);
    }

    #[test]
    fn parse_register_address_handles_prefixes() {
        assert_eq!(parse_register_address("h100").map(|(_, addr)| addr), Some(100));
        assert_eq!(parse_register_address("H200").map(|(_, addr)| addr), Some(200));
        assert_eq!(parse_register_address("i50").map(|(_, addr)| addr), Some(50));
        assert_eq!(parse_register_address("c10").map(|(_, addr)| addr), Some(10));
        assert_eq!(parse_register_address("d5").map(|(_, addr)| addr), Some(5));
        assert_eq!(parse_register_address("100").map(|(_, addr)| addr), Some(100));
    }
}
