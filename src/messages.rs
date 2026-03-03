//! # 消息模块
//!
//! 定义在 RoboPLC Hub 中传递的所有消息类型。
//!
//! ## 消息传递机制
//!
//! RoboPLC 使用 Hub 模式在 workers 之间传递消息：
//! - RpcWorker 接收 JSON-RPC 请求，发送 DeviceControl 消息
//! - ModbusWorker 接收 DeviceControl 消息，执行 Modbus 操作，返回 DeviceResponse 消息
//! - HttpWorker 查询系统状态，发送 SystemStatus 消息
//!
//! ## 消息类型
//!
//! - `DeviceControl`: 设备控制请求
//! - `DeviceResponse`: 设备响应
//! - `DeviceHeartbeat`: 心跳消息（总是传递）
//! - `ConfigUpdate`: 配置更新通知（总是传递）
//! - `SystemStatus`: 系统状态查询
//!

// 导入 roboplc 框架的 prelude 模块
// use 关键字用于引入外部 crate 或模块中的类型和函数
// roboplc::prelude::* 表示导入 roboplc crate 中 prelude 模块的所有公开内容
// prelude 通常包含该 crate 最常用的类型，方便用户快速开始
use roboplc::prelude::*;

// 导入 serde 库中的 Deserialize 和 Serialize trait
// serde 是 Rust 中用于序列化和反序列化的标准库
// Serialize: 将 Rust 数据结构转换为 JSON/ YAML/ TOML 等格式
// Deserialize: 将外部格式转换回 Rust 数据结构
use serde::{Deserialize, Serialize};

// 导入 serde_json 库中的 Value 类型，并重命名为 JsonValue
// as 关键字用于给导入的类型起别名，避免命名冲突或简化名称
// Value 是 serde_json 中的动态 JSON 类型，可以表示任意 JSON 值（对象、数组、字符串、数字、布尔值、null）
use serde_json::Value as JsonValue;

// 导入标准库中的 mpsc（多生产者单消费者）通道的发送端类型
// std::sync::mpsc 是标准库提供的线程间通信机制
// Sender<T> 用于向通道发送类型为 T 的消息
use std::sync::mpsc::Sender;

// #[...] 是 Rust 的属性语法，用于给下面的代码元素添加元数据或启用特定功能
// derive 是一个宏（macro），它自动为类型实现指定的 trait
// Clone trait: 允许通过 .clone() 方法创建值的深拷贝
// Debug trait: 允许使用 {:?} 格式化打印值的调试信息
// DataPolicy trait: 这是 roboplc 框架特有的 trait，用于定义消息传递策略
// pub 关键字表示该类型是公开的（public），可以被其他模块访问
// enum 关键字定义枚举类型，枚举是 Rust 中表示多种可能变体的一种类型
// Message 是这个枚举的名称，代表 Hub 中传递的所有消息类型
#[derive(Clone, Debug, DataPolicy)]
pub enum Message {
    // #[data_delivery(single)] 是 roboplc 框架的属性宏
    // 表示这种消息只应该传递给一个消费者（单播）
    // 通常用于请求-响应模式的消息
    // DeviceControl 是枚举的一个变体（variant）
    // 这个变体包含具名字段（named fields），类似于结构体
    #[data_delivery(single)]
    DeviceControl {
        // device_id 字段，类型是 String（Rust 标准库中的动态字符串类型）
        // String 拥有其数据的所有权，可以在运行时动态增长和修改
        device_id: String,
        // operation 字段，类型是 Operation（同模块中定义的枚举）
        // 表示要执行的具体操作（如读寄存器、写寄存器等）
        operation: Operation,
        // params 字段，类型是 JsonValue
        // 使用 serde_json::Value 可以接收任意 JSON 格式的参数
        // 这提供了灵活性，因为不同操作需要不同的参数
        params: JsonValue,
        // correlation_id 字段，类型是 u64（无符号 64 位整数）
        // 用于关联请求和响应，确保响应能匹配到正确的请求
        correlation_id: u64,
        // respond_to 字段，类型是 Option<Sender<DeviceResponseData>>
        // 用于直接响应机制：请求者提供通道，响应者通过该通道返回结果
        // None 表示发送后不管（fire-and-forget）模式
        respond_to: Option<Sender<DeviceResponseData>>,
    },
    // DeviceResponse 变体，没有 #[data_delivery] 属性
    // 默认情况下，消息会按照 Hub 的路由规则传递
    // 这个变体用于返回设备操作的结果
    DeviceResponse {
        // 设备标识符，用于指明是哪个设备的响应
        device_id: String,
        // success 字段，类型是 bool（布尔值）
        // true 表示操作成功，false 表示失败
        success: bool,
        // 返回的数据，以 JSON 格式封装
        // 成功的操作会在这里放入结果数据
        data: JsonValue,
        // error 字段，类型是 Option<String>
        // Option 是 Rust 标准库中的枚举，表示可能存在也可能不存在的值
        // Some(String) 表示有错误信息，None 表示没有错误
        // Option<T> 比 null 更安全，强制开发者处理两种情况
        error: Option<String>,
        // 关联 ID，与 DeviceControl 中的 correlation_id 对应
        // 用于匹配请求和响应
        correlation_id: u64,
    },
    // #[data_delivery(always)] 表示这种消息应该传递给所有订阅者（广播）
    // 心跳消息需要让所有关心设备状态的组件都收到
    #[data_delivery(always)]
    DeviceHeartbeat {
        // 设备标识符
        device_id: String,
        // timestamp_ms 字段，类型是 u64
        // 表示心跳生成的时间戳（毫秒级）
        // 用于检测设备是否超时无响应
        timestamp_ms: u64,
        // latency_us 字段，类型是 u64
        // 表示通信延迟（微秒级）
        // 用于监控系统性能
        latency_us: u64,
    },
    // ConfigUpdate 也是广播消息，所有 worker 都需要知道配置发生了变化
    #[data_delivery(always)]
    ConfigUpdate {
        // 新配置的字符串表示（通常是序列化后的配置）
        // 使用 String 而不是结构体提供了灵活性
        config: String,
    },
    // TimeoutCleanup: sent via direct channel from RpcWorker to DeviceManager
    // when a request times out, allowing cleanup of pending_requests HashMap
    TimeoutCleanup {
        correlation_id: u64,
    },
    // SystemStatus 变体，用于查询系统状态
    SystemStatus {
        // requester 字段，表示请求者的标识
        // 用于记录谁发起了状态查询
        requester: String,
        // respond_to 字段，类型是 Sender<SystemStatusResponse>
        // Sender 是 mpsc 通道的发送端
        // <SystemStatusResponse> 是泛型参数，表示这个 Sender 专门发送 SystemStatusResponse 类型的消息
        // 这是一种回调机制：查询者提供一个通道，响应者通过这个通道返回结果
        respond_to: Sender<SystemStatusResponse>,
    },
}

// 为 Operation 枚举添加 derive 宏
// Clone: 可复制
// Debug: 可打印调试信息
// Serialize: 可序列化为 JSON 等格式（用于发送）
// Deserialize: 可从 JSON 等格式反序列化（用于接收）
// pub 公开访问权限
// enum 定义枚举
// Operation 表示可以在设备上执行的操作类型
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    // 枚举变体，每个变体代表一种操作
    // SetRegister: 设置/写入单个寄存器的值
    SetRegister,
    // GetRegister: 读取单个寄存器的值
    GetRegister,
    // WriteBatch: 批量写入多个寄存器
    WriteBatch,
    // ReadBatch: 批量读取多个寄存器
    ReadBatch,
    // MoveTo: 控制机械臂移动到指定位置（机器人特有的操作）
    MoveTo,
    // GetStatus: 获取设备的整体状态
    GetStatus,
}

// 定义 SystemStatusResponse 结构体
// #[derive(...)] 自动实现 Clone、Debug、Serialize、Deserialize trait
// pub 公开访问
// struct 定义结构体，结构体是将多个相关值组合在一起的复合类型
// SystemStatusResponse 包含系统状态查询的响应数据
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemStatusResponse {
    // pub 表示这个字段是公开的，外部代码可以直接访问
    // devices_count 字段，类型是 u32（无符号 32 位整数）
    // 表示当前连接的设备数量
    pub devices_count: u32,
    // system_healthy 字段，类型是 bool
    // true 表示系统运行正常，false 表示有问题
    pub system_healthy: bool,
    // uptime_secs 字段，类型是 u64
    // 表示系统已经运行的秒数（运行时间）
    pub uptime_secs: u64,
}


// Response data for device control operations.
// Using tuple type for compatibility with rpc_worker's ResponseSender
pub type DeviceResponseData = (bool, JsonValue, Option<String>);