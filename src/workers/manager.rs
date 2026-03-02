use crate::config::Config;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time;
/// Response data sent back to requesters via channel
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceResponseData {
    pub success: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}

#[derive(WorkerOpts)]
#[worker_opts(name = "device_manager")]
pub struct DeviceManager {
    config: Config,
    worker_map: HashMap<String, String>,
    pending_requests: HashMap<u64, Sender<DeviceResponseData>>,
}

impl DeviceManager {
    pub fn new(config: Config) -> Self {
        let mut worker_map = HashMap::new();
        for device in &config.devices {
            worker_map.insert(device.id.clone(), format!("modbus_worker_{}", device.id));
        }
        Self {
            config,
            worker_map,
            pending_requests: HashMap::new(),
        }
    }

    pub fn get_worker_name(&self, device_id: &str) -> Option<&String> {
        self.worker_map.get(device_id)
    }

    fn register_devices(&self, context: &Context<Message, Variables>) {
        let mut states = context.variables().device_states.write();
        for device in &self.config.devices {
            states.insert(
                device.id.clone(),
                crate::DeviceStatus {
                    connected: false,
                    last_communication: time::Instant::now(),
                    error_count: 0,
                    reconnect_count: 0,
                },
            );
            tracing::info!("Registered device: {}", device.id);
        }
    }
}

impl Worker<Message, Variables> for DeviceManager {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let client = context.hub().register(
            "device_manager",
            event_matches!(Message::DeviceControl { .. } | Message::DeviceResponse { .. }),
        )?;

        tracing::info!(
            "Device Manager started, routing {} devices",
            self.config.devices.len()
        );

        self.register_devices(context);

        for msg in client {
            match msg {
                Message::DeviceControl {
                    device_id,
                    operation,
                    params,
                    correlation_id,
                } => {
                    tracing::debug!(
                        device_id = %device_id,
                        operation = ?operation,
                        "Received DeviceControl request"
                    );

                    // Forward to the appropriate ModbusWorker
                    match self.get_worker_name(&device_id) {
                        Some(worker_name) => {
                            tracing::trace!(
                                device_id = %device_id,
                                worker_name = %worker_name,
                                "Forwarding DeviceControl to worker"
                            );
                            context.hub().send(Message::DeviceControl {
                                device_id,
                                operation,
                                params,
                                correlation_id,
                            });
                        }
                        None => {
                            tracing::error!(
                                device_id = %device_id,
                                "No worker found for device"
                            );
                        }
                    }
                }
                Message::DeviceResponse {
                    device_id,
                    success,
                    data,
                    error,
                    correlation_id,
                } => {
                    tracing::debug!(
                        device_id = %device_id,
                        success = success,
                        "Received DeviceResponse"
                    );

                    // Route response to the original requester via correlation_id
                    if let Some(sender) = self.pending_requests.remove(&correlation_id) {
                        let response_data = DeviceResponseData {
                            success,
                            data,
                            error,
                        };
                        if let Err(e) = sender.send(response_data) {
                            tracing::warn!(
                                correlation_id = correlation_id,
                                error = %e,
                                "Failed to send response to requester"
                            );
                        }
                    } else {
                        tracing::warn!(
                            correlation_id = correlation_id,
                            "No pending request found for correlation_id"
                        );
                    }
                }
                Message::DeviceHeartbeat { .. } => {}
                Message::ConfigUpdate { .. } => {}
                Message::SystemStatus { .. } => {}
            }
        }

        tracing::info!("Device Manager stopped");
        Ok(())
    }
}
