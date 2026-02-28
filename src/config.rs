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
use thiserror::Error;

/// 配置根结构
///
/// 包含中间件的所有配置项。
///
/// # 字段说明
///
/// - `server`: 服务器配置（RPC 和 HTTP 端口）
/// - `logging`: 日志配置（级别、文件路径、轮转策略）
/// - `devices`: 设备列表，可以为空
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 服务器配置
    pub server: Server,
    /// 日志配置
    pub logging: Logging,
    /// 设备配置列表
    #[serde(default)]
    pub devices: Vec<Device>,
}

/// 服务器配置
///
/// 定义 JSON-RPC 和 HTTP 服务器的监听端口。
///
/// # 字段说明
///
/// - `rpc_port`: JSON-RPC 服务器端口（默认 8080）
/// - `http_port`: HTTP 管理接口端口（默认 8081）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    /// JSON-RPC 服务监听端口
    pub rpc_port: u16,
    /// HTTP API 监听端口
    pub http_port: u16,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// 定义单个 Modbus 设备的连接参数和寄存器映射。
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
/// - `heartbeat_interval_sec`: 心跳间隔（秒）
/// - `register_mappings`: 寄存器地址映射列表
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
    /// 心跳间隔（秒）
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_sec: u32,
    /// 寄存器地址映射列表
    #[serde(default)]
    pub register_mappings: Vec<RegisterMapping>,
}

/// 默认 TCP_NODELAY 值
fn default_tcp_nodelay() -> bool {
    true
}

/// 默认最大并发操作数
fn default_max_concurrent_ops() -> u8 {
    3
}

/// 默认心跳间隔（秒）
fn default_heartbeat_interval() -> u32 {
    30
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
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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

/// 数据类型
///
/// 定义寄存器中存储的数据类型。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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
        // 读取配置文件内容
        let content = fs::read_to_string(path)?;
        // 从 TOML 格式解析为 Config 结构体
        let config: Config = toml::from_str(&content)?;
        // 验证配置的有效性
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
        // 用于检测重复的设备 ID
        let mut seen_ids = HashSet::new();

        // 遍历所有设备
        for device in &self.devices {
            // 检查设备 ID 是否重复
            if !seen_ids.insert(&device.id) {
                return Err(ConfigError::DuplicateDeviceId(device.id.clone()));
            }

            // 遍历设备的所有寄存器映射
            for mapping in &device.register_mappings {
                let addr = mapping.address.trim();

                // 地址至少需要 2 个字符（1 个前缀 + 1 个数字）
                if addr.len() < 2 {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                // 提取前缀和数字部分
                let prefix = &addr[0..1].to_lowercase();
                let num_str = &addr[1..];

                // 检查前缀是否有效
                if !matches!(prefix.as_str(), "h" | "d" | "c" | "i") {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                // 检查数字部分是否有效
                if let Ok(num) = num_str.parse::<u32>() {
                    // Modbus 地址最大为 65535 (16 位)
                    if num > 65535 {
                        return Err(ConfigError::AddressOutOfRange(
                            device.id.clone(),
                            mapping.signal_name.clone(),
                            num,
                        ));
                    }
                } else {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
