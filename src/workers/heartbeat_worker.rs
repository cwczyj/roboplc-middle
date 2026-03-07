//! HeartbeatWorker - 独立心跳检测 Worker
//!
//! 职责：
//! - 定期检查所有设备是否在线
//! - 通过发送 GetStatus 请求复用 ModbusWorker 的连接
//! - 广播 DeviceHeartbeat 消息（包含真实延迟）
//! - 记录延迟到 latency_samples
//! - 更新设备状态到共享变量

use crate::config::Config;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 心跳检测 Worker
///
/// 通过发送 GetStatus 请求来检测设备是否在线，
/// 复用 ModbusWorker 已建立的连接。
#[derive(WorkerOpts)]
#[worker_opts(name = "heartbeat", blocking = true)]
pub struct HeartbeatWorker {
    config: Config,
    /// 下一个心跳检查的设备索引（轮询）
    current_device_index: usize,
    /// 全局心跳间隔（秒）- 取所有设备的最小值
    heartbeat_interval_sec: u32,
    /// 心跳超时（秒）- 等待响应的最大时间
    heartbeat_timeout_sec: u32,
}

impl HeartbeatWorker {
    /// 创建新的 HeartbeatWorker
    pub fn new(config: Config) -> Self {
        // 计算全局心跳间隔（取所有设备的最小值）
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
            heartbeat_timeout_sec: 5, // 默认 5 秒超时
        }
    }

    /// 发送心跳请求并等待响应
    ///
    /// 返回：(是否在线, 延迟微秒)
    fn ping_device(&self, device_id: &str, context: &Context<Message, Variables>) -> (bool, u64) {
        let start = SystemTime::now();
        let correlation_id = Self::generate_correlation_id();

        // 创建响应通道
        let (tx, rx) = mpsc::channel();

        // 发送 GetStatus 请求
        context.hub().send(Message::DeviceControl {
            device_id: device_id.to_string(),
            operation: crate::messages::Operation::GetStatus,
            params: serde_json::json!({}),
            correlation_id,
            respond_to: Some(tx),
        });

        // 等待响应（带超时）
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

    /// 生成唯一的 correlation_id
    fn generate_correlation_id() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// 更新设备状态到共享变量
    fn update_device_status(
        &self,
        device_id: &str,
        connected: bool,
        context: &Context<Message, Variables>,
    ) {
        let mut states = context.variables().device_states.write();
        if let Some(status) = states.get_mut(device_id) {
            let was_connected = status.connected;
            status.connected = connected;
            status.last_communication = std::time::Instant::now();

            // 如果状态发生变化，记录事件
            if was_connected != connected {
                let event_type = if connected {
                    DeviceEventType::Connected
                } else {
                    DeviceEventType::Disconnected
                };

                let event = DeviceEvent {
                    device_id: device_id.to_string(),
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

                // 释放锁后再推送事件
                drop(states);
                context.variables().device_events.force_push(event);
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

        // 广播 DeviceHeartbeat 消息
        let _ = context.hub().send(Message::DeviceHeartbeat {
            device_id: device_id.to_string(),
            timestamp_ms,
            latency_us,
        });

        // 记录 LatencySample
        if connected && latency_us > 0 {
            let sample = LatencySample {
                device_id: device_id.to_string(),
                latency_us,
                timestamp_ms,
            };
            context.variables().latency_samples.force_push(sample);
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
        let device_count = self.config.devices.len();

        if device_count == 0 {
            tracing::warn!("No devices configured, HeartbeatWorker will idle");
            while context.is_online() {
                std::thread::sleep(Duration::from_secs(10));
            }
            return Ok(());
        }

        // 计算每个设备的检查间隔
        // 平均分配检查时间，避免同时发送大量请求
        let per_device_interval =
            Duration::from_secs(self.heartbeat_interval_sec as u64 / device_count as u64);
        let min_interval = Duration::from_millis(100); // 最小间隔 100ms
        let check_interval = per_device_interval.max(min_interval);

        tracing::info!(
            devices_count = device_count,
            heartbeat_interval_sec = self.heartbeat_interval_sec,
            check_interval_ms = check_interval.as_millis(),
            "HeartbeatWorker started"
        );

        while context.is_online() {
            // 获取当前要检查的设备
            let device = &self.config.devices[self.current_device_index];

            tracing::debug!(
                device_id = %device.id,
                device_index = self.current_device_index,
                "Checking device heartbeat"
            );

            // 发送心跳请求
            let (connected, latency_us) = self.ping_device(&device.id, context);

            // 更新设备状态
            self.update_device_status(&device.id, connected, context);

            // 广播心跳消息
            self.broadcast_heartbeat(&device.id, connected, latency_us, context);

            // 移动到下一个设备
            self.current_device_index = (self.current_device_index + 1) % device_count;

            // 等待下一个检查周期
            std::thread::sleep(check_interval);
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
    fn correlation_id_increments() {
        let id1 = HeartbeatWorker::generate_correlation_id();
        let id2 = HeartbeatWorker::generate_correlation_id();

        assert!(id2 > id1);
    }
}
