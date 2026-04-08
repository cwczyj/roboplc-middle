//! # 配置模块
//!
//! 负责加载、解析和验证中间件的配置文件。
//!
//! ## 配置文件格式
//!
//! 配置文件使用 TOML 格式，命名为 `config.toml`。
//! 参考 `config.sample.toml` 获取完整示例。
//!
//! ## 主要功能
//!
//! - 从文件加载配置
//! - TOML 格式解析和类型转换
//! - 配置验证（端口范围、设备 ID 唯一性、地址格式）
//! - 提供默认值
//!
//! ## 使用方式
//!
//! ```rust,no_run
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     use roboplc_middleware::config::Config;
//!
//!     let config = Config::from_file("config.toml")?;
//!     println!("RPC port: {}", config.server.rpc_port);
//!     Ok(())
//! }

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// 数据流配置
///
/// 定义单个数据流的配置参数。
///
/// # 字段说明
///
/// - `device_id`: 设备唯一标识符
/// - `signal_group`: 信号组名称
/// - `poll_interval_ms`: 轮询间隔（毫秒，范围 5-5000）
/// - `enabled`: 是否启用此流
/// - `priority`: 优先级（数值越小优先级越高）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamConfig {
    /// 设备唯一标识符
    pub device_id: String,
    /// 信号组名称
    pub signal_group: String,
    /// 轮询间隔（毫秒，默认 100，范围 5-5000）
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u32,
    /// 是否启用此流（默认 true）
    #[serde(default = "default_stream_enabled")]
    pub enabled: bool,
    /// 优先级（默认 0，数值越小优先级越高）
    #[serde(default)]
    pub priority: u8,
}

/// 默认轮询间隔（100ms）
fn default_poll_interval_ms() -> u32 {
    100
}

/// 默认流启用状态
fn default_stream_enabled() -> bool {
    true
}

impl StreamConfig {
    /// 验证流配置
    ///
    /// 检查 poll_interval_ms 是否在 5-5000ms 范围内
    pub fn validate(&self) -> Result<(), String> {
        if self.poll_interval_ms < 5 || self.poll_interval_ms > 5000 {
            return Err(format!(
                "StreamConfig validation error: poll_interval_ms must be between 5 and 5000, got {}",
                self.poll_interval_ms
            ));
        }
        Ok(())
    }
}

/// 流全局设置
///
/// 管理所有数据流的全局配置。
///
/// # 字段说明
///
/// - `max_concurrent_streams`: 最大并发流数量
/// - `cache_ttl_ms`: 数据缓存 TTL（毫秒）
/// - `enable_websocket`: 是否启用 WebSocket 服务器
/// - `websocket_port`: WebSocket 服务器端口
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamSettings {
    /// 最大并发流数量（默认 100）
    #[serde(default = "default_max_concurrent_streams")]
    pub max_concurrent_streams: u32,
    /// 数据缓存 TTL（毫秒，默认 5000）
    #[serde(default = "default_cache_ttl_ms")]
    pub cache_ttl_ms: u32,
    /// 是否启用 WebSocket 服务器（默认 true）
    #[serde(default = "default_enable_websocket")]
    pub enable_websocket: bool,
    /// WebSocket 服务器端口（默认 8082）
    #[serde(default = "default_websocket_port")]
    pub websocket_port: u16,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            max_concurrent_streams: default_max_concurrent_streams(),
            cache_ttl_ms: default_cache_ttl_ms(),
            enable_websocket: default_enable_websocket(),
            websocket_port: default_websocket_port(),
        }
    }
}

/// 默认最大并发流数量
fn default_max_concurrent_streams() -> u32 {
    100
}

/// 默认缓存 TTL（毫秒）
fn default_cache_ttl_ms() -> u32 {
    5000
}

/// 默认 WebSocket 启用状态
fn default_enable_websocket() -> bool {
    true
}

/// 默认 WebSocket 端口
fn default_websocket_port() -> u16 {
    8082
}

/// 超时配置
///
/// 统一管理所有超时参数，确保系统行为一致。
///
/// # 字段说明
///
/// - `connect_timeout_ms`: Modbus TCP 连接超时（毫秒）
/// - `operation_timeout_ms`: Modbus 操作超时（毫秒）
/// - `max_operation_timeout_ms`: 最大操作超时（毫秒）
/// - `hub_send_timeout_ms`: Hub 消息发送超时（毫秒）
/// - `heartbeat_timeout_ms`: 心跳检测超时（毫秒）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeouts {
    /// Modbus TCP 连接超时（毫秒，默认 200）
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u16,
    /// Modbus 操作超时（毫秒，默认 1000）
    #[serde(default = "default_operation_timeout_ms")]
    pub operation_timeout_ms: u16,
    /// 最大操作超时（毫秒，默认 30000）
    #[serde(default = "default_max_operation_timeout_ms")]
    pub max_operation_timeout_ms: u16,
    /// Hub 消息发送超时（毫秒，默认 500）
    #[serde(default = "default_hub_send_timeout_ms")]
    pub hub_send_timeout_ms: u16,
    /// 心跳检测超时（毫秒，默认 1000）
    #[serde(default = "default_heartbeat_timeout_ms")]
    pub heartbeat_timeout_ms: u16,
    /// 连接池健康检查间隔（秒，默认 30）
    #[serde(default = "default_pool_health_check_interval")]
    pub pool_health_check_interval_sec: u16,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            operation_timeout_ms: default_operation_timeout_ms(),
            max_operation_timeout_ms: default_max_operation_timeout_ms(),
            hub_send_timeout_ms: default_hub_send_timeout_ms(),
            heartbeat_timeout_ms: default_heartbeat_timeout_ms(),
            pool_health_check_interval_sec: default_pool_health_check_interval(),
        }
    }
}

/// 默认连接超时（200ms）
fn default_connect_timeout_ms() -> u16 {
    200
}

/// 默认操作超时（1000ms）
fn default_operation_timeout_ms() -> u16 {
    1000
}

/// 默认最大操作超时（30000ms）
fn default_max_operation_timeout_ms() -> u16 {
    30000
}

/// 默认 Hub 发送超时（2000ms）
/// 增加到 2000ms 以防止高负载 SSE 广播时超时
fn default_hub_send_timeout_ms() -> u16 {
    2000
}

/// 默认心跳超时（1000ms）
fn default_heartbeat_timeout_ms() -> u16 {
    1000
}

/// 默认连接池健康检查间隔（30秒）
fn default_pool_health_check_interval() -> u16 {
    30
}

impl Timeouts {
    /// 获取连接超时 Duration
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms as u64)
    }

    /// 获取操作超时 Duration
    pub fn operation_timeout(&self) -> Duration {
        Duration::from_millis(self.operation_timeout_ms as u64)
    }

    /// 获取最大操作超时 Duration
    pub fn max_operation_timeout(&self) -> Duration {
        Duration::from_millis(self.max_operation_timeout_ms as u64)
    }

    /// 获取 Hub 发送超时 Duration
    pub fn hub_send_timeout(&self) -> Duration {
        Duration::from_millis(self.hub_send_timeout_ms as u64)
    }

    /// 获取心跳超时 Duration
    pub fn heartbeat_timeout(&self) -> Duration {
        Duration::from_millis(self.heartbeat_timeout_ms as u64)
    }
}

/// 配置根结构
///
/// 包含中间件的所有配置项。
///
/// # 字段说明
///
/// - `server`: 服务器配置（RPC 和 HTTP 端口）
/// - `logging`: 日志配置（级别、文件路径、轮转策略）
/// - `timeouts`: 超时配置（连接、操作、Hub 发送等超时）
/// - `devices`: 设备列表，可以为空
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// 服务器配置
    #[serde(default)]
    pub server: Server,
    /// 日志配置
    #[serde(default)]
    pub logging: Logging,
    /// 超时配置
    #[serde(default)]
    pub timeouts: Timeouts,
    /// 设备配置列表
    #[serde(default)]
    pub devices: Vec<Device>,
    /// 数据流配置列表
    #[serde(default)]
    pub streams: Vec<StreamConfig>,
    /// 流全局设置
    #[serde(default)]
    pub stream_settings: StreamSettings,
}

/// 服务器配置
///
/// 定义 JSON-RPC 和 HTTP 服务器的监听端口。
///
/// # 字段说明
///
/// - `rpc_port`: JSON-RPC 服务器端口（默认 8080）
/// - `http_port`: HTTP 管理接口端口（默认 8081）
/// - `rpc_worker_threads`: Tokio 工作线程数（默认 4）
/// - `rpc_max_blocking_threads`: Tokio 最大阻塞线程数（默认 128）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Server {
    /// JSON-RPC 服务监听端口
    pub rpc_port: u16,
    /// HTTP API 监听端口
    pub http_port: u16,
    /// Tokio 工作线程数
    #[serde(default = "default_rpc_worker_threads")]
    pub rpc_worker_threads: usize,
    /// Tokio 最大阻塞线程数
    #[serde(default = "default_rpc_max_blocking_threads")]
    pub rpc_max_blocking_threads: usize,
}

/// 日志配置
///
/// 配置日志输出级别和文件存储方式。
///
/// # 字段说明
///
/// - `level`: 日志级别（"trace", "debug", "info", "warn", "error"）
/// - `file`: 日志文件路径
/// - `daily_rotation`: 是否按天轮转日志文件
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Logging {
    /// 日志级别
    pub level: String,
    /// 日志文件路径
    pub file: String,
    /// 是否按天轮转日志
    pub daily_rotation: bool,
}

/// 设备配置
///
/// 定义单个 Modbus 设备的连接参数和信号组配置。
///
/// # 字段说明
///
/// - `id`: 设备唯一标识符（必须全局唯一）
/// - `device_type`: 设备类型（PLC 或机械臂）
/// - `address`: Modbus TCP 地址（IP 地址或主机名）
/// - `port`: Modbus TCP 端口（通常为 502）
/// - `unit_id`: Modbus 单元 ID（从站 ID）
/// - `addressing_mode`: 地址模式（从 0 开始或从 1 开始）
/// - `byte_order`: 字节序（大端、小端等）
/// - `tcp_nodelay`: 是否启用 TCP_NODELAY（禁用 Nagle 算法）
/// - `max_concurrent_ops`: 最大并发操作数
/// - `max_pool_size`: 连接池最大大小（上限）
/// - `heartbeat_interval_sec`: 心跳间隔（秒）
/// - `signal_groups`: 信号组列表（用于批量读写寄存器）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// 设备唯一标识符（在所有设备中必须唯一）
    pub id: String,
    /// 设备类型
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    /// Modbus TCP 地址
    pub address: String,
    /// Modbus TCP 端口
    pub port: u16,
    /// Modbus 单元 ID（从站地址）
    pub unit_id: u8,
    /// 地址模式
    #[serde(default)]
    pub addressing_mode: AddressingMode,
    /// 字节序
    #[serde(default)]
    pub byte_order: ByteOrder,
    /// 是否禁用 Nagle 算法（减少延迟）
    #[serde(default = "default_tcp_nodelay")]
    pub tcp_nodelay: bool,
    /// 最大并发操作数
    #[serde(default = "default_max_concurrent_ops")]
    pub max_concurrent_ops: u8,
    /// 连接池最大大小（上限）
    #[serde(default = "default_max_pool_size")]
    pub max_pool_size: u8,
    /// 心跳间隔（秒）
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_sec: u32,
    /// 连接池健康检查间隔（秒）
    #[serde(default = "default_pool_health_check_interval")]
    pub pool_health_check_interval_sec: u16,
    /// 信号组列表
    #[serde(default)]
    pub signal_groups: Vec<SignalGroup>,
}

/// 默认 TCP_NODELAY 值
fn default_tcp_nodelay() -> bool {
    true
}

/// 默认最大并发操作数
fn default_max_concurrent_ops() -> u8 {
    10
}

/// 默认连接池最大大小
fn default_max_pool_size() -> u8 {
    5
}

/// 默认心跳间隔（秒）
fn default_heartbeat_interval() -> u32 {
    30
}

/// 默认 Tokio 工作线程数
fn default_rpc_worker_threads() -> usize {
    4
}

/// 默认 Tokio 最大阻塞线程数
fn default_rpc_max_blocking_threads() -> usize {
    128
}

/// 设备类型
///
/// 定义支持的设备类型。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// 可编程逻辑控制器
    #[default]
    Plc,
    /// 机械臂
    RobotArm,
}

/// 地址模式
///
/// 定义寄存器地址是从 0 开始还是从 1 开始。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AddressingMode {
    /// 从 0 开始寻址（Modbus 标准）
    #[default]
    ZeroBased,
    /// 从 1 开始寻址
    OneBased,
}

/// 字节序
///
/// 定义多字节数据的字节排列顺序。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    /// 大端序（高位在前）
    #[default]
    BigEndian,
    /// 小端序（低位在前）
    LittleEndian,
    /// 小端序交换字节
    LittleEndianByteSwap,
    /// 中大端序
    MidBig,
}

/// 寄存器映射
///
/// 将 Modbus 地址映射到有意义的信号名称。
///
/// # 地址格式
///
/// 使用前缀表示寄存器类型：
/// - `c`: Coil (0x)
/// - `d`: Discrete Input (1x)
/// - `i`: Input Register (3x)
/// - `h`: Holding Register (4x)
///
/// 示例：`h100` = Holding Register 地址 100
///
/// # 字段说明
///
/// - `signal_name`: 信号名称（用于 API 响应）
/// - `address`: Modbus 地址（带前缀）
/// - `data_type`: 数据类型
/// - `access`: 访问模式
/// - `description`: 信号描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMapping {
    /// 信号名称（用于 API 接口）
    pub signal_name: String,
    /// Modbus 地址（带类型前缀，如 "h100"）
    pub address: String,
    /// 数据类型
    #[serde(default)]
    pub data_type: DataType,
    /// 访问模式
    #[serde(default)]
    pub access: AccessMode,
    /// 信号描述
    #[serde(default)]
    pub description: String,
}

/// 信号组
///
/// 定义一批在连续寄存器范围内的相关信号。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalGroup {
    /// 信号组名称
    pub name: String,
    /// 信号组描述
    #[serde(default)]
    pub description: String,
    /// Modbus 地址（带类型前缀，如 "h100"）
    pub register_address: String,
    /// 寄存器数量
    pub register_count: u16,
    /// 字段映射列表（使用 Arc 避免热路径克隆）
    pub fields: Arc<Vec<FieldMapping>>,
}

/// 字段映射
///
/// 将单个字段映射到寄存器组内的偏移量。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMapping {
    /// 字段名称
    pub name: String,
    /// 数据类型
    pub data_type: DataType,
    /// 寄存器偏移量（以寄存器为单位）
    pub offset: u16,
}

impl SignalGroup {
    /// 验证信号组的配置是否有效
    pub fn validate(&self) -> Result<(), String> {
        use std::collections::HashSet;

        let mut seen_fields = HashSet::new();

        for field in self.fields.iter() {
            if !seen_fields.insert(&field.name) {
                return Err(format!(
                    "Duplicate field name '{}' in signal group '{}'",
                    field.name, self.name
                ));
            }

            let required = field.data_type.required_registers();
            let end_offset = field.offset.saturating_add(required);

            if end_offset > self.register_count {
                return Err(format!(
                    "Field '{}' in signal group '{}' requires {} registers, but group only has {}",
                    field.name, self.name, required, self.register_count
                ));
            }
        }

        Ok(())
    }
}

impl DataType {
    /// 返回该数据类型需要的寄存器数量
    ///
    /// F64 需要 4 个寄存器（64 位 / 16 位 = 4）
    pub fn required_registers(&self) -> u16 {
        match self {
            DataType::U16 | DataType::I16 | DataType::Bool => 1,
            DataType::U32 | DataType::I32 | DataType::F32 => 2,
            DataType::F64 => 4,
        }
    }
}

/// 数据类型
///
/// 定义寄存器中存储的数据类型。
/// F64 需要 4 个连续的 16 位寄存器（64 位 = 4 * 16 位）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    /// 无符号 16 位整数
    #[default]
    U16,
    /// 无符号 32 位整数
    U32,
    /// 有符号 16 位整数
    I16,
    /// 有符号 32 位整数
    I32,
    /// 32 位浮点数 (IEEE 754)
    F32,
    /// 64 位浮点数 (IEEE 754)
    F64,
    /// 布尔值
    Bool,
}

/// 访问模式
///
/// 定义寄存器的读写权限。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    /// 可读写
    #[default]
    Rw,
    /// 只读
    Read,
    /// 只写
    Write,
}

/// 配置错误
///
/// 定义配置加载和验证过程中可能出现的错误。
#[derive(Debug, Error)]
pub enum ConfigError {
    /// 文件 IO 错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML 解析错误
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    /// 重复的设备 ID
    #[error("Duplicate device ID: {0}")]
    DuplicateDeviceId(String),
    /// 无效的端口号
    #[error("Invalid port: {0}")]
    InvalidPort(u16),
    /// 地址格式错误
    #[error("Invalid address format for device '{0}' register '{1}': {2}")]
    InvalidAddressFormat(String, String, String),
    /// 地址超出范围
    #[error("Address out of range for device '{0}' register '{1}': {2}")]
    AddressOutOfRange(String, String, u32),
    /// 信号组验证错误
    #[error("Signal group validation error for device '{0}': {1}")]
    SignalGroupValidation(String, String),
    /// 流配置验证错误
    #[error("Stream config validation error: {0}")]
    StreamConfigValidation(String),
}

impl Config {
    /// 从文件加载配置
    ///
    /// 读取 TOML 格式的配置文件，解析并验证。
    ///
    /// # 参数
    ///
    /// - `path`: 配置文件路径
    ///
    /// # 返回值
    ///
    /// - `Ok(Config)`: 配置加载成功
    /// - `Err(ConfigError)`: 配置文件不存在、格式错误或验证失败
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     use roboplc_middleware::config::Config;
    ///
    ///     let config = Config::from_file("config.toml")?;
    ///     println!("Loaded {} devices", config.devices.len());
    ///     Ok(())
    /// }
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// 验证配置
    ///
    /// 检查配置的有效性：
    /// 1. 设备 ID 必须唯一
    /// 2. 寄存器地址格式必须正确
    /// 3. 地址必须在有效范围内 (0-65535)
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 配置验证通过
    /// - `Err(ConfigError)`: 发现配置错误
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut seen_ids = HashSet::new();

        for device in &self.devices {
            if !seen_ids.insert(&device.id) {
                return Err(ConfigError::DuplicateDeviceId(device.id.clone()));
            }

            for group in &device.signal_groups {
                let addr = group.register_address.trim();
                if addr.len() < 2 {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        group.name.clone(),
                        addr.to_string(),
                    ));
                }

                let prefix = &addr[0..1].to_lowercase();
                let num_str = &addr[1..];

                if !matches!(prefix.as_str(), "h" | "d" | "c" | "i") {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        group.name.clone(),
                        addr.to_string(),
                    ));
                }

                if let Ok(num) = num_str.parse::<u32>() {
                    if num > 65535 {
                        return Err(ConfigError::AddressOutOfRange(
                            device.id.clone(),
                            group.name.clone(),
                            num,
                        ));
                    }
                } else {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        group.name.clone(),
                        addr.to_string(),
                    ));
                }

                if let Err(err) = group.validate() {
                    return Err(ConfigError::SignalGroupValidation(device.id.clone(), err));
                }
            }
        }

        // Validate stream configurations
        for stream in &self.streams {
            if let Err(err) = stream.validate() {
                return Err(ConfigError::StreamConfigValidation(err));
            }
        }

        Ok(())
    }

    /// 检查流配置是否发生变化
    ///
    /// 用于配置热重载时检测流配置的变化。
    ///
    /// # 参数
    ///
    /// - `other`: 另一个流配置列表
    ///
    /// # 返回值
    ///
    /// `true` 如果配置不同，`false` 如果相同
    pub fn streams_differ(&self, other: &[StreamConfig]) -> bool {
        self.streams.len() != other.len()
            || self.streams.iter().zip(other.iter()).any(|(a, b)| a != b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_parsing() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[stream_settings]
max_concurrent_streams = 100
cache_ttl_ms = 1000
enable_websocket = true
websocket_port = 8082

[[streams]]
device_id = "plc-1"
signal_group = "sensor_data"
poll_interval_ms = 100
enabled = true
priority = 1
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.streams.len(), 1);
        let stream = &config.streams[0];
        assert_eq!(stream.device_id, "plc-1");
        assert_eq!(stream.signal_group, "sensor_data");
        assert_eq!(stream.poll_interval_ms, 100);
        assert_eq!(stream.enabled, true);
        assert_eq!(stream.priority, 1);
        assert_eq!(config.stream_settings.max_concurrent_streams, 100);
        assert_eq!(config.stream_settings.cache_ttl_ms, 1000);
        assert_eq!(config.stream_settings.enable_websocket, true);
        assert_eq!(config.stream_settings.websocket_port, 8082);
    }

    #[test]
    fn test_stream_config_defaults() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[[streams]]
device_id = "plc-1"
signal_group = "sensor_data"
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.streams.len(), 1);
        let stream = &config.streams[0];
        assert_eq!(stream.poll_interval_ms, 100);
        assert_eq!(stream.enabled, true);
        assert_eq!(stream.priority, 0);
    }

    #[test]
    fn test_stream_settings_defaults() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.stream_settings.max_concurrent_streams, 100);
        assert_eq!(config.stream_settings.cache_ttl_ms, 5000);
        assert_eq!(config.stream_settings.enable_websocket, true);
        assert_eq!(config.stream_settings.websocket_port, 8082);
    }

    #[test]
    fn test_stream_config_validation() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[[streams]]
device_id = "plc-1"
signal_group = "sensor_data"
poll_interval_ms = 1
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("poll_interval_ms"));
    }

    #[test]
    fn test_stream_config_validation_max() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[[streams]]
device_id = "plc-1"
signal_group = "sensor_data"
poll_interval_ms = 6000
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("poll_interval_ms"));
    }

    #[test]
    fn test_stream_config_diff_detection() {
        let config1 = Config {
            streams: vec![StreamConfig {
                device_id: "plc-1".to_string(),
                signal_group: "sensor_data".to_string(),
                poll_interval_ms: 100,
                enabled: true,
                priority: 1,
            }],
            stream_settings: StreamSettings::default(),
            ..Default::default()
        };
        let config2 = Config {
            streams: vec![StreamConfig {
                device_id: "plc-1".to_string(),
                signal_group: "sensor_data".to_string(),
                poll_interval_ms: 200,
                enabled: true,
                priority: 1,
            }],
            stream_settings: StreamSettings::default(),
            ..Default::default()
        };
        assert!(config1.streams_differ(&config2.streams));

        let config3 = Config {
            streams: vec![StreamConfig {
                device_id: "plc-1".to_string(),
                signal_group: "sensor_data".to_string(),
                poll_interval_ms: 100,
                enabled: true,
                priority: 1,
            }],
            stream_settings: StreamSettings::default(),
            ..Default::default()
        };
        assert!(!config1.streams_differ(&config3.streams));
    }

    #[test]
    fn test_default_config() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.rpc_port, 8080);
        assert_eq!(config.devices.len(), 0);
    }

    #[test]
    fn test_config_without_timeouts() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        // Verify default timeout values
        assert_eq!(config.timeouts.connect_timeout_ms, 200);
        assert_eq!(config.timeouts.operation_timeout_ms, 1000);
        assert_eq!(config.timeouts.max_operation_timeout_ms, 30000);
        assert_eq!(config.timeouts.hub_send_timeout_ms, 2000);
        assert_eq!(config.timeouts.heartbeat_timeout_ms, 1000);
    }

    #[test]
    fn test_config_with_timeouts() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[timeouts]
connect_timeout_ms = 500
operation_timeout_ms = 2000
max_operation_timeout_ms = 60000
hub_send_timeout_ms = 1000
heartbeat_timeout_ms = 2000
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        // Verify custom timeout values
        assert_eq!(config.timeouts.connect_timeout_ms, 500);
        assert_eq!(config.timeouts.operation_timeout_ms, 2000);
        assert_eq!(config.timeouts.max_operation_timeout_ms, 60000);
        assert_eq!(config.timeouts.hub_send_timeout_ms, 1000);
        assert_eq!(config.timeouts.heartbeat_timeout_ms, 2000);
    }

    #[test]
    fn test_timeouts_partial_fields() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[timeouts]
connect_timeout_ms = 300
heartbeat_timeout_ms = 1500
"#;
        let config: Config = toml::from_str(config_str).unwrap();

        // Verify specified values
        assert_eq!(config.timeouts.connect_timeout_ms, 300);
        assert_eq!(config.timeouts.heartbeat_timeout_ms, 1500);

        // Verify default values for unspecified fields
        assert_eq!(config.timeouts.operation_timeout_ms, 1000);
        assert_eq!(config.timeouts.max_operation_timeout_ms, 30000);
        assert_eq!(config.timeouts.hub_send_timeout_ms, 2000);
    }

    #[test]
    fn test_timeouts_duration_helpers() {
        let timeouts = Timeouts {
            connect_timeout_ms: 200,
            operation_timeout_ms: 1000,
            max_operation_timeout_ms: 30000,
            hub_send_timeout_ms: 2000,
            heartbeat_timeout_ms: 1000,
            pool_health_check_interval_sec: 30,
        };

        // Verify Duration conversions
        assert_eq!(timeouts.connect_timeout(), Duration::from_millis(200));
        assert_eq!(timeouts.operation_timeout(), Duration::from_millis(1000));
        assert_eq!(
            timeouts.max_operation_timeout(),
            Duration::from_millis(30000)
        );
        assert_eq!(timeouts.hub_send_timeout(), Duration::from_millis(2000));
        assert_eq!(timeouts.heartbeat_timeout(), Duration::from_millis(1000));
    }

    #[test]
    fn test_timeouts_invalid_values() {
        // Test that invalid timeout values are properly handled
        // The spec says: "测试无效超时值的错误处理"

        // Test with string value (should fail to parse)
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[timeouts]
connect_timeout_ms = "invalid"
"#;
        let result: Result<Config, _> = toml::from_str(config_str);
        assert!(result.is_err(), "Should reject string value for timeout");

        // Test with negative value (should fail to parse)
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[timeouts]
connect_timeout_ms = -1
"#;
        let result: Result<Config, _> = toml::from_str(config_str);
        assert!(result.is_err(), "Should reject negative value for timeout");

        // Test with float value (should fail to parse)
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[timeouts]
operation_timeout_ms = 1000.5
"#;
        let result: Result<Config, _> = toml::from_str(config_str);
        assert!(result.is_err(), "Should reject float value for timeout");
    }

    #[test]
    fn test_server_runtime_config_defaults() {
        let config_str = r#"
            [server]
            rpc_port = 8080
            http_port = 8081
        "#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.rpc_worker_threads, 4);
        assert_eq!(config.server.rpc_max_blocking_threads, 128);
    }

    #[test]
    fn test_server_runtime_config_custom() {
        let config_str = r#"
            [server]
            rpc_port = 8080
            http_port = 8081
            rpc_worker_threads = 8
            rpc_max_blocking_threads = 256
        "#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.rpc_worker_threads, 8);
        assert_eq!(config.server.rpc_max_blocking_threads, 256);
    }
}
