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

// use 关键字用于导入其他模块或 crate 中的类型和函数
// serde 是一个序列化/反序列化库，Deserialize 用于从 TOML 等格式解析数据，Serialize 用于将数据转换为其他格式
use serde::{Deserialize, Serialize};
// HashSet 是哈希集合，用于存储唯一的值，这里用于检测重复的设备 ID
use std::collections::HashSet;
// fs 是文件系统模块，提供读写文件的功能
use std::fs;
// Path 是表示文件路径的 trait，用于抽象不同的路径类型
use std::path::Path;
// thiserror 库提供的 Error 宏，用于简化错误类型的定义
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
// #[derive(...)] 是属性宏，自动为结构体实现指定的 trait
// Debug: 允许使用 {:?} 格式化打印调试信息
// Clone: 允许使用 .clone() 方法创建副本
// Serialize: 允许序列化为 JSON/TOML 等格式
// Deserialize: 允许从 JSON/TOML 等格式反序列化
#[derive(Debug, Clone, Serialize, Deserialize)]
// pub 关键字表示公共可见性，其他模块可以访问
// struct 定义一个结构体，是 Rust 中自定义类型的主要方式
pub struct Config {
    /// 服务器配置
    // 这里的类型 Server 是另一个结构体，嵌套定义配置层次
    pub server: Server,
    /// 日志配置
    pub logging: Logging,
    /// 设备配置列表
    // Vec<Device> 是一个动态数组，可以存储任意数量的 Device
    // Vec 是 Rust 的标准动态数组类型，类似 C++ 的 vector 或 Python 的 list
    #[serde(default)]
    // #[serde(default)] 表示如果 TOML 中没有这个字段，使用 Default trait 的默认值
    // 对于 Vec，默认值是空数组
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
    // u16 是无符号 16 位整数，范围 0-65535，适合表示端口号
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
    // String 是 Rust 的可增长字符串类型，存储在堆上
    pub level: String,
    /// 日志文件路径
    pub file: String,
    /// 是否按天轮转日志
    // bool 是布尔类型，只有 true 或 false 两个值
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
    // #[serde(rename = "type")] 将 JSON/TOML 中的 "type" 字段映射到 Rust 的 device_type
    // 因为 type 是 Rust 的关键字，不能直接用作字段名
    pub device_type: DeviceType,
    /// Modbus TCP 地址
    pub address: String,
    /// Modbus TCP 端口
    pub port: u16,
    /// Modbus 单元 ID（从站地址）
    // u8 是无符号 8 位整数，范围 0-255
    pub unit_id: u8,
    /// 地址模式
    #[serde(default)]
    pub addressing_mode: AddressingMode,
    /// 字节序
    #[serde(default)]
    pub byte_order: ByteOrder,
    /// 是否禁用 Nagle 算法（减少延迟）
    #[serde(default = "default_tcp_nodelay")]
    // #[serde(default = "...")] 使用指定的函数生成默认值
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
// fn 定义一个函数
// 函数名使用 snake_case（蛇形命名法）
// -> bool 表示返回类型是 bool
fn default_tcp_nodelay() -> bool {
    // true 是布尔值，表示启用 TCP_NODELAY
    true
}

/// 默认最大并发操作数
fn default_max_concurrent_ops() -> u8 {
    // 3 是 u8 类型的整数字面量
    3
}

/// 默认心跳间隔（秒）
fn default_heartbeat_interval() -> u32 {
    30
}

/// 设备类型
///
/// 定义支持的设备类型。
// enum 定义枚举类型，表示一组固定的变体
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
// Default: 提供默认值
// PartialEq: 允许使用 == 和 != 比较
#[serde(rename_all = "snake_case")]
// #[serde(rename_all = "snake_case")] 将所有变体名序列化为蛇形命名法
// 例如 Plc 序列化为 "plc"
pub enum DeviceType {
    /// 可编程逻辑控制器
    #[default]
    // #[default] 标记这个变体作为枚举的默认值
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
// #[derive(Debug, Error)] 使用 thiserror 宏派生 Error trait
// 这使得枚举可以作为错误类型使用
#[derive(Debug, Error)]
pub enum ConfigError {
    /// 文件 IO 错误
    #[error("IO error: {0}")]
    // #[error("...")] 定义错误的显示格式
    // {0} 是占位符，表示第一个字段的值
    Io(#[from] std::io::Error),
    // #[from] 自动实现 From trait，允许使用 ? 操作符自动转换错误类型
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
    // 三个占位符 {0} {1} {2} 对应三个 String 参数
    InvalidAddressFormat(String, String, String),
    /// 地址超出范围
    #[error("Address out of range for device '{0}' register '{1}': {2}")]
    AddressOutOfRange(String, String, u32),
}

// impl 为类型实现方法
// impl Config 表示为 Config 结构体实现方法
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
    // P: AsRef<Path> 是泛型参数，P 可以是任何能转换为 Path 的类型
    // AsRef 是一个 trait，表示可以转换为某个类型的引用
    // 这允许调用者传入 &str、String 或 PathBuf 等类型
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        // let 用于绑定变量，content 的类型由右边表达式推断
        // fs::read_to_string 读取整个文件内容为 String
        // ? 是错误传播操作符，如果结果是 Err，立即返回错误
        let content = fs::read_to_string(path)?;
        // toml::from_str 将 TOML 格式的字符串解析为 Rust 类型
        //::<Config> 是类型标注（turbofish 语法），指定要解析成的类型
        let config: Config = toml::from_str(&content)?;
        // &content 是借用操作，借用 content 的引用而不转移所有权
        // config.validate() 调用 Config 的验证方法
        // ? 传播验证错误
        config.validate()?;
        // Ok(...) 包装成功结果
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
    // &self 是方法的第一个参数，表示借用实例的不可变引用
    // self 类似于其他语言中的 this，但 Rust 显式声明
    pub fn validate(&self) -> Result<(), ConfigError> {
        // mut 表示这个变量是可变的，默认变量是不可变的
        // HashSet::new() 创建一个新的空哈希集合
        // 类型 String 由编译器自动推断
        let mut seen_ids = HashSet::new();

        // for 循环遍历集合中的每个元素
        // &self.devices 借用 devices 向量的引用
        // device 是迭代变量，类型是 &Device（对 Device 的引用）
        for device in &self.devices {
            // HashSet::insert 方法插入一个值并返回布尔值
            // 如果值已存在返回 false，表示重复
            // &device.id 借用 id 字段，因为 String 不实现 Copy trait
            if !seen_ids.insert(&device.id) {
                // return 提前返回错误
                // device.id.clone() 克隆字符串，因为需要拥有所有权来构造错误
                return Err(ConfigError::DuplicateDeviceId(device.id.clone()));
            }

            // 遍历设备的寄存器映射
            for mapping in &device.register_mappings {
                // trim() 去除字符串两端的空白字符
                // addr 类型是 &str（字符串切片），是借用
                let addr = mapping.address.trim();

                // len() 返回字符串的字节长度
                if addr.len() < 2 {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                // [0..1] 是范围切片，获取第 0 个字符（字节）
                // to_lowercase() 转为小写，返回新的 String
                let prefix = &addr[0..1].to_lowercase();
                // [1..] 范围从第 1 个字符到末尾
                let num_str = &addr[1..];

                // matches! 宏检查值是否匹配给定的模式
                // 这里检查前缀是否是 h、d、c、i 之一
                if !matches!(prefix.as_str(), "h" | "d" | "c" | "i") {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                // if let 是模式匹配语法，用于解包 Result 或 Option
                // parse::<u32>() 尝试将字符串解析为 u32
                // Ok(num) 表示解析成功，num 是解析后的值
                if let Ok(num) = num_str.parse::<u32>() {
                    // Modbus 地址最大为 65535 (16 位无符号整数的最大值)
                    if num > 65535 {
                        return Err(ConfigError::AddressOutOfRange(
                            device.id.clone(),
                            mapping.signal_name.clone(),
                            num,
                        ));
                    }
                } else {
                    // 解析失败，说明数字部分无效
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }
            }
        }
        // 验证通过，返回 Ok(())
        // () 是单位类型，表示没有有意义的返回值
        Ok(())
    }
}

// #[#[cfg(test)] 属性表示以下代码只在测试模式下编译
// cargo test 会编译并运行这部分代码
#[cfg(test)]
// mod 定义一个模块
// tests 是测试模块的常规名称
mod tests {
    // use super::* 导入父模块的所有公共项
    // super 表示父模块（即 config 模块）
    // * 是通配符，导入所有内容
    use super::*;

    // #[test] 标记这是一个测试函数
    // cargo test 会自动发现并运行这些函数
    #[test]
    fn test_default_config() {
        // r#"..."# 是原始字符串字面量，允许包含引号而无需转义
        // TOML 格式的配置字符串
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
"#;
        // unwrap() 解包 Result，如果是 Err 则 panic（测试失败）
        // 在测试中使用 unwrap 是常见做法
        let config: Config = toml::from_str(config_str).unwrap();
        // assert_eq! 断言两个值相等，不相等则测试失败
        assert_eq!(config.server.rpc_port, 8080);
        // len() 返回向量的长度
        assert_eq!(config.devices.len(), 0);
    }
}
