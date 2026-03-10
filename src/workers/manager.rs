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

// use 关键字用于导入其他模块中的类型、函数或 trait
// crate::表示当前 crate（项目）的根模块
// config 模块包含配置相关的结构体和函数
use crate::config::Config;
// 从 crate 根导入 Message 和 Variables 类型
// Message 是枚举类型，表示 worker 之间传递的各种消息
// Variables 是共享状态结构体
use crate::{DeviceResponseData, Message, Variables};
// roboplc 是实时 PLC 框架，controller 模块包含 Worker 相关的基础 trait 和宏
// prelude 模块通常包含最常用的类型，使用*通配符导入所有公开项
use roboplc::controller::prelude::*;
// roboplc 的基础 prelude，包含框架核心功能
use roboplc::prelude::*;
// serde 是 Rust 的序列化/反序列化框架
// Deserialize 用于从 JSON 等格式解析结构体
// Serialize 用于将结构体转换为 JSON 等格式
// HashMap 是标准库提供的哈希表数据结构，用于键值对存储
// K 是键类型，V 是值类型，默认使用 SipHash 算法防止 HashDoS 攻击
use std::collections::HashMap;
// Sender 是标准库 mpsc（多生产者单消费者）通道的发送端
// 用于线程间安全地发送消息
use std::sync::mpsc::Sender;
// time 模块包含时间相关的类型，如 Instant（时间点）、Duration（时间间隔）
use std::time;

// WorkerOpts 是 RoboPLC 框架提供的派生宏
// 用于为 Worker 结构体生成配置选项相关的代码
#[derive(WorkerOpts)]
// 属性宏，用于配置 Worker 的元数据
// name 指定 worker 的名称，用于日志和监控识别
#[worker_opts(name = "device_manager")]
// pub struct 定义一个公共结构体
// DeviceManager 是这个 Worker 的名称，采用 PascalCase 命名规范
pub struct DeviceManager {
    // Config 结构体保存从配置文件加载的所有配置信息
    // 包含设备列表、服务器端口等
    config: Config,
    // HashMap<K, V> 是哈希映射表，也叫字典或关联数组
    // 这里键是 String（设备 ID），值也是 String（worker 名称）
    // 用于将设备 ID 映射到对应的 ModbusWorker 名称
    // 例如：{"plc-1" -> "modbus_worker_plc-1"}
    worker_map: HashMap<String, String>,
    // u64 是 64 位无符号整数，范围 0 到 2^64-1
    // 这里作为 correlation_id（关联 ID），用于匹配请求和响应
    // Sender<DeviceResponseData> 是通道发送端，可以发送 DeviceResponseData 类型的数据
    // 这个映射用于存储等待响应的请求，当收到响应时通过对应 Sender 通知请求者
    // 
    // 注意：在直接响应模式下（ModbusWorker -> RpcWorker），这个字段不再使用
    // 保留用于未来可能的扩展
    #[allow(dead_code)]
    pending_requests: HashMap<u64, Sender<DeviceResponseData>>,
}

// impl 为结构体实现方法
// impl DeviceManager 表示为 DeviceManager 结构体实现关联函数和方法
impl DeviceManager {
    // pub fn 定义一个公共关联函数（构造函数）
    // new 是 Rust 的惯用构造函数名称
    // 参数 config: Config 表示接受一个 Config 类型的参数
    // -> Self 表示返回 Self 类型，Self 是 impl 块所实现类型的别名（这里是 DeviceManager）
    pub fn new(config: Config) -> Self {
        // let 用于绑定变量，mut 表示这个变量是可变的
        // 默认变量是不可变的（immutable），修改需要 mut 关键字
        // HashMap::new() 创建一个新的空哈希表
        let mut worker_map = HashMap::new();
        // for 循环遍历集合
        // &config.devices 获取 devices 字段的引用，避免移动所有权
        // device 是迭代变量，每次循环代表一个设备配置
        for device in &config.devices {
            // HashMap 的 insert 方法插入键值对
            // device.id.clone() 克隆设备 ID 字符串
            // 因为 String 没有实现 Copy trait，移动会转移所有权
            // format! 宏用于格式化字符串，类似于 println! 但不输出到控制台
            // {} 是占位符，会被后面的值替换
            worker_map.insert(device.id.clone(), format!("modbus_worker_{}", device.id));
        }
        // Self { ... } 是结构体实例化语法
        // 创建 DeviceManager 实例并返回
        // 字段初始化简写：config 等价于 config: config
        Self {
            config,
            worker_map,
            // HashMap::new() 创建空的 pending_requests 映射
            pending_requests: HashMap::new(),
        }
    }

    // pub fn 定义一个公共方法
    // &self 是不可变借用，表示只读访问实例
    // &str 是字符串切片，是 String 的借用形式，更灵活
    // Option<&String> 返回类型表示可能找到也可能找不到
    // Some(&String) 表示找到了，None 表示未找到
    #[allow(dead_code)]
    pub fn get_worker_name(&self, device_id: &str) -> Option<&String> {
        // HashMap::get 方法根据键查找值
        // 返回 Option<&V>，对值的引用
        // 如果键存在返回 Some(&value)，不存在返回 None
        self.worker_map.get(device_id)
    }

    // fn 定义私有方法，只能在当前模块内访问
    // &self 是不可变借用，&Context<Message, Variables> 是对上下文的可变借用
    // Context 是 RoboPLC 框架提供的上下文，包含 Hub 和共享状态访问
    fn register_devices(&self, context: &Context<Message, Variables>) {
        // context.variables() 获取共享状态的引用
        // device_states 是共享状态中存储设备状态的部分
        // .write() 获取写锁，返回 RwLockWriteGuard
        // RwLock（读写锁）允许多个读者或单个写者
        // mut 表示这个守卫是可变的，可以修改锁保护的数据
        let mut states = context.variables().device_states.write();
        // 遍历所有设备配置
        for device in &self.config.devices {
            // HashMap::insert 插入键值对
            // 如果键已存在会返回旧的值，这里我们忽略返回值
            states.insert(
                // device.id.clone() 克隆字符串作为键
                device.id.clone(),
                // crate::DeviceStatus 创建设备状态结构体
                crate::DeviceStatus {
                    // 初始状态为未连接
                    connected: false,
                    // time::Instant::now() 获取当前时间点
                    // 用于记录最后通信时间
                    last_communication: time::Instant::now(),
                    // 初始错误计数为 0
                    // u32 类型，32 位无符号整数
                    error_count: 0,
                    // 初始重连计数为 0
                    reconnect_count: 0,
                },
            );
            // tracing::info! 是结构化日志宏，记录信息级别日志
            // {} 是格式化占位符，会被后面的 device.id 替换
            // %device_id 表示使用 Display trait 格式化
            tracing::info!("Registered device: {}", device.id);
        }
    }
}

// impl Trait for Type 语法为类型实现 trait
// Worker<Message, Variables> 是 RoboPLC 框架定义的工作者 trait
// 需要实现 run 方法作为 worker 的主循环
impl Worker<Message, Variables> for DeviceManager {
    // fn 定义方法，&mut self 是可变借用，允许修改实例
    // WResult 是 RoboPLC 定义的结果类型，用于 worker 的返回
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // context.hub() 获取消息总线（Hub）的引用
        // Hub 是 RoboPLC 的核心组件，负责 worker 间消息传递
        // register 方法注册当前 worker 到 Hub，返回 Client 用于接收消息
        // "device_manager" 是 worker 的标识名
        let client = context.hub().register(
            "device_manager",
            // event_matches! 是 RoboPLC 提供的宏
            // 用于定义 worker 感兴趣的消息模式
            // 
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
        // ? 是错误传播运算符，如果 Result 是 Err 会立即返回错误

        // tracing::info! 宏记录日志，支持结构化字段
        // 使用 key = value 语法添加结构化字段
        tracing::info!(
            "Device Manager started, managing {} devices",
            self.config.devices.len()
        );

        // 调用实例方法注册所有设备到共享状态
        self.register_devices(context);

        // for msg in client 遍历消息通道
        // client 实现了 Iterator trait，可以迭代接收消息
        // 当 Hub 发送匹配的消息时，for 循环会接收到
        // 如果 Hub 关闭，迭代会结束
        for msg in client {
            // match 表达式进行模式匹配，是 Rust 的核心特性
            // 根据 msg 的不同变体执行不同代码块
            match msg {
                // 匹配 TimeoutCleanup 消息，清理超时的请求
                // 当 RpcWorker 检测到超时时，发送此消息通知其他组件
                Message::TimeoutCleanup { correlation_id } => {
                    if let Some(_) = self.pending_requests.remove(&correlation_id) {
                        tracing::debug!(
                            correlation_id,
                            "Cleaned up timed-out request from pending_requests"
                        );
                    }
                }
                // 忽略其他不相关的消息
                Message::DeviceControl { .. }
                | Message::DeviceResponse { .. }
                | Message::DeviceHeartbeat { .. }
                | Message::ConfigUpdate { .. }
                | Message::SystemStatus { .. } => {}
            }
        }

        // 当消息通道关闭时，for 循环结束，执行到这里
        tracing::info!("Device Manager stopped");
        // Ok(()) 返回成功的结果
        // () 是单元类型，表示"没有值"
        Ok(())
    }
}