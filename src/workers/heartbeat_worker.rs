//! HeartbeatWorker - Independent heartbeat detection worker
//!
//! Responsibilities:
//! - Periodically check if all devices are online
//! - Reuse ModbusWorker connections by sending GetStatus requests
//! - Broadcast DeviceHeartbeat messages (including actual latency)
//! - Record latency to latency_samples
//! - Update device status to shared variables

use crate::config::Config;
use crate::hub_protection::{send_to_hub_with_protection, DEFAULT_HUB_SEND_TIMEOUT};
use crate::next_correlation_id;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::time::interval;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Heartbeat detection worker
///
/// Detects whether a device is online by sending GetStatus requests,
/// reusing the already established connections from ModbusWorker.
#[derive(WorkerOpts)]
#[worker_opts(name = "heartbeat", cpu = 2, scheduling = "fifo", priority = 70)]
pub struct HeartbeatWorker {
    config: Config,
    /// Next device index for heartbeat check (round-robin)
    current_device_index: usize,
    /// Global heartbeat interval (seconds) - minimum of all devices
    heartbeat_interval_sec: u32,
    /// Heartbeat timeout (seconds) - maximum time to wait for response
    heartbeat_timeout_sec: u32,
}

impl HeartbeatWorker {
    /// Create new HeartbeatWorker
    pub fn new(config: Config) -> Self {
        let heartbeat_interval_sec = config
            .devices
            .iter()
            .map(|d| d.heartbeat_interval_sec)
            .min()
            .unwrap_or(30);

        Self {
            config,
            current_device_index: 0,
            heartbeat_interval_sec,
            heartbeat_timeout_sec: 1,
        }
    }

    /// Send heartbeat request and wait for response
    ///
    /// Returns: (whether online, latency in microseconds)
    fn ping_device(&self, device_id: &str, context: &Context<Message, Variables>) -> (bool, u64) {
        let start = SystemTime::now();
        let correlation_id = next_correlation_id();

        let (tx, rx) = mpsc::sync_channel(crate::MAX_PENDING_HEARTBEATS);

        if let Err(e) = send_to_hub_with_protection(
            context.hub(),
            Message::DeviceControl {
                device_id: String::from(device_id),
                operation: crate::messages::Operation::GetStatus,
                params: serde_json::json!({}),
                correlation_id,
                respond_to: Some(tx),
            },
            DEFAULT_HUB_SEND_TIMEOUT,
        ) {
            tracing::warn!(error = %e, device_id = %device_id, "Failed to send heartbeat request");
            return (false, 0);
        }

        let timeout = Duration::from_secs(self.heartbeat_timeout_sec as u64);
        match rx.recv_timeout(timeout) {
            Ok((success, _data, _error)) => {
                let latency_us = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                (success, latency_us)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    device_id = %device_id,
                    timeout_sec = self.heartbeat_timeout_sec,
                    "Heartbeat request timed out"
                );
                (false, 0)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!(device_id = %device_id, "Response channel disconnected");
                (false, 0)
            }
        }
    }

    /// Update device status to shared variables
    fn update_device_status(
        &self,
        device_id: &str,
        connected: bool,
        context: &Context<Message, Variables>,
    ) {
        let states = &context.variables().device_states;
        if let Some(mut status) = states.get_mut(device_id) {
            let was_connected = status.connected;
            status.connected = connected;
            status.last_communication = std::time::Instant::now();

            if was_connected != connected {
                let event_type = if connected {
                    DeviceEventType::Connected
                } else {
                    DeviceEventType::Disconnected
                };

                let event = DeviceEvent {
                    device_id: String::from(device_id),
                    event_type,
                    timestamp_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    details: format!(
                        "Device {} via heartbeat check",
                        if connected {
                            "connected"
                        } else {
                            "disconnected"
                        }
                    ),
                };

                drop(status);
                if !context.variables().device_events.force_push(event) {
                    tracing::warn!(
                        device_id = %device_id,
                        "Device event buffer full, oldest event dropped"
                    );
                }
            }
        }
    }

    /// 广播心跳消息并记录延迟
    fn broadcast_heartbeat(
        &self,
        device_id: &str,
        connected: bool,
        latency_us: u64,
        context: &Context<Message, Variables>,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Err(e) = send_to_hub_with_protection(
            context.hub(),
            Message::DeviceHeartbeat {
                device_id: String::from(device_id),
                timestamp_ms,
                latency_us,
            },
            DEFAULT_HUB_SEND_TIMEOUT,
        ) {
            tracing::warn!(error = %e, device_id = %device_id, "Failed to send heartbeat message");
        }

        if connected && latency_us > 0 {
            let sample = LatencySample {
                device_id: String::from(device_id),
                latency_us,
                timestamp_ms,
            };
            // if !context.variables().latency_samples.force_push(sample) {
            //     tracing::warn!(
            //         device_id = %device_id,
            //         "Latency samples buffer full, oldest sample dropped"
            //     );
            // }
        }

        tracing::trace!(
            device_id = %device_id,
            connected = connected,
            latency_us = latency_us,
            "Heartbeat completed"
        );
    }
}

impl Worker<Message, Variables> for HeartbeatWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        tracing::info!("HeartbeatWorker started");

        let base_interval = Duration::from_millis(100);

        for _ in interval(base_interval).take_while(|_| context.is_online()) {
            let device_count = self.config.devices.len();

            if device_count == 0 {
                tracing::debug!("No devices configured, waiting");
                continue;
            }

            self.current_device_index = self.current_device_index % device_count;
            let device = &self.config.devices[self.current_device_index];

            tracing::debug!(
                device_id = %device.id,
                device_index = self.current_device_index,
                "Checking device heartbeat"
            );

            let (connected, latency_us) = self.ping_device(&device.id, context);
            self.update_device_status(&device.id, connected, context);
            self.broadcast_heartbeat(&device.id, connected, latency_us, context);

            self.current_device_index = self.current_device_index.saturating_add(1) % device_count;

            let per_device_interval =
                Duration::from_secs(self.heartbeat_interval_sec as u64 / device_count as u64);
            let target_interval = per_device_interval.max(base_interval);

            if target_interval > base_interval {
                let skip_ticks = (target_interval.as_millis() / base_interval.as_millis()) as usize;
                for _ in 1..skip_ticks {
                    if !context.is_online() {
                        break;
                    }
                    std::thread::yield_now();
                }
            }
        }

        tracing::info!("HeartbeatWorker stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Device, DeviceType};

    fn test_config() -> Config {
        Config {
            server: crate::config::Server {
                rpc_port: 8080,
                http_port: 8081,
            },
            logging: crate::config::Logging {
                level: "info".to_string(),
                file: String::new(),
                daily_rotation: false,
            },
            devices: vec![Device {
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
                signal_groups: vec![],
            }],
        }
    }

    #[test]
    fn heartbeat_worker_calculates_interval() {
        let config = test_config();
        let worker = HeartbeatWorker::new(config);

        assert_eq!(worker.heartbeat_interval_sec, 30);
    }

    #[test]
    fn heartbeat_worker_handles_empty_device_list() {
        let mut config = test_config();
        config.devices.clear();

        let worker = HeartbeatWorker::new(config);
        assert_eq!(worker.heartbeat_interval_sec, 30);
    }

    #[test]
    fn heartbeat_worker_bounds_check_index() {
        let config = test_config();
        let mut worker = HeartbeatWorker::new(config);

        worker.current_device_index = 1000;
        let device_count = worker.config.devices.len();
        let bounded_index = worker.current_device_index % device_count;
        assert_eq!(bounded_index, 0);
    }

    #[test]
    fn correlation_id_increments() {
        let id1 = next_correlation_id();
        let id2 = next_correlation_id();

        assert!(id2 > id1);
    }
}
