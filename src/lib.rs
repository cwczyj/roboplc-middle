//! # roboplc-middleware
//!
//! 通信中间件，用于将 JSON-RPC 2.0 请求转换为 Modbus TCP 操作。
//! 该中间件作为 PLC 和机械臂设备的通信桥梁，提供统一的 API 接口。
//!
//! ## 架构概述
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
//! │   JSON-RPC      │────▶│  Device Manager  │────▶│   Modbus    │
//! │   Server        │     │  (Hub Router)    │     │   Workers   │
//! └─────────────────┘     └──────────────────┘     └─────────────┘
//!        │                        │                       │
//!        │                        ▼                       │
//!        │              ┌──────────────────┐             │
//!        └─────────────▶│  HTTP API       │◀────────────┘
//!                       │  /api/devices   │
//!                       │  /api/health    │
//!                       └──────────────────┘
//! ```
//!
//! ## 核心组件
//!
//! - **Workers**: 基于 RoboPLC 框架的实时工作线程
//!   - `RpcWorker`: 处理 JSON-RPC 请求 (端口 8080)
//!   - `ModbusWorker`: 管理 Modbus TCP 连接
//!   - `HttpWorker`: 提供 HTTP 管理接口 (端口 8081)
//!   - `ConfigLoader`: 热重载配置文件
//!   - `LatencyMonitor`: 监控设备延迟和异常检测
//!
//! - **Hub**: RoboPLC 消息路由系统，在 workers 之间传递消息
//!
//! - **Variables**: 跨 workers 的共享状态
//!   - 设备状态跟踪
//!   - 延迟样本数据
//!   - 事务日志
//!   - 事件流
//!
//! ## 使用方式
//!
//! 1. 创建 `config.toml` 配置文件
//! 2. 运行 `cargo run --release` 启动中间件
//! 3. 通过 JSON-RPC 或 HTTP API 与设备交互
//!
//! 详见 [USAGE.md](USAGE.md) 获取完整使用说明。

pub mod api;
pub mod config;
pub mod messages;
pub mod profiles;
pub mod workers;

// 导出主要的公共接口类型
pub use messages::{Message, Operation, SystemStatusResponse};

use parking_lot_rt::RwLock; // 实时安全的读写锁
use rtsc::buf::DataBuffer; // 循环缓冲区，用于高效的数据流
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Instant;

/// 设备状态跟踪
///
/// 记录每个设备的连接状态和通信指标，用于监控设备健康状态。
/// 该状态由 `ModbusWorker` 更新，并通过共享状态供其他 workers 访问。
///
/// # 字段说明
///
/// - `connected`: 设备当前是否已连接
/// - `last_communication`: 上一次成功通信的时间戳
/// - `error_count`: 累计的错误次数
/// - `reconnect_count`: 累计的重连次数
#[derive(Debug)]
pub struct DeviceStatus {
    /// 设备连接状态
    pub connected: bool,
    /// 上一次成功通信的时间
    pub last_communication: Instant,
    /// 累计错误计数
    pub error_count: u32,
    /// 累计重连计数
    pub reconnect_count: u32,
}

/// Modbus 事务日志条目
///
/// 记录每次 Modbus 操作的详细信息，用于调试和审计。
/// 该日志存储在循环缓冲区中，保留最近的操作记录。
///
/// # 字段说明
///
/// - `device_id`: 设备标识符
/// - `timestamp_ms`: 操作发生的 Unix 时间戳（毫秒）
/// - `operation`: 操作类型（如 "ReadHolding", "WriteSingle"）
/// - `address`: Modbus 地址
/// - `success`: 操作是否成功
/// - `latency_us`: 操作耗时（微秒）
#[derive(Clone, Debug)]
pub struct ModbusLogEntry {
    /// 设备标识符
    pub device_id: String,
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
    /// 操作类型描述
    pub operation: String,
    /// Modbus 地址
    pub address: String,
    /// 操作是否成功
    pub success: bool,
    /// 操作延迟（微秒）
    pub latency_us: u64,
}

/// 设备事件
///
/// 记录设备状态变化事件，如连接、断开、重连等。
/// 这些事件用于监控和日志记录，帮助诊断连接问题。
///
/// # 字段说明
///
/// - `device_id`: 设备标识符
/// - `event_type`: 事件类型（连接、断开、重连等）
/// - `timestamp_ms`: 事件发生的 Unix 时间戳（毫秒）
/// - `details`: 事件详情描述
#[derive(Clone, Debug)]
pub struct DeviceEvent {
    /// 设备标识符
    pub device_id: String,
    /// 事件类型
    pub event_type: DeviceEventType,
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
    /// 事件详情
    pub details: String,
}

/// 设备事件类型
///
/// 定义设备可能发生的各种状态变化事件。
#[derive(Clone, Debug)]
pub enum DeviceEventType {
    /// 设备成功连接
    Connected,
    /// 设备连接断开
    Disconnected,
    /// 设备正在重新连接
    Reconnecting,
    /// 发生错误
    Error,
    /// 心跳超时未收到
    HeartbeatMissed,
}

/// 延迟样本
///
/// 记录单次设备通信的延迟数据，用于延迟监控和异常检测。
/// 使用 3-sigma 算法检测延迟异常。
///
/// # 字段说明
///
/// - `device_id`: 设备 ID（u32，与 String 的 device_id 不同）
/// - `latency_us`: 通信延迟（微秒）
/// - `timestamp_ms`: 采样时间戳（毫秒）
#[derive(Clone, Debug, Copy)]
pub struct LatencySample {
    /// 设备 ID
    pub device_id: u32,
    /// 延迟时间（微秒）
    pub latency_us: u64,
    /// 采样时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 所有 workers 的共享状态
///
/// 这是 RoboPLC Hub 中的全局变量结构，所有 workers 都可以访问这些状态。
/// 使用原子类型和锁来保证线程安全。
///
/// # 数据结构说明
///
/// - `devices_count`: 使用 `AtomicU32` 进行原子计数
/// - `system_healthy`: 使用 `AtomicBool` 标记系统健康状态
/// - `device_states`: 使用 `Arc<RwLock<HashMap>>` 实现并发安全的随机访问
/// - 其他字段: 使用 `DataBuffer` 实现高效的循环缓冲区
///
/// # 使用场景
///
/// - `ModbusWorker`: 更新设备状态、记录延迟和事件
/// - `HttpWorker`: 读取设备状态以提供 API 响应
/// - `LatencyMonitor`: 从延迟样本中读取数据进行分析
pub struct Variables {
    /// 活跃设备数量（原子计数器）
    pub devices_count: AtomicU32,
    /// 系统健康标志（原子布尔值）
    pub system_healthy: AtomicBool,
    /// 每个设备的状态（随机访问，并发读取）
    /// 使用 RwLock 允许多个读取者同时访问
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// 延迟监控样本（批量统计数据）
    /// 用于 3-sigma 异常检测
    pub latency_samples: DataBuffer<LatencySample>,
    /// 最近的 Modbus 事务（环形缓冲区，用于日志）
    /// 保留最近的 100 条事务记录
    pub modbus_transactions: DataBuffer<ModbusLogEntry>,
    /// 设备事件流（事件流）
    /// 记录设备状态变化事件
    pub device_events: DataBuffer<DeviceEvent>,
}

impl std::fmt::Debug for Variables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Variables")
            .field("devices_count", &self.devices_count)
            .field("system_healthy", &self.system_healthy)
            .field("device_states", &self.device_states)
            .field("latency_samples_len", &self.latency_samples.len())
            .field("modbus_transactions_len", &self.modbus_transactions.len())
            .field("device_events_len", &self.device_events.len())
            .finish()
    }
}

impl Default for Variables {
    fn default() -> Self {
        Self {
            // 初始化设备计数为 0
            devices_count: AtomicU32::new(0),
            // 系统初始状态为健康
            system_healthy: AtomicBool::new(true),
            // 空的设备状态映射
            device_states: Arc::new(RwLock::new(HashMap::new())),
            // 延迟样本缓冲区，保留 100 个样本
            latency_samples: DataBuffer::bounded(100),
            // Modbus 事务日志，保留 100 条记录
            modbus_transactions: DataBuffer::bounded(100),
            // 设备事件流，保留 100 个事件
            device_events: DataBuffer::bounded(100),
        }
    }
}
