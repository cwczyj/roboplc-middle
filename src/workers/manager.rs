//! # Device Manager Worker
//!
//! 设备消息路由器，负责在RpcWorker、HttpWorker和ModbusWorker之间路由消息。
//!
//! ## 功能
//!
//! - 注册所有设备到共享状态
//! - 路由DeviceControl消息到对应的ModbusWorker
//! - 路由DeviceResponse消息回请求的worker
//! - 维护待处理请求映射（correlation_id -> sender）

// use关键字用于导入其他模块中的类型、函数或trait
// crate::表示当前crate（项目）的根模块
// config模块包含配置相关的结构体和函数
use crate::config::Config;
// 从crate根导入Message和Variables类型
// Message是枚举类型，表示worker之间传递的各种消息
// Variables是共享状态结构体
use crate::{DeviceResponseData, Message, Variables};
// roboplc是实时PLC框架，controller模块包含Worker相关的基础trait和宏
// prelude模块通常包含最常用的类型，使用*通配符导入所有公开项
use roboplc::controller::prelude::*;
// roboplc的基础prelude，包含框架核心功能
use roboplc::prelude::*;
// serde是Rust的序列化/反序列化框架
// Deserialize用于从JSON等格式解析结构体
// Serialize用于将结构体转换为JSON等格式
use serde::{Deserialize, Serialize};
// HashMap是标准库提供的哈希表数据结构，用于键值对存储
// K是键类型，V是值类型，默认使用SipHash算法防止HashDoS攻击
use std::collections::HashMap;
// Sender是标准库mpsc（多生产者单消费者）通道的发送端
// 用于线程间安全地发送消息
use std::sync::mpsc::Sender;
// time模块包含时间相关的类型，如Instant（时间点）、Duration（时间间隔）
use std::time;

// WorkerOpts是RoboPLC框架提供的派生宏
// 用于为Worker结构体生成配置选项相关的代码
#[derive(WorkerOpts)]
// 属性宏，用于配置Worker的元数据
// name指定worker的名称，用于日志和监控识别
#[worker_opts(name = "device_manager")]
// pub struct定义一个公共结构体
// DeviceManager是这个Worker的名称，采用PascalCase命名规范
pub struct DeviceManager {
    // Config结构体保存从配置文件加载的所有配置信息
    // 包含设备列表、服务器端口等
    config: Config,
    // HashMap<K, V>是哈希映射表，也叫字典或关联数组
    // 这里键是String（设备ID），值也是String（worker名称）
    // 用于将设备ID映射到对应的ModbusWorker名称
    // 例如: {"plc-1" -> "modbus_worker_plc-1"}
    worker_map: HashMap<String, String>,
    // u64是64位无符号整数，范围0到2^64-1
    // 这里作为correlation_id（关联ID），用于匹配请求和响应
    // Sender<DeviceResponseData>是通道发送端，可以发送DeviceResponseData类型的数据
    // 这个映射用于存储等待响应的请求，当收到响应时通过对应Sender通知请求者
    pending_requests: HashMap<u64, Sender<DeviceResponseData>>,
}

// impl为结构体实现方法
// impl DeviceManager表示为DeviceManager结构体实现关联函数和方法
impl DeviceManager {
    // pub fn定义一个公共关联函数（构造函数）
    // new是Rust的惯用构造函数名称
    // 参数config: Config表示接受一个Config类型的参数
    // -> Self表示返回Self类型，Self是impl块所实现类型的别名（这里是DeviceManager）
    pub fn new(config: Config) -> Self {
        // let用于绑定变量，mut表示这个变量是可变的
        // 默认变量是不可变的（immutable），修改需要mut关键字
        // HashMap::new()创建一个新的空哈希表
        let mut worker_map = HashMap::new();
        // for循环遍历集合
        // &config.devices获取devices字段的引用，避免移动所有权
        // device是迭代变量，每次循环代表一个设备配置
        for device in &config.devices {
            // HashMap的insert方法插入键值对
            // device.id.clone()克隆设备ID字符串
            // 因为String没有实现Copy trait，移动会转移所有权
            // format!宏用于格式化字符串，类似于println!但不输出到控制台
            // {}是占位符，会被后面的值替换
            worker_map.insert(device.id.clone(), format!("modbus_worker_{}", device.id));
        }
        // Self { ... }是结构体实例化语法
        // 创建DeviceManager实例并返回
        // 字段初始化简写: config等价于config: config
        Self {
            config,
            worker_map,
            // HashMap::new()创建空的pending_requests映射
            pending_requests: HashMap::new(),
        }
    }

    // pub fn定义一个公共方法
    // &self是不可变借用，表示只读访问实例
    // &str是字符串切片，是String的借用形式，更灵活
    // Option<&String>返回类型表示可能找到也可能找不到
    // Some(&String)表示找到了，None表示未找到
    pub fn get_worker_name(&self, device_id: &str) -> Option<&String> {
        // HashMap::get方法根据键查找值
        // 返回Option<&V>，对值的引用
        // 如果键存在返回Some(&value)，不存在返回None
        self.worker_map.get(device_id)
    }

    // fn定义私有方法，只能在当前模块内访问
    // &self是不可变借用，&Context<Message, Variables>是对上下文的可变借用
    // Context是RoboPLC框架提供的上下文，包含Hub和共享状态访问
    fn register_devices(&self, context: &Context<Message, Variables>) {
        // context.variables()获取共享状态的引用
        // device_states是共享状态中存储设备状态的部分
        // .write()获取写锁，返回RwLockWriteGuard
        // RwLock（读写锁）允许多个读者或单个写者
        // mut表示这个守卫是可变的，可以修改锁保护的数据
        let mut states = context.variables().device_states.write();
        // 遍历所有设备配置
        for device in &self.config.devices {
            // HashMap::insert插入键值对
            // 如果键已存在会返回旧的值，这里我们忽略返回值
            states.insert(
                // device.id.clone()克隆字符串作为键
                device.id.clone(),
                // crate::DeviceStatus创建设备状态结构体
                crate::DeviceStatus {
                    // 初始状态为未连接
                    connected: false,
                    // time::Instant::now()获取当前时间点
                    // 用于记录最后通信时间
                    last_communication: time::Instant::now(),
                    // 初始错误计数为0
                    // u32类型，32位无符号整数
                    error_count: 0,
                    // 初始重连计数为0
                    reconnect_count: 0,
                },
            );
            // tracing::info!是结构化日志宏，记录信息级别日志
            // {}是格式化占位符，会被后面的device.id替换
            // %device_id表示使用Display trait格式化
            tracing::info!("Registered device: {}", device.id);
        }
    }
}

// impl Trait for Type 语法为类型实现trait
// Worker<Message, Variables>是RoboPLC框架定义的工作者trait
// 需要实现run方法作为worker的主循环
impl Worker<Message, Variables> for DeviceManager {
    // fn定义方法，&mut self是可变借用，允许修改实例
    // WResult是RoboPLC定义的结果类型，用于worker的返回
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // context.hub()获取消息总线（Hub）的引用
        // Hub是RoboPLC的核心组件，负责worker间消息传递
        // register方法注册当前worker到Hub，返回Client用于接收消息
        // "device_manager"是worker的标识名
        let client = context.hub().register(
            "device_manager",
            // event_matches!是RoboPLC提供的宏
            // 用于定义worker感兴趣的消息模式
            // |表示"或"关系，这个worker接收两种消息：
            // 1. DeviceControl消息（控制设备）
            // 2. DeviceResponse消息（设备响应）
            // ..表示忽略其他字段，只匹配消息类型
            event_matches!(Message::DeviceControl { .. } | Message::DeviceResponse { .. }),
        )?;
        // ?是错误传播运算符，如果Result是Err会立即返回错误

        // tracing::info!宏记录日志，支持结构化字段
        // 使用key = value语法添加结构化字段
        tracing::info!(
            "Device Manager started, routing {} devices",
            self.config.devices.len()
        );

        // 调用实例方法注册所有设备到共享状态
        self.register_devices(context);

        // for msg in client 遍历消息通道
        // client实现了Iterator trait，可以迭代接收消息
        // 当Hub发送匹配的消息时，for循环会接收到
        // 如果Hub关闭，迭代会结束
        for msg in client {
            // match表达式进行模式匹配，是Rust的核心特性
            // 根据msg的不同变体执行不同代码块
            match msg {
                // 匹配DeviceControl消息变体
                // { .. }语法解构消息，提取字段到局部变量
                Message::DeviceControl {
                    device_id,      // 设备ID
                    operation,      // 操作类型
                    params,         // 操作参数
                    correlation_id, // 关联ID，用于匹配请求和响应
                    respond_to,
                } => {
                    // Store respond_to in pending_requests if present
                    if let Some(sender) = respond_to {
                        self.pending_requests.insert(correlation_id, sender);
                    }

                    // tracing::debug!记录调试级别日志
                    // %表示使用Display trait格式化
                    // ?表示使用Debug trait格式化
                    tracing::debug!(
                        device_id = %device_id,
                        operation = ?operation,
                        "Received DeviceControl request"
                    );

                    // match表达式对get_worker_name的结果进行匹配
                    // 根据设备ID查找对应的ModbusWorker
                    match self.get_worker_name(&device_id) {
                        // Some表示找到了对应的worker
                        Some(worker_name) => {
                            // tracing::trace!记录追踪级别日志，比debug更详细
                            tracing::trace!(
                                device_id = %device_id,
                                worker_name = %worker_name,
                                "Forwarding DeviceControl to worker"
                            );
                            // context.hub().send()通过Hub发送消息
                            // 这条消息会被所有订阅了DeviceControl的worker接收
                            // 特别是对应的ModbusWorker
                            context.hub().send(Message::DeviceControl {
                                // 这里使用字段初始化简写
                                // device_id等价于device_id: device_id
                                device_id,
                                operation,
                                params,
                                correlation_id,
                                respond_to: None,
                            });
                        }
                        // None表示没有找到对应的worker
                        None => {
                            // tracing::error!记录错误级别日志
                            tracing::error!(
                                device_id = %device_id,
                                "No worker found for device"
                            );
                        }
                    }
                }
                // 匹配DeviceResponse消息变体
                Message::DeviceResponse {
                    device_id,
                    success,
                    data,
                    error,
                    correlation_id,
                } => {
                    tracing::debug!(
                        device_id = %device_id,
                        success = success,
                        "Received DeviceResponse"
                    );

                    // if let是match的简写形式，只匹配一个模式
                    // self.pending_requests.remove(&correlation_id)从映射中删除并获取值
                    // &correlation_id表示借用correlation_id作为查找键
                    // 返回Option<Sender<DeviceResponseData>>
                    if let Some(sender) = self.pending_requests.remove(&correlation_id) {
                        // 创建响应数据元组
                        let response_data = (success, data, error);
                        // sender.send()通过通道发送响应
                        // 返回Result<(), SendError<T>>
                        // if let Err(e)匹配发送失败的情况
                        if let Err(e) = sender.send(response_data) {
                            // 发送失败通常意味着接收端已关闭
                            tracing::warn!(
                                correlation_id = correlation_id,
                                error = %e,
                                "Failed to send response to requester"
                            );
                        }
                    } else {
                        // 没有找到对应的pending request
                        // 可能是请求已超时或correlation_id无效
                        tracing::warn!(
                            correlation_id = correlation_id,
                            "No pending request found for correlation_id"
                        );
                    }
                }
                // 匹配DeviceHeartbeat消息但忽略内容
                // ..表示忽略所有字段
                // 设备管理器不处理心跳消息
                Message::DeviceHeartbeat { .. } => {}
                // 匹配ConfigUpdate消息但忽略内容
                // 设备管理器不处理配置更新消息
                Message::ConfigUpdate { .. } => {}
                // 匹配SystemStatus消息但忽略内容
                // 设备管理器不处理系统状态消息
                Message::SystemStatus { .. } => {}
                // 匹配TimeoutCleanup消息，清理超时的请求
                // 当RpcWorker检测到超时时，发送此消息给DeviceManager清理pending_requests中的条目
                Message::TimeoutCleanup { correlation_id } => {
                    if let Some(_) = self.pending_requests.remove(&correlation_id) {
                        tracing::debug!(
                            correlation_id,
                            "Cleaned up timed-out request from pending_requests"
                        );
                    }
                }
            }
        }

        // 当消息通道关闭时，for循环结束，执行到这里
        tracing::info!("Device Manager stopped");
        // Ok(())返回成功的结果
        // ()是单元类型，表示"没有值"
        Ok(())
    }
}
