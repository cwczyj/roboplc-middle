// 导入模块说明：
// use 关键字用于引入其他模块中的类型、函数或 trait
// crate:: 表示从当前 crate 的根目录开始查找模块
use crate::{config::Config, Message, Variables}; // 从 crate 根导入 Config 配置结构体、Message 消息枚举和 Variables 共享变量
use notify::{RecursiveMode, Watcher}; // 从 notify crate 导入文件监控相关的类型：RecursiveMode（递归模式）和 Watcher（监控器 trait）
use roboplc::controller::prelude::*; // 从 roboplc crate 导入控制器相关的预导入模块，包含 Worker、Context 等核心 trait 和类型
use serde_json::Value as JsonValue; // 从 serde_json 导入 Value 类型，并重命名为 JsonValue，用于处理 JSON 数据
use std::collections::BTreeSet; // 从标准库导入 BTreeSet，这是一个基于 B 树的有序集合，按键的自然顺序排序
use std::path::Path; // 从标准库导入 Path 类型，用于表示文件系统路径（跨平台的抽象）

// derive 属性宏：
// #[derive(...)] 自动为结构体实现指定的 trait
// WorkerOpts 是 roboplc 框架提供的派生宏，会为 ConfigLoader 生成 Worker 配置相关的代码
#[derive(WorkerOpts)]
// worker_opts 属性宏配置：
// name = "config_loader"：设置 worker 的名称为 "config_loader"，用于日志和监控识别
// blocking = true：标记这是一个阻塞型 worker，意味着它会长时间运行并占用线程
#[worker_opts(name = "config_loader", blocking = true)]
// pub 关键字：声明这个结构体是公有的，可以被其他模块访问
// struct 关键字：定义一个结构体（复合数据类型，可以包含多个不同类型的字段）
pub struct ConfigLoader {
    // String 是标准库提供的 UTF-8 编码的可变长字符串类型
    // 存储配置文件的路径字符串
    config_path: String,
    // Config 是自定义的配置结构体（来自 crate::config）
    // 存储当前加载的配置内容
    current_config: Config,
}

// impl 关键字：为类型实现方法（类似于其他语言的类方法）
// impl ConfigLoader 表示为 ConfigLoader 结构体实现方法
impl ConfigLoader {
    // pub 关键字：这个方法是对外公开的
    // fn 关键字：定义一个函数
    // new 是 Rust 中约定俗成的构造函数名称
    // 参数说明：
    //   config_path: String - 配置文件路径，String 类型表示所有权转移给函数
    //   config: Config - 初始配置对象
    // -> Self 是返回类型，Self 指代当前类型（ConfigLoader）
    pub fn new(config_path: String, config: Config) -> Self {
        // Self { ... } 是结构体实例化语法
        // 创建 ConfigLoader 的新实例，将传入的参数赋值给对应字段
        Self {
            config_path,            // 字段初始化简写：等价于 config_path: config_path
            current_config: config, // 显示指定字段名和值的映射
        }
    }

    // 私有方法（没有 pub），只能在本模块内调用
    // &mut self 表示可变借用，允许方法修改结构体的字段
    // 返回类型：Result 是 Rust 的错误处理枚举
    //   Option<(String, Vec<String>)> 表示可能有返回值，也可能没有
    //   Box<dyn std::error::Error> 是动态错误类型，可以包装任何实现了 Error trait 的错误
    fn reload_config(
        &mut self,
    ) -> Result<Option<(String, Vec<String>)>, Box<dyn std::error::Error>> {
        // let 关键字：绑定变量，创建不可变绑定（默认）
        // 调用 Config::from_file 从磁盘加载配置文件
        // &self.config_path 是对 config_path 字段的借用
        // ? 运算符：如果结果是 Err，立即返回错误；如果是 Ok，解包值
        let new_config = Config::from_file(&self.config_path)?;

        // serde_json::to_value 将结构体序列化为 serde_json::Value 类型
        // &self.current_config 借用当前配置
        // ? 传播错误
        let old_value = serde_json::to_value(&self.current_config)?;
        // 同样地将新配置序列化为 JSON Value
        let new_value = serde_json::to_value(&new_config)?;
        // mut 关键字：声明可变变量，之后可以修改
        // Vec<String> 是动态数组（向量），存储字符串
        // 用于存储检测到变更的配置路径
        let mut changed_paths = Vec::new();
        // Self:: 调用关联函数（静态方法）
        // "" 是空字符串，作为路径前缀的根
        // &mut changed_paths 可变借用 changed_paths，允许函数向其中添加元素
        Self::collect_diff_paths("", &old_value, &new_value, &mut changed_paths);

        // .is_empty() 是 Vec 的方法，检查向量是否为空
        // if 语句：条件判断
        if changed_paths.is_empty() {
            // return 关键字：提前返回函数
            // Ok(None) 表示操作成功，但没有变更内容
            return Ok(None);
        }

        // serde_json::to_string 将配置序列化为 JSON 字符串
        let config_json = serde_json::to_string(&new_config)?;
        // 更新当前配置字段，将新配置赋值给 self.current_config
        // 这里发生了所有权的移动：new_config 被移动到 self.current_config
        self.current_config = new_config;

        // Ok(Some(...)) 表示操作成功，且有返回值
        // (config_json, changed_paths) 是元组（tuple），包含两个元素
        Ok(Some((config_json, changed_paths)))
    }

    // 递归函数：比较两个 JSON Value 的差异
    // prefix: &str - 当前路径前缀，&str 是字符串切片（对字符串的引用）
    // old: &JsonValue - 旧的 JSON 值的引用
    // new: &JsonValue - 新的 JSON 值的引用
    // out: &mut Vec<String> - 输出的变更路径列表（可变引用）
    fn collect_diff_paths(prefix: &str, old: &JsonValue, new: &JsonValue, out: &mut Vec<String>) {
        // match 表达式：Rust 的模式匹配，类似于 switch 但更强大
        // (old, new) 创建一个元组，同时匹配两个值
        match (old, new) {
            // 模式 1：两个都是 JSON 对象（Object）
            // JsonValue::Object 是 serde_json::Value 的变体，包含一个 Map（键值对集合）
            (JsonValue::Object(old_map), JsonValue::Object(new_map)) => {
                // 创建 BTreeSet 来存储所有唯一的键
                // BTreeSet 会自动去重并按字母顺序排序
                // &str 是字符串切片类型
                let keys: BTreeSet<&str> = old_map
                    .keys() // 获取旧对象的所有键
                    .map(String::as_str) // map 是迭代器方法，将每个 String 转换为 &str
                    .chain(new_map.keys().map(String::as_str)) // chain 将两个迭代器连接起来
                    .collect(); // collect 将迭代器收集为集合

                // for 循环遍历所有键
                for key in keys {
                    // if-else 表达式，根据 prefix 是否为空决定路径格式
                    // to_string() 将 &str 转换为 String（分配堆内存）
                    // format! 宏：格式化字符串，类似 println! 但返回 String
                    let key_path = if prefix.is_empty() {
                        key.to_string() // 根层级，直接使用键名
                    } else {
                        format!("{prefix}.{key}") // 嵌套层级，使用点号连接
                    };

                    // 嵌套 match：处理键在旧对象和新对象中的存在情况
                    match (old_map.get(key), new_map.get(key)) {
                        // 模式：键在两者中都存在，递归比较值
                        (Some(old_val), Some(new_val)) => {
                            // 递归调用，深入比较嵌套结构
                            Self::collect_diff_paths(&key_path, old_val, new_val, out);
                        }
                        // _ 是通配符，匹配所有其他情况（键只存在于其中一个对象）
                        _ => out.push(key_path), // 将变更路径添加到输出列表
                    }
                }
            }
            // 模式 2：两个都是 JSON 数组
            (JsonValue::Array(old_arr), JsonValue::Array(new_arr)) => {
                // != 是不等于运算符
                // 如果数组内容不同，整个数组视为变更
                if old_arr != new_arr {
                    out.push(prefix.to_string()); // 添加整个数组路径
                }
            }
            // 模式 3：其他所有情况（标量类型或类型不匹配）
            _ => {
                // 如果值不相同，记录变更
                if old != new {
                    out.push(prefix.to_string());
                }
            }
        }
    }
}

// 为 ConfigLoader 实现 Worker trait
// Worker<Message, Variables> 是泛型 trait
//   Message 是消息类型，用于 worker 间通信
//   Variables 是共享变量类型
impl Worker<Message, Variables> for ConfigLoader {
    // run 方法是 Worker trait 的要求，包含 worker 的主逻辑
    // &mut self 允许修改 self
    // context: &Context<Message, Variables> 是 roboplc 提供的上下文引用
    // -> WResult 是返回类型，通常是 Result<(), Box<dyn Error>> 的别名
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // std::sync::mpsc::channel 创建多生产者单消费者通道
        // 用于在文件监控器和当前线程间传递事件
        // tx: 发送端（Transmitter）
        // rx: 接收端（Receiver）
        let (tx, rx) = std::sync::mpsc::channel();
        // notify::recommended_watcher 创建系统推荐的最佳文件监控器
        // 传入发送端，当文件变更时监控器会通过通道发送事件
        // ? 传播可能的错误
        let mut watcher = notify::recommended_watcher(tx)?;
        // watcher.watch 开始监控指定路径
        // Path::new 从字符串创建 Path 引用
        // &self.config_path 借用配置文件路径
        // RecursiveMode::NonRecursive 表示只监控指定文件，不监控子目录
        watcher.watch(Path::new(&self.config_path), RecursiveMode::NonRecursive)?;

        // while 循环：只要上下文处于在线状态就持续运行
        // context.is_online() 检查 worker 是否应该继续运行
        while context.is_online() {
            // rx.try_recv() 非阻塞地尝试接收通道消息
            // 返回 Result<Event, TryRecvError>
            match rx.try_recv() {
                // Ok(_event) 表示成功接收到文件变更事件
                // _event 中的下划线前缀表示虽然绑定但暂时不使用该变量
                Ok(_event) => {
                    // std::thread::sleep 让当前线程暂停执行
                    // std::time::Duration::from_millis(100) 创建 100 毫秒的持续时间
                    // 延迟是为了等待文件写入完成（避免读到不完整的文件）
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    // 调用 reload_config 重新加载配置
                    match self.reload_config() {
                        // Ok(Some(...)) 表示配置有变更
                        Ok(Some((config_json, changed_paths))) => {
                            // context.hub() 获取消息总线（Hub）
                            // .send(...) 发送消息给所有订阅者
                            context.hub().send(Message::ConfigUpdate {
                                config: config_json, // 新配置的 JSON 字符串
                            });
                            // tracing::info! 宏：记录信息级别日志
                            // 使用结构化日志格式，包含字段名和值
                            tracing::info!(
                                config_path = %self.config_path,  // % 表示使用 Display trait 格式化
                                changed_fields = ?changed_paths,  // ? 表示使用 Debug trait 格式化
                                "Config reloaded"  // 日志消息
                            );
                        }
                        // Ok(None) 表示文件有事件但内容没有实际变更
                        Ok(None) => {
                            // tracing::debug! 记录调试级别日志
                            tracing::debug!(
                                config_path = %self.config_path,
                                "Config file event received but no content change detected"
                            );
                        }
                        // Err(e) 表示重新加载过程中出错
                        Err(e) => {
                            // tracing::error! 记录错误级别日志
                            tracing::error!(
                                config_path = %self.config_path,
                                error = %e,  // %e 格式化错误信息
                                "Failed to reload config"
                            );
                        }
                    }
                }
                // Err(_) 表示没有接收到消息（通道为空）
                Err(_) => {
                    // 短暂休眠后再次检查，避免 CPU 空转
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        // Ok(()) 表示 worker 正常结束
        Ok(())
    }
}

// #[cfg(test)] 属性：条件编译，只在运行测试时编译下面的代码
#[cfg(test)]
// mod 关键字：定义一个模块
// tests 是测试模块的常规名称
mod tests {
    // use super::* 导入父模块的所有公有内容
    use super::*;
    // tempfile crate 提供临时文件功能，测试结束后自动清理
    use tempfile::NamedTempFile;

    // fn 定义函数
    // write_config 是辅助函数，用于在测试中写入配置文件
    // 参数说明：
    //   path: &Path - 文件路径的引用
    //   rpc_port: u16 - RPC 端口（16位无符号整数）
    //   http_port: u16 - HTTP 端口
    //   level: &str - 日志级别的字符串切片
    fn write_config(path: &Path, rpc_port: u16, http_port: u16, level: &str) {
        // format! 宏：类似 println! 但返回 String
        // r#"..."# 是原始字符串字面量，# 之间的内容不处理转义
        let content = format!(
            r#"[server]
rpc_port = {rpc_port}
http_port = {http_port}

[logging]
level = "{level}"
file = "/tmp/roboplc.log"
daily_rotation = true
"#
        );
        // std::fs::write 将内容写入文件
        // .unwrap() 解包 Result，如果是 Err 会 panic（测试中这是期望的行为）
        std::fs::write(path, content).unwrap();
    }

    // #[test] 属性标记这是一个测试函数
    #[test]
    // 测试函数名描述测试目的：验证配置差异检测和更新功能
    fn reload_config_detects_diff_and_updates_current_config() {
        // NamedTempFile::new() 创建一个临时文件
        // unwrap() 解包 Result
        let file = NamedTempFile::new().unwrap();
        // file.path() 获取临时文件的路径引用
        let path = file.path();

        // 写入初始配置：RPC 端口 8080，HTTP 端口 8081，日志级别 info
        write_config(path, 8080, 8081, "info");
        // 从文件加载配置，验证 Config::from_file 工作正常
        let config = Config::from_file(path).unwrap();
        // 创建 ConfigLoader 实例
        // path.display().to_string() 将 Path 转换为可显示的字符串
        let mut loader = ConfigLoader::new(path.display().to_string(), config);

        // 修改配置文件：RPC 端口改为 9090，日志级别改为 debug
        write_config(path, 9090, 8081, "debug");

        // 调用 reload_config 检测变更
        // unwrap() 确保没有错误发生
        let diff = loader.reload_config().unwrap();
        // assert! 宏：断言条件为真，否则测试失败
        // is_some() 检查 Option 是 Some 而不是 None
        assert!(diff.is_some());
        // unwrap() 解包 Option，获取元组内容
        let (_config_json, changed_paths) = diff.unwrap();
        // assert! 配合 any() 检查变更路径中是否包含指定字段
        // .iter() 创建迭代器
        // .any(|path| path == "...") 检查是否有任意元素满足条件
        assert!(changed_paths.iter().any(|path| path == "server.rpc_port"));
        assert!(changed_paths.iter().any(|path| path == "logging.level"));
        // assert_eq! 宏：断言两个值相等
        // 验证 loader 内部状态已更新为新配置值
        assert_eq!(loader.current_config.server.rpc_port, 9090);
        assert_eq!(loader.current_config.logging.level, "debug");
    }
}
