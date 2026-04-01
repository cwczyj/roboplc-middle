//! ModbusWorker - RoboPLC worker implementation

use crate::config::{Config, Device};
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::event_matches;

use super::handler::DeviceControlHandler;

/// ModbusWorker wrapper with WorkerOpts for RoboPLC scheduling
#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 0, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    handler: DeviceControlHandler,
    device_id: String,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        Self {
            handler: DeviceControlHandler::new(device.clone()),
            device_id: device.id,
        }
    }

    /// Find device config from new config and update
    fn find_device_config(&self, new_config: &Config) -> Option<Device> {
        new_config
            .devices
            .iter()
            .find(|d| d.id == self.device_id)
            .cloned()
    }
}

// ==================== Worker trait implementation ====================

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let worker_name = format!("modbus_worker_{}", self.handler.device().id);

        // Subscribe to both DeviceControl and ConfigUpdate messages
        let mut hub_client = context.hub().register(
            &worker_name,
            event_matches!(Message::DeviceControl { .. } | Message::ConfigUpdate { .. }),
        )?;

        tracing::info!(device_id = %self.handler.device().id, "ModbusWorker started");

        for msg in hub_client.by_ref() {
            if !context.is_online() {
                break;
            }

            match msg {
                Message::DeviceControl {
                    device_id,
                    operation,
                    params,
                    correlation_id,
                    respond_to,
                } => {
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
                Message::ConfigUpdate { config } => {
                    tracing::info!(
                        device_id = %self.handler.device().id,
                        "Received ConfigUpdate message"
                    );

                    // Parse the new config and find our device configuration
                    match serde_json::from_str::<Config>(&config) {
                        Ok(new_config) => {
                            tracing::info!(
                                device_id = %self.handler.device().id,
                                new_devices_count = new_config.devices.len(),
                                "Parsed new config"
                            );
                            if let Some(new_device_config) = self.find_device_config(&new_config) {
                                let old_signal_groups: Vec<String> = self
                                    .handler
                                    .device()
                                    .signal_groups
                                    .iter()
                                    .map(|g| g.name.clone())
                                    .collect();
                                let new_signal_groups: Vec<String> = new_device_config
                                    .signal_groups
                                    .iter()
                                    .map(|g| g.name.clone())
                                    .collect();
                                tracing::info!(
                                    device_id = %self.handler.device().id,
                                    old_signal_groups = ?old_signal_groups,
                                    new_signal_groups = ?new_signal_groups,
                                    "Updating device signal_groups"
                                );
                                self.handler.update_device_config(new_device_config);
                                tracing::info!(
                                    device_id = %self.handler.device().id,
                                    "Device configuration updated successfully"
                                );
                            } else {
                                tracing::warn!(
                                    device_id = %self.handler.device().id,
                                    "Device not found in new config"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                device_id = %self.handler.device().id,
                                error = %e,
                                "Failed to parse ConfigUpdate JSON"
                            );
                        }
                    }
                }
                _ => {
                    tracing::debug!(device_id = %self.handler.device().id, "Received unexpected message type in ModbusWorker");
                }
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
    use crate::workers::modbus::{parse_register_address, RegisterType};
    use crate::ConnectionState;
    use std::sync::Arc;

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
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            pool_health_check_interval_sec: 30,
            signal_groups: Vec::<SignalGroup>::new(),
        }
    }

    #[test]
    fn worker_new_initializes_without_client() {
        let worker = ModbusWorker::new(test_device());

        assert_eq!(
            worker.handler.connection_state(),
            ConnectionState::Disconnected
        );
    }

    #[test]
    fn parse_register_address_handles_prefixes() {
        assert_eq!(
            parse_register_address("h100").map(|(_, addr)| addr),
            Some(100)
        );
        assert_eq!(
            parse_register_address("H200").map(|(_, addr)| addr),
            Some(200)
        );
        assert_eq!(
            parse_register_address("i50").map(|(_, addr)| addr),
            Some(50)
        );
        assert_eq!(
            parse_register_address("c10").map(|(_, addr)| addr),
            Some(10)
        );
        assert_eq!(parse_register_address("d5").map(|(_, addr)| addr), Some(5));
        assert_eq!(
            parse_register_address("100").map(|(_, addr)| addr),
            Some(100)
        );
    }

    #[test]
    fn find_device_config_finds_matching_device() {
        let device = crate::config::Device {
            id: "test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: Default::default(),
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            pool_health_check_interval_sec: 30,
            signal_groups: Vec::<SignalGroup>::new(),
        };
        let worker = ModbusWorker::new(device);

        let new_config = Config {
            devices: vec![crate::config::Device {
                id: "test-device".to_string(),
                device_type: DeviceType::Plc,
                address: "127.0.0.1".to_string(),
                port: 502,
                unit_id: 1,
                addressing_mode: Default::default(),
                byte_order: Default::default(),
                tcp_nodelay: true,
                max_concurrent_ops: 3,
                max_pool_size: 5,
                heartbeat_interval_sec: 30,
                pool_health_check_interval_sec: 30,
                signal_groups: vec![SignalGroup {
                    name: "new_group".to_string(),
                    description: "New test group".to_string(),
                    register_address: "h0".to_string(),
                    register_count: 10,
                    fields: Arc::new(vec![]),
                }],
            }],
            server: Default::default(),
            logging: Default::default(),
            timeouts: Default::default(),
        };

        let found = worker.find_device_config(&new_config);
        assert!(found.is_some());
        assert_eq!(found.unwrap().signal_groups.len(), 1);
    }

    #[test]
    fn find_device_config_returns_none_for_unknown_device() {
        let device = crate::config::Device {
            id: "test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: Default::default(),
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            pool_health_check_interval_sec: 30,
            signal_groups: Vec::<SignalGroup>::new(),
        };
        let worker = ModbusWorker::new(device);

        let new_config = Config {
            devices: vec![crate::config::Device {
                id: "other-device".to_string(),
                device_type: DeviceType::Plc,
                address: "127.0.0.1".to_string(),
                port: 502,
                unit_id: 1,
                addressing_mode: Default::default(),
                byte_order: Default::default(),
                tcp_nodelay: true,
                max_concurrent_ops: 3,
                max_pool_size: 5,
                heartbeat_interval_sec: 30,
                pool_health_check_interval_sec: 30,
                signal_groups: Vec::<SignalGroup>::new(),
            }],
            server: Default::default(),
            logging: Default::default(),
            timeouts: Default::default(),
        };

        let found = worker.find_device_config(&new_config);
        assert!(found.is_none());
    }
}
