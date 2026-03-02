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

// ========== 模块声明 ==========
// pub mod 声明公共子模块，使它们可以被外部 crate 使用
pub mod api;        // HTTP API 模块
pub mod config;     // 配置解析模块
pub mod messages;   // 消息类型定义模块
pub mod profiles;   // 设备配置文件模块
pub mod workers;    // Worker 实现模块

// ========== 公共接口导出 ==========
// pub use 将内部模块的公共类型重新导出到 crate 根级别
// 这样外部代码可以直接使用 `roboplc_middleware::Message` 而不是 `roboplc_middleware::messages::Message`
pub use messages::{Message, Operation, SystemStatusResponse};

// ========== 外部依赖导入 ==========
// use 语句用于导入外部 crate 和标准库的类型到当前作用域
use parking_lot_rt::RwLock;           // 实时安全的读写锁，比标准库的 RwLock 性能更好
use rtsc::buf::DataBuffer;            // 循环缓冲区，用于高效的数据流存储
use std::collections::HashMap;        // 标准库的哈希映射，键值对存储
use std::sync::atomic::{AtomicBool, AtomicU32};  // 原子类型，用于无锁并发
use std::sync::Arc;                   // 原子引用计数，用于跨线程共享所有权
use std::time::Instant;               // 时间戳类型，用于测量时间间隔

// ========== 公共类型定义 ==========

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
///
/// # Rust 语法说明
///
/// - `#[derive(Debug)]`: 自动实现 Debug trait，允许使用 `{:?}` 格式化打印
/// - `pub struct`: 公开的结构体，字段也需要 `pub` 才能被外部访问
#[derive(Debug)]  // 自动生成 Debug trait 实现，用于调试输出
pub struct DeviceStatus {
    /// 设备连接状态
    /// bool 是布尔类型，只有 true 和 false 两个值
    pub connected: bool,
    
    /// 上一次成功通信的时间
    /// Instant 是标准库的时间戳类型，用于测量时间间隔
    /// 它是单调递增的，不受系统时间修改影响
    pub last_communication: Instant,
    
    /// 累计错误计数
    /// u32 是无符号 32 位整数，范围 0 到 4,294,967,295
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
///
/// # Rust 语法说明
///
/// - `#[derive(Clone, Debug)]`: Clone trait 允许克隆结构体，Debug 允许调试输出
/// - String 是 Rust 的堆分配字符串类型，Vec<u8> 的封装
#[derive(Clone, Debug)]  // Clone 允许 .clone() 复制，Debug 允许 {:?} 打印
pub struct ModbusLogEntry {
    /// 设备标识符
    /// String 是所有权字符串，存储在堆上
    pub device_id: String,
    
    /// 时间戳（毫秒）
    /// u64 是无符号 64 位整数，足够存储毫秒级时间戳
    pub timestamp_ms: u64,
    
    /// 操作类型描述
    /// 例如 "ReadHolding", "WriteSingle", "ReadCoils"
    pub operation: String,
    
    /// Modbus 地址
    /// 例如 "h100" 表示保持寄存器 100
    pub address: String,
    
    /// 操作是否成功
    pub success: bool,
    
    /// 操作延迟（微秒）
    /// 1 微秒 = 0.000001 秒
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
    /// 使用自定义枚举 DeviceEventType
    pub event_type: DeviceEventType,
    
    /// 时间戳（毫秒）
    pub timestamp_ms: u64,
    
    /// 事件详情
    /// 例如 "Connection established" 或 "Timeout after 5000ms"
    pub details: String,
}

/// 设备事件类型
///
/// 定义设备可能发生的各种状态变化事件。
/// 
/// # Rust 语法说明
///
/// - enum 是枚举类型，定义一组可能的变体
/// - 每个变体可以有关联数据（元组或结构体形式）
/// - Clone 和 Debug 是派生宏，自动实现 trait
#[derive(Clone, Debug)]
pub enum DeviceEventType {
    /// 设备成功连接
    /// 单元变体（无关联数据）
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
///
/// # Rust 语法说明
///
/// - `#[derive(Clone, Debug, Copy)]`: Copy trait 允许按位复制（栈上类型）
/// - Copy 类型在赋值时会自动复制，而不是移动所有权
#[derive(Clone, Debug, Copy)]  // Copy 允许按位复制，避免所有权转移
pub struct LatencySample {
    /// 设备 ID
    /// 使用 u32 而不是 String，因为 ID 通常是数字
    /// 更节省内存，比较更快
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
///
/// # Rust 语法说明
///
/// - AtomicU32/AtomicBool: 原子类型，支持无锁并发访问
/// - Arc: Atomic Reference Counting，线程安全的引用计数
/// - RwLock: 读写锁，允许多读者或单写者
/// - DataBuffer: 固定大小的循环缓冲区
pub struct Variables {
    /// 活跃设备数量（原子计数器）
    /// AtomicU32 提供原子操作，无需锁即可安全并发访问
    /// 支持 load/store/fetch_add 等原子操作
    pub devices_count: AtomicU32,
    
    /// 系统健康标志（原子布尔值）
    /// AtomicBool 用于跨线程共享状态标志
    pub system_healthy: AtomicBool,
    
    /// 每个设备的状态（随机访问，并发读取）
    /// Arc<RwLock<...>> 模式：
    /// - Arc 允许多个所有者共享同一数据
    /// - RwLock 允许多个读者同时读取，或一个写者写入
    /// - HashMap<K, V> 是键值对映射
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    
    /// 延迟监控样本（批量统计数据）
    /// DataBuffer 是固定大小的循环缓冲区
    /// 当缓冲区满时，新数据会覆盖最旧的数据
    /// 用于 3-sigma 异常检测
    pub latency_samples: DataBuffer<LatencySample>,
    
    /// 最近的 Modbus 事务（环形缓冲区，用于日志）
    /// 保留最近的 100 条事务记录
    /// 用于调试和审计
    pub modbus_transactions: DataBuffer<ModbusLogEntry>,
    
    /// 设备事件流（事件流）
    /// 记录设备状态变化事件
    /// 用于监控和告警
    pub device_events: DataBuffer<DeviceEvent>,
}

// ========== trait 实现 ==========

/// 为 Variables 实现 Debug trait
/// 
/// 手动实现而不是 derive，因为 DataBuffer 没有实现 Debug
/// 我们只打印缓冲区的长度，而不是内容
impl std::fmt::Debug for Variables {
    /// fmt 方法用于格式化输出
    /// 
    /// 参数说明：
    /// - &self: 不可变引用，只读访问结构体
    /// - f: &mut Formatter 格式化器，用于写入输出
    /// 
    /// 返回值：std::fmt::Result，表示格式化是否成功
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // debug_struct 创建一个结构体的调试输出构建器
        // 链式调用 .field() 添加字段
        f.debug_struct("Variables")
            .field("devices_count", &self.devices_count)          // 原子类型实现了 Debug
            .field("system_healthy", &self.system_healthy)        // 原子类型实现了 Debug
            .field("device_states", &self.device_states)          // RwLock 实现了 Debug
            .field("latency_samples_len", &self.latency_samples.len())  // 只打印长度
            .field("modbus_transactions_len", &self.modbus_transactions.len())
            .field("device_events_len", &self.device_events.len())
            .finish()  // 完成构建，返回 Result
    }
}

/// 为 Variables 实现 Default trait
/// 
/// Default trait 提供类型的默认值
/// 使用 default() 方法创建实例
impl Default for Variables {
    /// default 方法返回 Variables 的默认实例
    /// 
    /// 返回值：Self 是 Variables 的别名，返回一个新的 Variables 实例
    fn default() -> Self {
        // Self { ... } 创建并返回新的结构体实例
        Self {
            // 初始化设备计数为 0
            // AtomicU32::new(0) 创建值为 0 的原子整数
            devices_count: AtomicU32::new(0),
            
            // 系统初始状态为健康
            // AtomicBool::new(true) 创建值为 true 的原子布尔
            system_healthy: AtomicBool::new(true),
            
            // 空的设备状态映射
            // Arc::new(...) 将数据包装在 Arc 中
            // RwLock::new(...) 创建新的读写锁
            // HashMap::new() 创建空的哈希映射
            device_states: Arc::new(RwLock::new(HashMap::new())),
            
            // 延迟样本缓冲区，保留 100 个样本
            // DataBuffer::bounded(100) 创建容量为 100 的有界缓冲区
            latency_samples: DataBuffer::bounded(100),
            
            // Modbus 事务日志，保留 100 条记录
            modbus_transactions: DataBuffer::bounded(100),
            
            // 设备事件流，保留 100 个事件
            device_events: DataBuffer::bounded(100),
        }
    }
}