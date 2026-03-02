//! # Latency Monitor Worker
//!
//! 延迟监控和异常检测worker。
//!
//! ## 功能
//!
//! - 接收设备心跳消息
//! - 维护延迟统计窗口
//! - 实现3-sigma异常检测
//! - 发送延迟异常事件

// =============================================================================
// 第一部分: 导入模块 (Imports)
// =============================================================================

// `use` 关键字用于将其他模块中的类型、函数、宏等导入到当前作用域
// 这样就不需要在每次使用时写出完整的路径

// `crate::` 是crate根路径的别名，表示从当前项目的根模块开始查找
// 这里导入的是在项目的lib.rs或main.rs中定义的模块
// DeviceEvent: 设备事件结构体，用于表示设备相关的事件
// DeviceEventType: 设备事件类型枚举（如Error, Warning等）
// LatencySample: 延迟样本结构体，存储一次延迟测量的数据
// Message: 消息枚举，worker之间通信的消息类型
// Variables: 共享变量结构体，用于在worker之间共享状态
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};

// 从roboplc框架导入控制器相关的预导模块(prelude)
// roboplc是一个实时PLC(可编程逻辑控制器)框架，用于工业控制应用
// prelude模块通常包含最常用的类型和trait，方便一次性导入
// Worker: trait，定义worker的基本行为
// WorkerOpts: 派生宏，用于配置worker选项
// Context: 运行上下文，包含消息总线、共享变量等
// WResult: worker返回的结果类型，通常是Result<(), WorkerError>
use roboplc::controller::prelude::*;

// 从roboplc框架导入通用预导模块
// 包含time、communication等常用功能
use roboplc::prelude::*;

// 从Rust标准库的collections模块导入集合类型
// `std` 是标准库的根模块
// `collections` 子模块包含各种集合数据结构
// HashMap: 哈希映射，键值对存储结构，基于哈希表实现，平均O(1)查找
// VecDeque: 双端队列(double-ended queue)，支持在两端高效添加/移除元素
use std::collections::{HashMap, VecDeque};

// =============================================================================
// 第二部分: 常量定义 (Constants)
// =============================================================================

// `const` 关键字定义编译时常量，其值在编译时确定且不可更改
// 与`static`不同，`const`没有固定的内存地址，会被内联到使用处
// 类型标注是必须的，Rust需要知道常量的确切类型

// `usize` 是无符号整数类型，大小取决于目标平台：
// - 32位系统上是u32 (0 到 4,294,967,295)
// - 64位系统上是u64 (0 到 18,446,744,073,709,551,615)
// usize通常用于索引、长度、计数等（数组索引类型就是usize）
// LATENCY_WINDOW定义了滑动窗口的最大容量，即最多保存100个延迟样本
const LATENCY_WINDOW: usize = 100;

// `f64` 是64位浮点数（双精度浮点数），遵循IEEE 754标准
// 范围约 ±1.8×10^308，精度约15-17位十进制数字
// SIGMA_THRESHOLD是3-sigma算法的阈值系数
// 在统计学中，约99.7%的数据落在平均值±3个标准差范围内
// 因此超过3倍标准差的值被认为是异常值（outlier）
const SIGMA_THRESHOLD: f64 = 3.0;

// MIN_ANOMALY_SAMPLES定义进行异常检测所需的最小样本数
// 样本数太少时，计算出的平均值和标准差不具有统计意义
// 容易产生误报（false positives）或漏报（false negatives）
// 这里设为10，意味着至少需要10个样本才开始异常检测
const MIN_ANOMALY_SAMPLES: usize = 10;

// =============================================================================
// 第三部分: 延迟统计结构体 (LatencyStats)
// =============================================================================

// `#[derive(Debug)]` 是一个属性宏(attribute macro)
// `derive` 告诉编译器自动实现某些trait
// `Debug` trait允许使用 `{:?}` 格式化输出结构体，便于调试和日志记录
// 自动生成的实现会打印结构体的字段名和值
// 类似Python的__repr__或Java的toString()
#[derive(Debug)]

// `struct` 关键字定义结构体(structure)，是Rust中创建自定义复合类型的主要方式
// 结构体将多个相关的值组合在一起，形成有意义的整体
// LatencyStats表示一个设备的延迟统计信息
// 命名约定: PascalCase（每个单词首字母大写）
struct LatencyStats {
    // `samples` 字段存储延迟样本值
    // VecDeque<u64>是带类型的泛型结构：
    // - VecDeque是容器类型（双端队列）
    // - <u64>是类型参数，表示队列中存储u64类型的元素
    // u64是无符号64位整数，范围0到18,446,744,073,709,551,615
    // 适合存储时间戳、延迟值等不会为负的数值
    // 使用VecDeque实现滑动窗口：新样本从后端加入，旧样本从前端移除
    samples: VecDeque<u64>,

    // `mean` 字段存储当前的平均延迟值
    // f64选择原因：
    // 1. 平均值计算涉及除法，可能产生小数
    // 2. 需要高精度的统计计算
    // 3. 方差和标准差计算需要浮点数
    // 平均值公式: mean = sum(samples) / count(samples)
    mean: f64,

    // `variance` 字段存储方差（variance），表示数据的离散程度
    // 方差公式: variance = sum((x - mean)²) / n
    // 其中x是每个样本，n是样本数量
    // 方差越大，说明延迟波动越大，网络/设备越不稳定
    variance: f64,
}

// =============================================================================
// 第四部分: LatencyStats的方法实现
// =============================================================================

// `impl` 关键字为类型实现方法(method)和关联函数(associated function)
// 这是Rust实现面向对象编程的方式（虽然Rust不是OOP语言）
// `impl LatencyStats` 表示为LatencyStats结构体实现方法
// 可以有多个impl块，用于组织不同功能的方法
impl LatencyStats {
    // `fn` 关键字定义函数或方法
    // `new` 是Rust的构造函数命名惯例（类似其他语言的constructor）
    // 这是一个关联函数(associated function)，不是方法，因为它没有self参数
    // 调用方式: LatencyStats::new()（使用::而不是.）

    // `-> Self` 指定返回类型为Self
    // Self是impl块所属类型的别名，这里等同于LatencyStats
    // 使用Self的好处：如果类型名改变，impl块内无需修改
    fn new() -> Self {
        // 创建结构体实例的语法
        // Self { field1: value1, field2: value2, ... }
        // 字段初始化顺序不重要，但通常按定义顺序书写
        Self {
            // VecDeque::with_capacity(capacity)创建具有指定初始容量的双端队列
            // 预分配容量是性能优化：避免频繁的内存重新分配和复制
            // 当知道大致容量时，预先分配可以减少运行时的堆分配次数
            // 这里容量设为LATENCY_WINDOW(100)，因为我们最多存储100个样本
            samples: VecDeque::with_capacity(LATENCY_WINDOW),

            // `0.0` 是f64类型的浮点数字面量
            // 注意浮点数必须有小数点，0和0.0是不同的类型
            // 初始化平均值为0，表示还没有样本时的状态
            mean: 0.0,

            // 同样初始化方差为0
            variance: 0.0,
        }
        // 表达式结尾没有分号，表示这是返回值
        // Rust中，函数最后一个表达式的值就是返回值（无需显式return）
    }

    // -------------------------------------------------------------------------
    // 添加样本方法 - 实现滑动窗口逻辑
    // -------------------------------------------------------------------------

    // `&mut self` 是方法的第一个参数，表示对实例的可变引用
    // & 表示引用（借用），不获取所有权
    // mut 表示可变，允许修改实例的字段
    // self 是实例本身（类似Python的self，Java的this）
    // 如果不加&，会获取所有权（消耗self），实例在函数结束后不可用

    // `latency_us` 是参数名，`_us`后缀表示单位是微秒(microseconds)
    // 这是一种命名惯例，帮助程序员理解数值的单位
    // u64类型确保可以存储很大的时间值（约584,554年）
    fn add_sample(&mut self, latency_us: u64) {
        // `if` 是条件控制语句，根据条件执行不同代码
        // 条件必须是bool类型，Rust不会隐式转换（不像C/JavaScript）
        // self.samples.len()返回当前队列中的元素数量，类型是usize
        // >= 是比较运算符，表示"大于或等于"
        // 检查逻辑：如果当前样本数已经达到窗口上限，需要先移除最旧的样本
        if self.samples.len() >= LATENCY_WINDOW {
            // `pop_front()` 是VecDeque的方法
            // 从队列前端移除一个元素并返回它（Option<T>）
            // 滑动窗口的核心逻辑：窗口满了，移除最旧的数据（FIFO）
            // 如果队列为空，返回None；但这里的if条件确保了不会为空
            // 我们不使用返回值，只是单纯丢弃旧样本
            self.samples.pop_front();
        }

        // `push_back()` 在队列末尾添加新元素
        // 新样本总是添加到后端，保持时间顺序
        // 这是O(1)操作，VecDeque保证两端操作都是常数时间
        self.samples.push_back(latency_us);

        // `self.recalculate()` 调用实例的另一个方法
        // 添加新样本后，需要重新计算平均值和方差
        // 注意这里不需要self，因为我们在impl块内，可以直接访问
        self.recalculate();
    }

    // -------------------------------------------------------------------------
    // 重新计算统计值方法
    // -------------------------------------------------------------------------

    // `&mut self` 再次表示可变引用
    // 这个方法会修改mean和variance字段
    fn recalculate(&mut self) {
        // `is_empty()` 是VecDeque的方法，检查队列是否为空
        // 返回bool类型，true表示没有元素
        // 空检查很重要：避免后续计算中的除以零错误
        if self.samples.is_empty() {
            // 如果没有样本，将统计值设为0
            self.mean = 0.0;
            self.variance = 0.0;

            // `return` 关键字提前结束函数执行
            // 这里不需要返回具体值，因为返回类型是()
            // () 是单元类型(unit type)，表示"没有值"，类似void
            return;
        }

        // `let` 关键字绑定变量名到值
        // Rust中变量默认是不可变的（immutable）
        // 如果需要可变，必须显式写 `let mut`
        // 这是Rust安全哲学的核心：默认安全，显式选择风险

        // `as` 关键字用于类型转换（casting）
        // self.samples.len()返回usize，但我们需要f64进行浮点运算
        // as将整数转换为浮点数
        // 注意：as转换可能丢失精度（大整数转f64），但这里样本数不会太大
        // n存储为浮点数，用于后续的除法运算
        let n = self.samples.len() as f64;

        // 计算平均值
        // self.samples.iter()创建迭代器(iterator)，遍历队列中的每个元素
        // 迭代器是Rust的核心抽象，提供惰性(lazy)遍历，不分配新内存
        // sum::<u64>()对迭代器元素求和
        // ::<u64>是显式指定类型参数（turbofish语法）
        // 如果不指定，编译器可能无法推断（因为迭代器可以求和成不同类型）
        // `as f64` 将u64的和转换为f64，以便与n进行浮点除法
        // 公式: mean = sum / count
        self.mean = self.samples.iter().sum::<u64>() as f64 / n;

        // 计算方差
        // 使用迭代器方法链（iterator chain）进行函数式编程
        // 方法链是连续的.操作，每个方法返回新的迭代器，延迟执行
        let sum_sq: f64 = self
            .samples // 获取samples队列的引用
            .iter() // 创建迭代器，产生& u64（对元素的引用）
            // `map` 对迭代器的每个元素进行转换，返回新的迭代器
            // `|&x|` 是闭包(closure)的参数语法，类似于匿名函数/ lambda
            // |...| 定义闭包参数，&x表示解引用（从& u64获取u64值）
            // x就是每个样本值（延迟微秒数）
            .map(|&x| {
                // 计算当前样本与平均值的差
                // x as f64 将u64转为f64
                // 必须转换，因为self.mean是f64，混合类型不能相减
                let diff = x as f64 - self.mean;

                // 返回差的平方（diff * diff）
                // 平方确保正负差都被同等对待（方差定义）
                diff * diff
            }) // map返回新的迭代器，元素类型是f64
            .sum(); // 对所有平方差求和，返回f64

        // 方差 = 平方差之和 / 样本数量
        // 这是总体方差(population variance)，不是样本方差
        // 样本方差会除以n-1（贝塞尔校正），这里不需要
        self.variance = sum_sq / n;
    }

    // -------------------------------------------------------------------------
    // 计算标准差方法
    // -------------------------------------------------------------------------

    // `&self` 表示不可变引用，只读访问，不修改实例
    // 适合 getter 方法或纯计算不修改状态的方法
    // `-> f64` 指定返回值类型为f64
    fn std_dev(&self) -> f64 {
        // `sqrt()` 是f64的方法，计算平方根（square root）
        // 标准差(standard deviation)是方差的平方根
        // σ = √variance
        // 标准差与原始数据同单位（这里是微秒），比方差更直观
        self.variance.sqrt()
    }

    // -------------------------------------------------------------------------
    // 计算异常阈值方法
    // -------------------------------------------------------------------------

    // `Option<f64>` 是泛型枚举，表示"可能有值，也可能没有"
    // Option是Rust处理可空值的类型安全方式，替代其他语言的null
    // 定义:
    //   enum Option<T> {
    //       Some(T),  // 有值
    //       None,     // 无值
    //   }
    // 使用Option强制程序员处理"无值"情况，避免空指针异常
    fn anomaly_threshold(&self) -> Option<f64> {
        // 检查样本数量是否达到最小要求
        // 如果样本太少，统计数据不可靠，不应该进行异常检测
        // < 是小于比较运算符
        if self.samples.len() < MIN_ANOMALY_SAMPLES {
            // `None` 表示没有阈值（样本不足）
            // 调用者必须处理这种情况
            return None;
        }

        // `Some` 是Option::Some的简写，包装一个值表示"有值"
        // 3-sigma规则：异常阈值 = 平均值 + 3 × 标准差
        // 约99.7%的正态分布数据落在此范围内
        // 超过此值的点只有约0.3%的概率，认为是异常
        // SIGMA_THRESHOLD是常量3.0，self.std_dev()计算标准差
        Some(self.mean + SIGMA_THRESHOLD * self.std_dev())
    }

    // -------------------------------------------------------------------------
    // 判断是否为异常的方法
    // -------------------------------------------------------------------------

    // `latency_us` 是要检测的延迟值
    // `-> bool` 返回布尔值，true表示异常，false表示正常
    fn is_anomaly(&self, latency_us: u64) -> bool {
        // `match` 是Rust的模式匹配表达式，是其最强大的特性之一
        // 类似于其他语言的switch，但更强大和类型安全
        // match会穷举所有可能的情况，确保没有遗漏
        // 类似于if/else if/else，但更清晰
        match self.anomaly_threshold() {
            // 模式1: Some(threshold)匹配成功，表示有阈值
            // threshold是绑定的变量名，会被赋值为Some内部的f64值
            // 类似于if let Some(threshold) = ...
            Some(threshold) => {
                // 比较延迟值是否超过阈值
                // `as f64` 类型转换，因为threshold是f64
                // `>` 是大于运算符
                // 如果latency_us > threshold，返回true（是异常）
                latency_us as f64 > threshold
            }
            // 模式2: None匹配，表示样本不足无法计算阈值
            // 此时保守起见，认为不是异常（false positive比false negative好）
            None => false,
        }
        // match表达式的返回值就是整个表达式的值
        // 这里返回bool值
    }
}

// =============================================================================
// 第五部分: 延迟监控Worker结构体
// =============================================================================

// `#[derive(WorkerOpts)]` 是过程宏(procedural macro)
// 为结构体自动实现WorkerOpts trait
// 这是roboplc框架的特性，用于配置worker的行为参数
// 过程宏在编译时展开，生成额外的代码
#[derive(WorkerOpts)]
// `#[worker_opts(...)]` 属性宏，指定具体的worker选项
// name = "latency_monitor": 设置worker的名称，用于日志、调试、监控
// 名称应该是唯一的，便于识别
#[worker_opts(name = "latency_monitor")]

// `pub` 关键字设置可见性(visibility)为公共
// 默认是私有的(private)，只在当前模块可见
// pub允许其他模块访问这个结构体
// LatencyMonitor是延迟监控worker的主结构体，需要被main.rs使用
pub struct LatencyMonitor {
    // `latency_stats` 存储每个设备的延迟统计
    // HashMap<K, V>是泛型哈希映射:
    // - K是键类型：String，设备ID
    // - V是值类型：LatencyStats，该设备的统计信息
    // HashMap使用哈希表实现，平均O(1)时间复杂度的查找、插入、删除
    // 适合需要根据键快速查找值的场景
    latency_stats: HashMap<String, LatencyStats>,
}

// =============================================================================
// 第六部分: LatencyMonitor的方法实现
// =============================================================================

impl LatencyMonitor {
    // 公共关联函数，创建新的LatencyMonitor实例
    // `pub` 使其可以在其他模块调用
    pub fn new() -> Self {
        Self {
            // HashMap::new()创建空的哈希映射
            // 初始容量为0，第一次插入时会分配内存
            // 也可以使用with_capacity(capacity)预分配，但这里设备数不确定
            latency_stats: HashMap::new(),
        }
    }

    // -------------------------------------------------------------------------
    // 处理延迟样本的核心方法
    // -------------------------------------------------------------------------

    // 处理延迟样本，进行异常检测，如有异常则返回事件
    // `&mut self`: 可变引用，需要修改latency_stats
    // `device_id: &str`: 设备ID，&str是字符串切片（借用String的一部分）
    //   - &表示借用，不获取所有权
    //   - str是字符串类型，但大小不固定，所以总是用引用
    //   - 比String更轻量，不分配堆内存
    // `latency_us: u64`: 延迟值（微秒）
    // `timestamp_ms: u64`: 时间戳（毫秒）
    // `-> Option<DeviceEvent>`: 可能返回事件，也可能不返回
    fn process_latency_sample(
        &mut self,
        device_id: &str,
        latency_us: u64,
        timestamp_ms: u64,
    ) -> Option<DeviceEvent> {
        // `entry` 是HashMap的高效API，用于"如果不存在则插入"的场景
        // 它返回一个Entry枚举，表示键的入口状态
        // Entry避免两次查找（先检查contains_key，再insert）
        let stats = self
            .latency_stats
            // device_id是&str，但HashMap的键是String，需要转换
            // to_string() 从&str创建新的String（分配堆内存，复制内容）
            // 这会获取所有权，因为String是拥有类型
            .entry(device_id.to_string())
            // `or_insert_with` 如果键不存在，则插入闭包返回的值
            // 参数是一个闭包：|| { ... } 或简写为函数名
            // LatencyStats::new 作为闭包，在需要时调用
            // 如果键已存在，返回现有值的可变引用；如果不存在，插入新值并返回引用
            .or_insert_with(LatencyStats::new);

        // 在添加新样本之前，先获取当前阈值
        // 这是重要的设计决策：用历史数据判断新样本
        // 如果用包含新样本的数据判断，可能会产生偏差
        let anomaly_threshold = stats.anomaly_threshold();

        // 判断新样本是否为异常
        // 基于历史统计数据（窗口内的样本）
        let is_anomaly = stats.is_anomaly(latency_us);

        // 将新样本添加到统计数据中
        // 这会更新滑动窗口和平均值、方差
        stats.add_sample(latency_us);

        // `!` 是逻辑非运算符，将bool取反
        // if !is_anomaly 表示"如果不是异常"
        if !is_anomaly {
            // 正常情况，不返回事件
            // return None提前结束函数
            return None;
        }

        // 检测到异常，创建DeviceEvent事件
        // `Some` 包装返回值，表示有事件
        Some(DeviceEvent {
            // 再次将&str转为String存储到事件结构体中
            device_id: device_id.to_string(),

            // DeviceEventType::Error 是枚举变体
            // :: 用于访问枚举、模块、关联函数的成员
            event_type: DeviceEventType::Error,

            // 事件发生的时间戳
            timestamp_ms,

            // `format!` 是宏(macro)，类似println!但返回String
            // 格式化字符串语法:
            // {} 是默认格式化占位符
            // {:.2} 表示保留2位小数的浮点格式化
            // σ 是希腊字母sigma，表示标准差
            details: format!(
                "Latency anomaly: {}us exceeds {:.2}us (mean {:.2}us, σ {:.2}us)",
                latency_us,
                // `unwrap_or(0.0)` 是Option的方法
                // 如果是Some(value)，返回value；如果是None，返回默认值0.0
                // 这里anomaly_threshold应该是Some（样本足够后才判断异常）
                // 但Rust要求处理所有情况，所以提供默认值
                anomaly_threshold.unwrap_or(0.0),
                stats.mean,
                stats.std_dev()
            ),
        })
    }
}

// =============================================================================
// 第七部分: Worker trait实现
// =============================================================================

// `impl Trait for Type` 为类型实现trait
// Worker是roboplc框架的核心trait，定义worker的行为
// <Message, Variables> 是trait的泛型参数
// - Message: worker接收的消息类型
// - Variables: 共享状态变量的类型
// trait类似其他语言的接口(interface)，定义契约
impl Worker<Message, Variables> for LatencyMonitor {
    // `run` 是Worker trait要求实现的方法，包含worker的主逻辑
    // 这是worker的入口点，框架会调用此方法启动worker
    // `&mut self`: 可变引用，worker可以修改自身状态
    // `context: &Context<Message, Variables>`: 对上下文的引用
    //   - Context提供访问消息总线、共享变量等功能
    //   - &Context表示借用上下文，不获取所有权
    // `-> WResult`: 返回worker结果，成功或失败
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // `context.hub()` 获取消息总线(Hub)的引用
        // Hub是roboplc框架的消息路由系统，worker通过它发送/接收消息
        // `register` 方法在Hub上注册一个消息客户端
        // 参数1: "latency_monitor" - 客户端名称，用于标识和调试
        // 参数2: event_matches!(...) - 宏，指定订阅的消息模式
        //   - 只接收匹配此模式的消息
        //   - Message::DeviceHeartbeat { .. } 匹配DeviceHeartbeat变体
        //   - { .. } 表示解构模式，不关心具体字段值
        // `?` 是错误传播运算符（try operator）
        // 如果register失败（返回Err），立即从run返回该错误
        // 类似try/catch中的throw，但更简洁
        let client = context.hub().register(
            "latency_monitor",
            event_matches!(Message::DeviceHeartbeat { .. }),
        )?;

        // `for msg in client` 遍历客户端接收的消息
        // client实现了IntoIterator trait，可以迭代
        // 这是一个无限循环，持续监听消息，直到worker停止
        // 每条消息会触发一次循环体执行
        for msg in client {
            // `if let` 是模式匹配的条件语句
            // 尝试将msg解构为DeviceHeartbeat，如果匹配成功执行块
            // 比match更简洁，当只关心一种模式时使用
            // 解构语法提取结构体字段：
            // device_id: 设备标识符
            // timestamp_ms: 时间戳（毫秒）
            // latency_us: 延迟（微秒）
            if let Message::DeviceHeartbeat {
                device_id,
                timestamp_ms,
                latency_us,
            } = msg
            {
                // `parse::<u32>()` 将字符串解析为u32类型
                // ::<u32>是类型参数，告诉编译器目标类型
                // parse返回Result<T, E>，可能成功或失败
                // `unwrap_or(0)` 如果是Err（解析失败），使用默认值0
                // 这里假设device_id是数字字符串，如"42"
                let device_id_num = device_id.parse::<u32>().unwrap_or(0);

                // 创建LatencySample结构体实例
                // 用于记录到共享变量中，供其他组件查询
                // struct初始化语法: TypeName { field: value }
                let sample = LatencySample {
                    device_id: device_id_num,
                    latency_us,
                    timestamp_ms,
                };

                // `context.variables()` 获取共享变量的引用
                // 这是线程安全的共享状态（通过Arc<Mutex<T>>或类似机制）
                // `latency_samples` 是延迟样本队列（可能是循环缓冲区）
                // `force_push` 方法将样本推入队列（如果满了可能覆盖旧数据）
                context.variables().latency_samples.force_push(sample);

                // 调用process_latency_sample进行异常检测
                // `if let Some(event)` 只在返回Some(event)时执行
                // 如果返回None（正常），跳过
                if let Some(event) =
                    self.process_latency_sample(&device_id, latency_us, timestamp_ms)
                {
                    // 将异常事件推入共享事件队列
                    // 其他worker（如日志、告警）可以消费这些事件
                    context.variables().device_events.force_push(event);
                }
            }
            // 如果不是DeviceHeartbeat消息，if let不匹配，自然跳过
        }

        // `Ok(())` 表示成功
        // Ok是Result::Ok，包装成功的值
        // () 是单元类型，表示"无返回值"
        // WResult 是Result<(), WorkerError>的别名
        Ok(())
    }
}

// =============================================================================
// 第八部分: 测试模块
// =============================================================================

// `#[cfg(test)]` 是条件编译属性
// 表示这部分代码只在运行测试时编译（`cargo test`）
// 不会包含在正式发布二进制文件中，减小体积
#[cfg(test)]

// `mod` 关键字定义模块(module)
// Rust的代码组织单元，类似文件/命名空间
// tests是模块名，通常测试放在tests子模块中
mod tests {
    // `use super::*` 导入父模块的所有公共项
    // super表示父模块（文件级别的模块）
    // * 是通配符，导入所有
    // 这样测试可以访问LatencyStats、LatencyMonitor、常量等
    use super::*;

    // -------------------------------------------------------------------------
    // 测试1: 滑动窗口维护
    // -------------------------------------------------------------------------

    // `#[test]` 属性标记这是一个测试函数
    // cargo test 会自动发现并运行所有标记为test的函数
    #[test]
    // 函数名应该描述测试内容，使用snake_case
    // 好的测试名: 被测功能_条件_预期结果
    fn latency_stats_maintains_rolling_window() {
        // `mut` 关键字使变量可变
        // 默认不可变是Rust的安全特性，防止意外修改
        let mut stats = LatencyStats::new();

        // `for` 循环遍历范围
        // `..` 是范围(range)语法：0..10 表示 0到9（不包括10）
        // `..=` 包含结束：0..=10 表示 0到10
        // 这里添加比窗口大小多10个样本
        for sample in 0..(LATENCY_WINDOW as u64 + 10) {
            // 将sample（u64）作为延迟值添加
            stats.add_sample(sample);
        }

        // `assert_eq!` 是断言宏，检查两个值相等
        // 如果不相等，测试失败，打印错误信息
        // 检查样本数量等于窗口大小（旧样本已被移除）
        assert_eq!(stats.samples.len(), LATENCY_WINDOW);

        // `front()` 获取队列前端（最旧元素）的引用
        // 返回Option<&T>，因为队列可能为空
        // `copied()` 将Option<&u64>转为Option<u64>
        // 复制值，解引用
        // 验证最旧的样本是10（0-9已被移除）
        assert_eq!(stats.samples.front().copied(), Some(10));
    }

    // -------------------------------------------------------------------------
    // 测试2: 最小样本数要求
    // -------------------------------------------------------------------------

    #[test]
    fn latency_stats_requires_minimum_samples_for_anomaly_detection() {
        let mut stats = LatencyStats::new();

        // 添加9个样本，少于MIN_ANOMALY_SAMPLES(10)
        // `_` 是忽略模式，表示不关心循环变量的值
        for _ in 0..9 {
            stats.add_sample(1000);
        }

        // `assert!` 宏断言表达式为true
        // `!stats.is_anomaly(...)` 取反，应该返回true（不是异常）
        // 即使10000远高于样本值，样本不足也不应判定为异常
        assert!(!stats.is_anomaly(10_000));
    }

    // -------------------------------------------------------------------------
    // 测试3: 3-sigma异常检测
    // -------------------------------------------------------------------------

    #[test]
    fn latency_stats_detects_three_sigma_outlier() {
        let mut stats = LatencyStats::new();

        // 添加50个延迟为1000的样本
        // 平均值约1000，标准差接近0（因为所有值相同）
        for _ in 0..50 {
            stats.add_sample(1000);
        }

        // 添加一个延迟为2000的样本（在add_sample之后检测）
        // 但这里是在添加前检测，所以基于50个1000的样本
        // 2000远超平均值+3*标准差（约1000+3*0=1000）
        // 所以应该是异常
        // 注意：实际标准差不是0，因为计算的是总体方差
        // 但由于样本非常集中，2000仍会被判定为异常
        assert!(stats.is_anomaly(2000));
    }

    // -------------------------------------------------------------------------
    // 测试4: 监控器产生事件
    // -------------------------------------------------------------------------

    #[test]
    fn monitor_emits_event_on_detected_anomaly() {
        // 创建新的监控器
        let mut monitor = LatencyMonitor::new();
        let device_id = "42";

        // 先添加20个正常样本建立基线
        // `_` 表示循环变量不使用
        for _ in 0..20 {
            // process_latency_sample返回Option<DeviceEvent>
            // `is_none()` 检查是否为None（正常情况无事件）
            let event = monitor.process_latency_sample(device_id, 1000, 1);
            assert!(event.is_none());
        }

        // 添加一个异常高的延迟值
        let event = monitor.process_latency_sample(device_id, 5000, 2);
        // 现在应该产生异常事件
        assert!(event.is_some());

        // `unwrap()` 解包Option，获取Some内部的值
        // 如果是None会panic（测试失败），但前面已检查is_some
        let event = event.unwrap();

        // 验证事件中的设备ID正确
        assert_eq!(event.device_id, device_id);

        // `matches!` 宏检查模式是否匹配
        // 验证事件类型是Error
        assert!(matches!(event.event_type, DeviceEventType::Error));

        // `contains` 是String的方法，检查是否包含子串
        // 验证事件详情包含预期的错误信息
        assert!(event.details.contains("Latency anomaly"));
    }
}
