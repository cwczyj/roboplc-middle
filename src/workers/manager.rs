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
//! - DeviceManager 现在只用于设备注册

use crate::config::Config;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use std::collections::HashMap;
use std::time;

#[derive(WorkerOpts)]
#[worker_opts(name = "device_manager", cpu = 4, scheduling = "fifo", priority = 50)]
pub struct DeviceManager {
    config: Config,
    #[allow(dead_code)]
    worker_map: HashMap<String, String>,
}

impl DeviceManager {
    pub fn new(config: Config) -> Self {
        let mut worker_map = HashMap::new();
        for device in &config.devices {
            worker_map.insert(device.id.clone(), format!("modbus_worker_{}", device.id));
        }
        Self { config, worker_map }
    }

    fn register_devices(&self, context: &Context<Message, Variables>) {
        let states = &context.variables().device_states;
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
        tracing::info!(
            "Device Manager started, managing {} devices",
            self.config.devices.len()
        );

        self.register_devices(context);

        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        tracing::info!("Device Manager stopped");
        Ok(())
    }
}
