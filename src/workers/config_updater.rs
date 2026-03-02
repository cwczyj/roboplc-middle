//! # Config Updater Worker
//!
//! 配置更新应用worker。
//!
//! ## 功能
//!
//! - 接收ConfigUpdate消息
//! - 更新设备列表（添加/删除设备）
//! - 更新设备状态映射

// use 是 Rust 的关键字，用于将其他模块或 crate 中的名称导入当前作用域
// crate 是 Rust 的关键字，表示当前 crate（项目）的根
// 这里从 crate::config 模块导入 Config 结构体
// Config 结构体用于存储整个系统的配置信息（设备列表、服务器设置等）
use crate::config::Config;

// 从 crate 根导入 DeviceStatus、Message、Variables 三个类型
// DeviceStatus: 记录单个设备的连接状态和统计信息
// Message: 定义在 Hub 中传递的所有消息类型（枚举）
// Variables: 所有 workers 共享的全局状态结构体
use crate::{DeviceStatus, Message, Variables};

// 导入 parking_lot_rt crate 中的 RwLock
// parking_lot_rt 是一个实时安全的同步原语库，比标准库的 RwLock 性能更好
// RwLock 是 "Read-Write Lock" 的缩写，读写锁
// 读写锁允许多个读者同时读取，或者单个写者写入（但不能同时进行）
use parking_lot_rt::RwLock;

// 导入 roboplc crate 的 controller 模块的 prelude
// roboplc 是一个用于工业控制的实时 PLC 框架
// prelude 是 Rust 中常用的命名约定，表示 "prelude"（前奏曲）
// 它通常包含该 crate 最常用的类型和 trait，方便用户快速开始
// * 表示导入 prelude 中所有公开的内容
use roboplc::controller::prelude::*;

// 导入 roboplc crate 的主 prelude 模块
// 这里可能包含 Worker trait、Context 类型等核心组件
use roboplc::prelude::*;

// 导入标准库 collections 模块中的 HashMap
// HashMap 是哈希映射，一种键值对数据结构
// 它通过哈希函数将键映射到值，提供平均 O(1) 的查找性能
use std::collections::HashMap;

// 导入标准库 sync 模块中的 Arc
// Arc 是 "Atomic Reference Counting"（原子引用计数）的缩写
// 它是线程安全的智能指针，允许多个线程共享同一数据的所有权
// 当最后一个 Arc 被丢弃时，数据才会被释放
use std::sync::Arc;

// 导入标准库 time 模块中的 Instant
// Instant 是一个单调递增的时间点，用于测量时间间隔
// 它不受系统时间修改的影响，适合用于计时和超时检测
use std::time::Instant;

// #[derive(...)] 是一个属性宏（attribute macro）
// derive 宏会自动为类型实现指定的 trait
// WorkerOpts trait 来自 roboplc 框架，用于定义 worker 的元数据选项
// pub struct 定义一个公开的结构体（struct）
// 结构体是将多个相关值组合在一起的复合类型
#[derive(WorkerOpts)]
// #[worker_opts(...)] 是 roboplc 框架提供的属性宏
// 它允许为 WorkerOpts trait 的实现提供配置参数
// name = "config_updater": 为这个 worker 指定名称
// 这个名称用于日志记录和调试识别
#[worker_opts(name = "config_updater")]

// pub 关键字表示这个结构体是公开的，可以被其他模块访问
// struct 关键字定义结构体
// ConfigUpdater 是结构体的名称，遵循 Rust 的 PascalCase 命名规范
// 这个结构体代表配置更新 worker，负责处理配置变更
pub struct ConfigUpdater {
    // config 字段，类型是 Config
    // Config 结构体定义在 crate::config 模块中
    // 它包含当前系统的完整配置（设备列表、服务器端口等）
    // 这里没有 pub 修饰，表示这个字段是私有的，只能在 ConfigUpdater 内部访问
    config: Config,
}

// impl 是 Rust 的关键字，用于为类型实现方法或 trait
// impl ConfigUpdater 表示为 ConfigUpdater 结构体实现方法
// 这段代码块中定义的所有方法都属于 ConfigUpdater 类型
impl ConfigUpdater {
    // pub 关键字表示这个方法是公开的
    // fn 是 Rust 定义函数的关键字
    // new 是方法名，这是 Rust 中构造函数的标准命名约定
    // (config: Config) 是参数列表，接收一个 Config 类型的参数
    // -> Self 是返回类型，Self 是 ConfigUpdater 的别名
    // 这个方法返回一个新的 ConfigUpdater 实例
    pub fn new(config: Config) -> Self {
        // Self { ... } 语法创建并返回一个新的结构体实例
        // config: 将传入的参数赋值给结构体的 config 字段
        // 这是结构体字面量语法，字段名和值用冒号分隔
        Self { config }
    }

    // fn 定义方法
    // apply_config_update 是方法名，描述这个方法的功能：应用配置更新
    // &mut self 是方法的第一个参数，表示对自身的可变引用
    // &mut 表示可变引用，允许修改 self 指向的数据
    // 这对应于其他语言中的 "this" 指针，但在 Rust 中必须显式声明
    // (config_json: &str, ...) 是第二个参数
    // &str 是字符串切片类型，表示对字符串的不可变引用
    // 它是一种轻量级的字符串视图，不拥有数据所有权
    fn apply_config_update(
        &mut self,
        config_json: &str,
        // device_states 参数的类型是 &Arc<RwLock<HashMap<String, DeviceStatus>>>
        // & 表示引用，这里是对 Arc 的引用
        // Arc<RwLock<...>> 是多层包装：
        //   - Arc: 线程安全的引用计数，允许多线程共享
        //   - RwLock: 读写锁，允许多读单写
        //   - HashMap<String, DeviceStatus>: 键为设备ID字符串，值为设备状态
        device_states: &Arc<RwLock<HashMap<String, DeviceStatus>>>,
        // ) 结束参数列表
        // -> 指定返回类型
        // Result<ConfigUpdateSummary, Box<dyn std::error::Error>> 是 Result 枚举
        // Result 是 Rust 的错误处理类型，有两个变体：Ok(T) 和 Err(E)
        // ConfigUpdateSummary 是成功时的返回类型
        // Box<dyn std::error::Error> 是错误时的返回类型
        // Box 是堆分配的智能指针
        // dyn 关键字表示动态分发（trait object）
        // std::error::Error 是标准库的错误 trait
        // 这种写法允许返回任何实现了 Error trait 的错误类型
    ) -> Result<ConfigUpdateSummary, Box<dyn std::error::Error>> {
        // let 关键字用于绑定变量
        // new_config 是变量名
        // : Config 是类型注解，显式指定变量类型（这里可由编译器推断，但显式更清晰）
        // serde_json::from_str 是 serde_json crate 的函数
        // 它将 JSON 字符串反序列化为 Rust 数据结构
        // <Config> 是泛型参数，指定要反序列化的目标类型
        // (config_json) 是函数参数，传入 JSON 字符串
        // ? 是问号操作符，如果结果是 Err，立即返回该错误
        // 它是 Rust 中简洁的错误传播语法糖
        let new_config: Config = serde_json::from_str(config_json)?;

        // let 绑定变量 old_device_ids
        // std::collections::HashSet<_> 是标准库的哈希集合类型
        // HashSet 存储唯一的元素，不允许重复
        // _ 是类型占位符，让编译器推断具体类型
        // : 后面的下划线告诉编译器 "这里填什么类型都行，你决定"
        let old_device_ids: std::collections::HashSet<_> =
            // self.config 访问结构体的 config 字段
            // .devices 访问 Config 结构体的 devices 字段（设备列表）
            // .iter() 创建迭代器，用于遍历集合
            // .map(|d| ...) 对迭代器中的每个元素应用闭包（匿名函数）
            // |d| 是闭包参数语法，d 表示当前设备
            // d.id.as_str() 获取设备 ID 的字符串切片引用
            // .collect() 将迭代器的结果收集到集合中
            // 这里收集到 HashSet，会自动去重
            self.config.devices.iter().map(|d| d.id.as_str()).collect();

        // 同理，创建新配置中的设备 ID 集合
        // new_config 是传入的新配置
        // 同样的模式：遍历、提取 ID、收集到 HashSet
        let new_device_ids: std::collections::HashSet<_> =
            new_config.devices.iter().map(|d| d.id.as_str()).collect();

        // let 绑定 added 变量
        // Vec<_> 是向量（动态数组）类型
        // Vec 是 Rust 最常用的序列类型，可以动态增长和收缩
        // new_device_ids.difference(&old_device_ids) 计算集合差集
        // difference 返回在新集合中但不在旧集合中的元素
        // &old_device_ids 传入旧集合的引用
        // 结果是迭代器，产生 "新增的设备的 ID"
        let added: Vec<_> = new_device_ids
            .difference(&old_device_ids)
            // .map(|s| s.to_string()) 将 &str 转换为 String
            // to_string() 分配新的堆内存，创建拥有所有权的字符串
            .map(|s| s.to_string())
            // .collect() 收集到 Vec<String>
            .collect();

        // removed 变量：计算被移除的设备 ID
        // old_device_ids.difference(&new_device_ids) 计算在旧配置中但不在新配置中的设备
        let removed: Vec<_> = old_device_ids
            .difference(&new_device_ids)
            .map(|s| s.to_string())
            .collect();

        // unchanged 变量：计算保持不变的设备 ID
        // .intersection(&new_device_ids) 计算交集
        // 返回在两个集合中都存在的元素
        let unchanged: Vec<_> = old_device_ids
            .intersection(&new_device_ids)
            .map(|s| s.to_string())
            .collect();

        // let mut 声明可变变量 states
        // mut 关键字表示这个变量可以被重新赋值或其内容可以被修改
        // device_states.write() 获取 RwLock 的写锁
        // write() 会阻塞直到获得写锁
        // 返回的是写锁守卫（WriteGuard），实现了 DerefMut trait
        // 当 states 变量离开作用域时，锁会自动释放（RAII 模式）
        let mut states = device_states.write();

        // for 是 Rust 的循环关键字
        // for device_id in &removed 遍历 removed 向量中的每个元素
        // &removed 是对向量的引用，迭代时会产生 &&String，然后自动解引用
        // device_id 是循环变量，表示当前遍历到的设备 ID
        for device_id in &removed {
            // states.remove(device_id) 从 HashMap 中移除键值对
            // 传入设备 ID，删除对应的 DeviceStatus
            // 如果键存在则删除并返回 Some(值)，否则返回 None
            // 这里忽略返回值，只关心删除操作本身
            states.remove(device_id);
        }

        // 遍历新增的设备列表
        for device_id in &added {
            // states.insert(...) 向 HashMap 插入键值对
            // 第一个参数 device_id.clone() 是键的克隆
            // clone() 创建 String 的深拷贝，因为 HashMap 需要拥有键的所有权
            // 第二个参数是 DeviceStatus 结构体的实例
            states.insert(
                device_id.clone(),
                // DeviceStatus { ... } 创建结构体实例
                // connected: false 表示设备初始状态为未连接
                // Instant::now() 获取当前时间点
                // error_count: 0 初始错误计数为 0
                // reconnect_count: 0 初始重连计数为 0
                DeviceStatus {
                    connected: false,
                    last_communication: Instant::now(),
                    error_count: 0,
                    reconnect_count: 0,
                },
            );
        }

        // self.config = new_config 更新配置
        // 将新配置赋值给 self 的 config 字段
        // 这会消耗 new_config（转移所有权）
        // 旧配置会被丢弃（如果实现了 Drop trait 则会调用）
        self.config = new_config;

        // Ok(...) 创建 Result 的 Ok 变体
        // 包裹 ConfigUpdateSummary 结构体实例
        // 返回成功结果，包含新增、移除、保持不变的设备列表
        Ok(ConfigUpdateSummary {
            added,
            removed,
            unchanged,
        })
    }
}

// struct 定义结构体
// ConfigUpdateSummary 用于存储配置更新的摘要信息
// 这个结构体没有 pub 修饰，表示它是模块私有的
// 只能在 config_updater.rs 模块内部使用
struct ConfigUpdateSummary {
    // added 字段，类型是 Vec<String>
    // 存储新增设备的 ID 列表
    added: Vec<String>,

    // removed 字段，类型是 Vec<String>
    // 存储被移除设备的 ID 列表
    removed: Vec<String>,

    // unchanged 字段，类型是 Vec<String>
    // 存储保持不变的设备 ID 列表
    unchanged: Vec<String>,
}

// impl Worker<Message, Variables> for ConfigUpdater
// 这是 trait 实现的语法
// Worker 是 roboplc 框架定义的 trait
// <Message, Variables> 是泛型参数，指定消息类型和共享变量类型
// for ConfigUpdater 表示为 ConfigUpdater 实现 Worker trait
// trait 定义了类型的行为契约，实现 trait 必须提供指定的方法
impl Worker<Message, Variables> for ConfigUpdater {
    // fn run 定义 run 方法，这是 Worker trait 的核心方法
    // &mut self 是可变引用，允许修改 worker 状态
    // context: &Context<Message, Variables> 是上下文参数
    // &Context 包含 Hub 访问、共享变量、系统状态等
    // Context 是 roboplc 框架提供的运行时上下文
    // -> WResult 是返回类型，WResult 是 roboplc 定义的结果类型
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // let client = ... 绑定 client 变量
        // context.hub() 获取消息中心（Hub）的引用
        // Hub 是 roboplc 的消息路由系统，用于 worker 间通信
        // .register(...) 注册当前 worker 为消息消费者
        // 第一个参数 "config_updater" 是注册名称，用于识别
        // 第二个参数是过滤器，决定接收哪些消息
        let client = context.hub().register(
            "config_updater",
            // event_matches!(...) 是 roboplc 提供的宏
            // 用于创建消息过滤器
            // Message::ConfigUpdate { .. } 匹配 ConfigUpdate 变体
            // .. 表示匹配该变体的所有字段（忽略具体值）
            // 这表示只接收 ConfigUpdate 类型的消息
            event_matches!(Message::ConfigUpdate { .. }),
            // ? 操作符处理错误，如果注册失败则立即返回错误
        )?;

        // tracing::info! 是日志宏，用于记录信息级别日志
        // tracing 是 Rust 的结构化日志框架
        // "Config Updater started..." 是日志消息
        // 这条日志表示 worker 已启动并等待配置更新
        tracing::info!("Config Updater started, waiting for config updates");

        // for msg in client 遍历客户端接收的消息
        // client 实现了 Iterator trait，可以产生接收到的消息
        // 这是一个阻塞迭代，当有新消息到达时继续
        // worker 会一直运行，直到 client 被关闭或程序终止
        for msg in client {
            // match 是 Rust 的模式匹配关键字
            // 它类似于其他语言的 switch，但更强大
            // match msg 根据 msg 的值执行不同的分支
            match msg {
                // Message::ConfigUpdate { config } 匹配 ConfigUpdate 变体
                // { config } 解构模式，提取 config 字段到同名变量
                // 这行只匹配 ConfigUpdate 类型的消息
                Message::ConfigUpdate { config } => {
                    // 嵌套 match 处理 apply_config_update 的结果
                    // self.apply_config_update(...) 调用配置更新方法
                    // &config 传入配置字符串的引用
                    // &context.variables().device_states 传入设备状态映射的引用
                    match self.apply_config_update(&config, &context.variables().device_states) {
                        // Ok(summary) 匹配成功结果，解构出 summary
                        // summary 是 ConfigUpdateSummary 类型
                        Ok(summary) => {
                            // tracing::info! 记录结构化日志
                            // 括号中的键值对是结构化字段
                            // added_devices = ?summary.added: ? 表示使用 Debug 格式打印
                            // summary.added 访问 added 字段
                            tracing::info!(
                                added_devices = ?summary.added,
                                removed_devices = ?summary.removed,
                                unchanged_devices = ?summary.unchanged,
                                total_devices = self.config.devices.len(),
                                // 最后的位置参数是日志消息文本
                                "Config applied successfully"
                            );
                        }
                        // Err(e) 匹配错误结果，解构出错误 e
                        // 使用 tracing::error! 记录错误日志
                        Err(e) => {
                            tracing::error!(
                                // error = %e: % 表示使用 Display 格式打印错误
                                error = %e,
                                "Failed to apply config update"
                            );
                        }
                    }
                }
                // _ 是通配模式，匹配所有其他情况
                // 由于我们只注册了 ConfigUpdate 消息，这行实际上不会执行
                // 但 Rust 要求 match 必须覆盖所有可能的情况（穷尽性检查）
                _ => {}
            }
        }

        // 当消息循环结束时（client 被关闭），记录停止日志
        tracing::info!("Config Updater stopped");

        // Ok(()) 返回 Ok 变体，包含单元值 ()
        // 单元值是空元组，表示 "没有有意义的返回值"
        // 这表示 worker 正常结束
        Ok(())
    }
}
