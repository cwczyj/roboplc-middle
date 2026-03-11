//! # Device Manager Worker
//!
//! 设备消息路由器，负责在 RpcWorker、HttpWorker 和 ModbusWorker 之间路由消息。
//!
//! ## 功能
//!
//! - 注册所有设备到共享状态
//! - 路由 DeviceResponse 消息回请求的 worker
//! - 维护待处理请求映射（correlation_id -> sender）
//!
//! ## 架构说明（修复无限循环）
//!
//! 之前的实现中，DeviceManager 订阅了 DeviceControl 消息，收到后会转发到 Hub。
//! 但由于自己也订阅了 DeviceControl，会再次收到自己转发的消息，形成无限循环：
//! 1. DeviceManager 收到 DeviceControl
//! 2. DeviceManager 转发到 Hub
//! 3. DeviceManager 再次收到自己转发的消息（回到步骤 1）
//!
//! 修复方案：
//! - DeviceManager 不再订阅 DeviceControl 消息
//! - ModbusWorker 直接使用 respond_to 通道响应给 RpcWorker
//! - DeviceManager 现在只用于设备注册和可选的响应路由

use crate::config::Config;
use crate::{DeviceResponseData, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time;

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
            // 重要更新：不再订阅 DeviceControl！
            // 原因：之前 DeviceManager 收到 DeviceControl 后会转发到 Hub，
            // 但由于自己也订阅了 DeviceControl，会再次收到自己转发的消息，
            // 形成无限循环。
            //
            // 现在 ModbusWorker 直接使用 respond_to 通道响应给 RpcWorker，
            // 不再需要 DeviceManager 路由 DeviceResponse。
            //
            // DeviceManager 现在只订阅：
            // - TimeoutCleanup 消息（清理超时请求）
            // 其他消息都不需要处理，只用于保持 worker 在线
            event_matches!(Message::TimeoutCleanup { .. }),
        )?;

        tracing::info!(
            "Device Manager started, managing {} devices",
            self.config.devices.len()
        );

        self.register_devices(context);

        for msg in client {
            match msg {
                Message::TimeoutCleanup { correlation_id } => {
                    if let Some(_) = self.pending_requests.remove(&correlation_id) {
                        tracing::debug!(
                            correlation_id,
                            "Cleaned up timed-out request from pending_requests"
                        );
                    }
                }
                Message::DeviceControl { .. }
                | Message::DeviceResponse { .. }
                | Message::DeviceHeartbeat { .. }
                | Message::ConfigUpdate { .. }
                | Message::SystemStatus { .. } => {}
            }
        }

        tracing::info!("Device Manager stopped");
        Ok(())
    }
}
