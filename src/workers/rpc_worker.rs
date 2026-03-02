// =============================================================================
// RPC Worker - JSON-RPC服务器实现
// =============================================================================
// 这个模块实现了一个TCP上的JSON-RPC 2.0服务器
// 用于接收外部客户端的请求，并将其转发给设备管理器处理

// ---------------------------------------------------------------------------
// 第一部分：导入模块（use语句）
// ---------------------------------------------------------------------------
// use语句用于导入其他模块中定义的类型、函数和trait
// Rust的模块系统类似于其他语言的包/库导入

use crate::config::Config;
// ^ use: 导入关键字
//   crate:: : 表示从当前crate（项目）的根开始查找
//   config: 模块名，对应src/config.rs文件
//   Config: 从config模块导入的结构体，用于存储服务器配置（如端口号）

use crate::messages::Message;
//   Message: 从messages模块导入的枚举，定义了worker之间传递的消息类型
//   这是RoboPLC框架中worker间通信的核心类型

use crate::messages::Operation;
//   Operation: 枚举类型，表示可以对设备执行的操作（如GetStatus、SetRegister等）

use crate::Variables;
//   Variables: 从crate根导入的共享状态类型，用于在workers之间共享数据
//   在RoboPLC中，Variables是线程安全的共享存储

use roboplc::controller::prelude::*;
//   roboplc: 外部crate，提供实时PLC控制功能
//   controller: roboplc的子模块，包含Worker、Context等核心trait
//   prelude: 预导入模块，通常包含最常用的类型
//   *: 通配符导入，导入prelude中所有公共类型

use roboplc_rpc::{dataformat::Json, server::RpcServer, server::RpcServerHandler, RpcResult};
//   roboplc_rpc: 外部crate，提供JSON-RPC服务器功能
//   dataformat::Json: JSON数据格式处理器
//   server::RpcServer: JSON-RPC服务器结构体
//   server::RpcServerHandler: trait，定义如何处理RPC请求
//   RpcResult: 类型别名，表示RPC处理函数的返回类型

use serde::{Deserialize, Serialize};
//   serde: 外部crate，Rust中最流行的序列化/反序列化框架
//   Serialize: trait，定义如何将Rust类型转换为JSON等格式
//   Deserialize: trait，定义如何从JSON等格式解析为Rust类型

use serde_json::Value as JsonValue;
//   serde_json: serde的JSON实现
//   Value: 表示任意JSON值的枚举（可以是对象、数组、字符串、数字等）
//   as JsonValue: 类型别名，简化后续代码中的书写

use std::collections::HashMap;
//   std: Rust标准库
//   collections: 集合模块，包含各种数据结构
//   HashMap: 哈希映射（键值对存储），类似Python的dict或Java的HashMap

use std::io::{Read, Write};
//   io: 输入输出模块
//   Read: trait，定义读取字节流的方法（如从TCP流读取数据）
//   Write: trait，定义写入字节流的方法（如向TCP流写入数据）

use std::net::SocketAddr;
//   net: 网络模块
//   SocketAddr: 表示套接字地址（IP地址+端口号），如"127.0.0.1:8080"

use std::sync::atomic::{AtomicU64, Ordering};
//   sync: 同步原语模块，用于多线程编程
//   atomic: 原子操作模块，提供线程安全的整数操作
//   AtomicU64: 64位无符号原子整数，可在线程间安全地递增
//   Ordering: 枚举，指定内存排序规则（如SeqCst表示顺序一致性）

use std::sync::mpsc::{channel, Sender};
//   mpsc: 多生产者单消费者通道模块（Multiple Producer Single Consumer）
//   channel: 函数，创建一个发送者(Sender)和接收者(Receiver)对
//   Sender: 发送端类型，用于向通道发送消息

// ---------------------------------------------------------------------------
// 第二部分：静态变量和关联函数
// ---------------------------------------------------------------------------
// static用于定义具有'static生命周期的全局变量
// 静态变量在程序整个运行期间都存在

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(0);
// ^ static: 定义全局静态变量
//   CORRELATION_COUNTER: 变量名，用于生成唯一的请求关联ID
//   AtomicU64: 类型，线程安全的64位无符号整数
//   = AtomicU64::new(0): 初始化为0
//   这个计数器用于跟踪每个RPC请求，确保请求和响应能正确匹配

fn next_correlation_id() -> u64 {
    // ^ fn: 定义函数的关键字
    //   next_correlation_id: 函数名，意为"下一个关联ID"
    //   () -> u64: 参数为空，返回值为u64类型

    CORRELATION_COUNTER.fetch_add(1, Ordering::SeqCst)
    // ^ 调用AtomicU64的fetch_add方法
    //   fetch_add: 原子地增加数值，并返回增加前的值
    //   1: 每次增加1
    //   Ordering::SeqCst: 顺序一致性内存序， strongest ordering保证
    //   在多线程环境下，这确保了每个调用都得到唯一的、递增的ID
}

// ---------------------------------------------------------------------------
// 第三部分：RPC方法枚举定义
// ---------------------------------------------------------------------------
// 这个枚举定义了服务器支持的所有JSON-RPC方法
// 每个变体对应一个客户端可以调用的RPC方法

#[derive(Serialize, Deserialize)]
// ^ 属性宏，自动为枚举实现Serialize和Deserialize trait
//   这允许枚举实例被转换为JSON，以及从JSON解析
#[serde(
    tag = "m",
    // ^ serde属性：使用外部标签方式序列化
    //   tag = "m": JSON中会有一个"m"字段表示方法名
    content = "p",
    //   content = "p": 参数放在"p"字段中
    rename_all = "lowercase",
    //   rename_all = "lowercase": 所有变体名序列化为小写
    //   例如：GetVersion变成"get_version"
    deny_unknown_fields
    //   deny_unknown_fields: 拒绝未知字段，严格的解析模式
)]
enum RpcMethod<'a> {
    // ^ enum: 定义枚举类型
    //   RpcMethod: 枚举名
    //   <'a>: 生命周期参数，表示其中包含的字符串引用需要至少存活'a时间
    Ping {},
    // ^ Ping: 心跳检测方法，用于检查服务器是否存活
    //   {}: 空参数，表示这个方法不需要参数
    GetVersion {},
    //   GetVersion: 获取服务器版本信息
    GetDeviceList {},
    //   GetDeviceList: 获取所有已配置设备的列表
    GetStatus {
        device_id: &'a str,
        // ^ 带参数的变体
        //   device_id: 参数名
        //   &'a str: 类型为字符串切片引用，生命周期为'a
        //   这是要查询状态的设备ID
    },

    SetRegister {
        device_id: &'a str,
        address: String,
        // ^ String: 拥有的字符串类型，不同于&str
        //   Modbus寄存器地址，如"h100"表示保持寄存器100
        value: u16,
        // ^ u16: 16位无符号整数，要写入寄存器的值
    },

    GetRegister {
        device_id: &'a str,
        address: String,
    },

    MoveTo {
        device_id: &'a str,
        position: String,
        //   position: 机器人手臂的目标位置
    },

    ReadBatch {
        device_id: &'a str,
        addresses: Vec<String>,
        // ^ Vec<String>: 字符串向量（动态数组）
        //   要批量读取的多个寄存器地址
    },

    WriteBatch {
        device_id: &'a str,
        values: Vec<(String, u16)>,
        // ^ Vec<(String, u16)>: 元组向量
        //   (String, u16)是元组类型，包含地址和值
        //   用于批量写入多个寄存器
    },
}

// ---------------------------------------------------------------------------
// 第四部分：RPC响应结果枚举
// ---------------------------------------------------------------------------
// 这个枚举定义了所有可能的RPC响应格式

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
// ^ untagged: 无标签序列化方式
//   serde会根据字段内容自动推断是哪个变体
//   这要求每个变体有独特的字段集合
enum RpcResultType {
    Success {
        success: bool,
        //   最简单的成功响应，只包含布尔值
    },

    Version {
        version: String,
        //   版本响应，包含版本号字符串
    },

    DeviceList {
        devices: Vec<String>,
        //   设备列表响应，包含设备ID字符串数组
    },

    Data {
        data: serde_json::Value,
        //   通用数据响应，data字段可以是任何JSON值
    },

    Status {
        connected: bool,
        //   连接状态
        last_communication_ms: u64,
        //   上次通信时间戳（毫秒）
        error_count: u32,
        //   错误计数
    },

    Error {
        error: String,
        //   错误响应，包含错误描述字符串
    },
}

// ---------------------------------------------------------------------------
// 第五部分：类型别名和请求结构体
// ---------------------------------------------------------------------------

pub type ResponseSender = Sender<(bool, JsonValue, Option<String>)>;
// ^ pub: 公共可见性，其他模块可以使用
//   type: 定义类型别名
//   ResponseSender: 别名，表示响应发送者类型
//   Sender<...>: 通道发送者
//   (bool, JsonValue, Option<String>): 元组类型
//     - bool: 成功标志
//     - JsonValue: 响应数据
//     - Option<String>: 可选的错误信息（Some表示有错误，None表示无错误）

#[derive(Clone)]
// ^ 自动实现Clone trait，允许复制这个结构体
pub struct DeviceControlRequest {
    // ^ struct: 定义结构体
    //   这个结构体封装了一个设备控制请求的所有信息
    pub device_id: String,
    //   目标设备ID
    pub operation: Operation,
    //   要执行的操作
    pub params: JsonValue,
    //   操作参数（JSON格式）
    pub correlation_id: u64,
    //   关联ID，用于匹配请求和响应
    pub respond_to: ResponseSender,
    //   响应发送者，用于向请求发起者返回结果
}

// ---------------------------------------------------------------------------
// 第六部分：RPC处理器结构体
// ---------------------------------------------------------------------------
// RpcHandler实现了实际处理RPC请求的逻辑

struct RpcHandler {
    device_ids: Vec<String>,
    //   所有可用设备的ID列表，用于响应GetDeviceList请求
    device_control_tx: Sender<DeviceControlRequest>,
    //   向设备管理器发送控制请求的通道发送者
    //   使用通道实现线程间通信（mpsc模式）
}

impl RpcHandler {
    // ^ impl: 为类型实现方法
    //   这里为RpcHandler实现关联函数（构造函数）

    pub fn new(device_ids: Vec<String>, device_control_tx: Sender<DeviceControlRequest>) -> Self {
        // ^ -> Self: 返回Self类型（即RpcHandler）
        Self {
            // ^ Self: 指代当前实现类型（RpcHandler）
            device_ids,
            //   字段初始化简写，等价于 device_ids: device_ids
            device_control_tx,
        }
    }
}

// ---------------------------------------------------------------------------
// 第七部分：实现RpcServerHandler trait
// ---------------------------------------------------------------------------
// trait类似于其他语言的接口，定义了一组方法契约
// 这里RpcHandler实现了处理RPC请求的具体逻辑

impl<'a> RpcServerHandler<'a> for RpcHandler {
    // ^ impl<Trait> for Type: 为类型实现trait的语法
    //   <'a>: 生命周期参数

    type Method = RpcMethod<'a>;
    // ^ type: 在trait实现中定义关联类型
    //   Method: RpcServerHandler要求定义请求方法类型
    //   我们指定为RpcMethod<'a>

    type Result = RpcResultType;
    //   Result: 响应结果类型

    type Source = SocketAddr;
    //   Source: 请求来源类型（客户端地址）

    fn handle_call(
        &'a self,
        // ^ &'a self: 生命周期为'a的self引用
        //   允许方法体中使用生命周期为'a的参数
        method: Self::Method,
        //   method: 要处理的RPC方法
        _source: Self::Source,
        //   _source: 客户端地址
        //   下划线前缀表示此参数暂时不使用（但保留以备将来使用）
    ) -> RpcResult<Self::Result> {
        //   返回RpcResult，它是Result<RpcResultType, RpcError>的别名

        match method {
            // ^ match: Rust的模式匹配表达式
            //   根据method的不同变体执行不同逻辑
            RpcMethod::Ping {} => Ok(RpcResultType::Success { success: true }),
            // ^ RpcMethod::Ping: 匹配Ping变体
            //   =>: 箭头，表示匹配成功后的结果
            //   Ok(...): Result的Ok变体，表示成功
            //   返回包含success=true的Success响应
            RpcMethod::GetVersion {} => Ok(RpcResultType::Version {
                version: env!("CARGO_PKG_VERSION").to_string(),
                // ^ env!: 编译时宏，获取环境变量
                //   CARGO_PKG_VERSION: Cargo自动设置的环境变量，表示包版本
                //   .to_string(): 将&str转换为String
            }),

            RpcMethod::GetDeviceList {} => Ok(RpcResultType::DeviceList {
                devices: self.device_ids.clone(),
                //   .clone(): 克隆向量，因为需要返回所有权
            }),

            RpcMethod::GetStatus { device_id } => {
                // ^ 解构模式，从变体中提取device_id字段
                self.send_device_control(device_id, Operation::GetStatus, serde_json::json!({}))
                //   调用辅助方法，发送设备控制请求
                //   serde_json::json!({}): 宏，创建一个空JSON对象
            }

            RpcMethod::SetRegister {
                device_id,
                address,
                value,
            } => {
                // ^ 解构多个字段
                let params = serde_json::json!({ "address": address, "value": value });
                // ^ json!宏创建JSON对象
                //   { "address": address }: JSON对象字面量语法
                self.send_device_control(device_id, Operation::SetRegister, params)
            }

            RpcMethod::GetRegister { device_id, address } => {
                let params = serde_json::json!({ "address": address });
                self.send_device_control(device_id, Operation::GetRegister, params)
            }

            RpcMethod::MoveTo {
                device_id,
                position,
            } => {
                let params = serde_json::json!({ "position": position });
                self.send_device_control(device_id, Operation::MoveTo, params)
            }

            RpcMethod::ReadBatch {
                device_id,
                addresses,
            } => {
                let params = serde_json::json!({ "addresses": addresses });
                self.send_device_control(device_id, Operation::ReadBatch, params)
            }

            RpcMethod::WriteBatch { device_id, values } => {
                let params = serde_json::json!({ "values": values });
                self.send_device_control(device_id, Operation::WriteBatch, params)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 第八部分：RpcHandler辅助方法实现
// ---------------------------------------------------------------------------

impl RpcHandler {
    fn send_device_control(
        &self,
        device_id: &str,
        operation: Operation,
        params: JsonValue,
    ) -> RpcResult<RpcResultType> {
        //   这个辅助方法将设备控制请求发送到设备管理器并等待响应

        let correlation_id = next_correlation_id();
        //   生成唯一关联ID

        let (response_tx, response_rx) = channel();
        // ^ channel(): 创建新的mpsc通道
        //   response_tx: 发送端，用于发送响应
        //   response_rx: 接收端，用于接收响应
        //   这是一个一对一双向通信模式

        let request = DeviceControlRequest {
            device_id: device_id.to_string(),
            //   to_string(): 将&str转换为String（获得所有权）
            operation,
            params,
            correlation_id,
            respond_to: response_tx,
        };

        if let Err(error) = self.device_control_tx.send(request) {
            // ^ if let: 简化的错误处理模式
            //   Err(error): 如果发送失败（通道关闭）
            //   tracing::error!: 日志宏，记录错误
            tracing::error!(%error, "failed to send DeviceControl request");
            //   %error: 使用Display trait格式化错误
            return Ok(RpcResultType::Error {
                error: format!("Internal error: {}", error),
                //   format!: 字符串格式化宏，类似printf
            });
        }

        match response_rx.recv() {
            //   recv(): 阻塞等待接收响应
            Ok((success, data, error)) => {
                // ^ 成功接收到响应
                if success {
                    Ok(RpcResultType::Data { data })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                        // ^ unwrap_or_else: 如果None则使用默认值
                        //   ||: 闭包（匿名函数）语法
                    })
                }
            }
            Err(error) => {
                //   接收失败（发送端被丢弃）
                tracing::error!(%error, "failed to receive DeviceResponse");
                Ok(RpcResultType::Error {
                    error: format!("Response error: {}", error),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 第九部分：RpcWorker结构体定义
// ---------------------------------------------------------------------------
// RpcWorker是实际的Worker实现，它运行TCP服务器并处理连接

#[derive(WorkerOpts)]
// ^ 过程宏，为Worker生成元数据
#[worker_opts(name = "rpc_server", blocking = true)]
// ^ 属性指定Worker选项
//   name = "rpc_server": Worker名称，用于日志和监控
//   blocking = true: 这是一个阻塞式Worker（使用标准线程而非async）
pub struct RpcWorker {
    config: Config,
    //   服务器配置，包含端口号等
}

impl RpcWorker {
    //   为RpcWorker实现构造函数

    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

// ---------------------------------------------------------------------------
// 第十部分：Worker trait实现
// ---------------------------------------------------------------------------
// 这是实际的服务器主循环，处理TCP连接和请求分发

impl Worker<Message, Variables> for RpcWorker {
    // ^ Worker<Message, Variables>: 为Worker trait实现
    //   Message: 消息类型
    //   Variables: 共享状态类型

    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // ^ &mut self: 可变引用，允许修改self
        //   context: Worker上下文，用于检查在线状态、访问hub等
        //   WResult: 返回类型，Result<(), Box<dyn Error>>的别名

        let port = self.config.server.rpc_port;
        //   从配置中获取RPC服务器端口
        let bind_addr = format!("0.0.0.0:{}", port);
        // ^ format!: 字符串格式化宏
        //   0.0.0.0: 绑定到所有网络接口
        //   {}: 占位符，会被port值替换

        let device_ids: Vec<String> = self.config.devices.iter().map(|d| d.id.clone()).collect();
        // ^ iter(): 创建迭代器
        //   map(|d| ...): 对每项应用闭包转换
        //   |d|: 闭包参数语法
        //   d.id.clone(): 克隆每个设备的ID
        //   collect(): 收集迭代器结果为Vec

        let (device_control_tx, device_control_rx) = channel::<DeviceControlRequest>();
        // ^ ::<DeviceControlRequest>: 显式指定通道传输的类型（turbofish语法）

        let handler = RpcHandler::new(device_ids, device_control_tx);
        //   创建RPC处理器实例
        let server = RpcServer::new(handler);
        //   使用处理器创建RPC服务器

        let listener = match std::net::TcpListener::bind(&bind_addr) {
            // ^ match: 错误处理
            //   TcpListener::bind: 绑定到地址，创建TCP监听器
            //   &bind_addr: 字符串引用
            Ok(listener) => listener,
            //   成功绑定，返回监听器
            Err(error) => {
                tracing::error!(%error, "RPC Server Worker failed to bind {}", bind_addr);
                return Ok(());
                //   绑定失败，返回Ok表示Worker正常退出
            }
        };

        if let Err(error) = listener.set_nonblocking(true) {
            // ^ set_nonblocking: 设置为非阻塞模式
            //   这样accept()不会阻塞，允许我们检查Hub消息
            tracing::error!(
                %error,
                "RPC Server Worker failed to set non-blocking mode on {}",
                bind_addr
            );
            return Ok(());
        }

        tracing::info!("RPC Server Worker started on {}", bind_addr);

        let mut pending_requests: HashMap<u64, ResponseSender> = HashMap::new();
        //   存储待处理的请求，键是correlation_id，值是响应发送者

        while context.is_online() {
            // ^ while: 循环，只要Worker在线就一直运行
            //   is_online(): 检查Worker是否应该继续运行

            match listener.accept() {
                //   accept(): 接受新连接（非阻塞模式）
                Ok((mut stream, source)) => {
                    //   有新连接：stream是TCP流，source是客户端地址

                    if let Err(error) =
                        stream.set_read_timeout(Some(std::time::Duration::from_millis(200)))
                    {
                        // ^ set_read_timeout: 设置读取超时
                        //   Some(...): Option类型，表示有超时值
                        //   Duration::from_millis: 从毫秒创建时间间隔
                        tracing::warn!(%source, %error, "failed to set read timeout");
                    }

                    let mut request_payload = Vec::new();
                    //   存储请求数据的动态字节数组
                    let mut buf = [0u8; 4096];
                    // ^ 固定大小的数组，4096字节的缓冲区
                    //   [0u8; 4096]: 语法表示4096个0（u8类型）

                    loop {
                        //   循环读取数据直到完成或出错
                        match stream.read(&mut buf) {
                            //   read(): 从流读取数据到缓冲区
                            Ok(0) => break,
                            //   返回0表示对端关闭连接
                            Ok(n) => {
                                //   n是读取的字节数
                                request_payload.extend_from_slice(&buf[..n]);
                                //   extend_from_slice: 将读取的数据追加到payload
                                //   &buf[..n]: 切片语法，取buf的前n个元素
                            }
                            Err(e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut =>
                            {
                                // ^ if guard: 匹配守卫，附加条件
                                //   WouldBlock: 非阻塞模式下暂无数据可读
                                //   TimedOut: 读取超时
                                break;
                            }
                            Err(error) => {
                                tracing::warn!(%source, %error, "failed reading RPC request payload");
                                break;
                            }
                        }
                    }

                    if request_payload.is_empty() {
                        //   如果payload为空，跳过处理
                        continue;
                    }

                    if let Some(response_payload) =
                        server.handle_request_payload::<Json>(&request_payload, source)
                    {
                        // ^ handle_request_payload: 处理JSON-RPC请求
                        //   <Json>: 显式指定使用JSON格式
                        //   返回Some(响应数据)或None
                        if let Err(error) = stream.write_all(&response_payload) {
                            //   write_all: 写入所有字节
                            tracing::warn!(%source, %error, "failed writing RPC response payload");
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    //   没有新连接（非阻塞模式正常情况）
                    //   处理来自Hub的消息（设备管理器的响应）

                    while let Ok(request) = device_control_rx.try_recv() {
                        // ^ try_recv(): 非阻塞接收
                        //   处理所有待处理的消息
                        pending_requests.insert(request.correlation_id, request.respond_to);
                        //   保存关联ID和响应发送者，等待设备管理器响应

                        let message = Message::DeviceControl {
                            device_id: request.device_id,
                            operation: request.operation,
                            params: request.params,
                            correlation_id: request.correlation_id,
                        };
                        context.hub().send(message);
                        //   通过Hub发送消息给设备管理器
                    }

                    std::thread::sleep(std::time::Duration::from_millis(50));
                    //   短暂休眠，避免CPU空转
                }
                Err(error) => {
                    tracing::warn!(%error, "RPC Server Worker accept error");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

        tracing::info!("RPC Server Worker stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 第十一部分：测试模块
// ---------------------------------------------------------------------------
// #[cfg(test)]表示这部分只在测试时编译

#[cfg(test)]
mod tests {
    // ^ mod: 定义子模块
    //   tests: 模块名

    use super::*;
    //   导入父模块的所有公共项

    #[test]
    // ^ 标记这是一个测试函数
    fn correlation_id_increments() {
        //   测试关联ID是否正确递增
        let id1 = next_correlation_id();
        let id2 = next_correlation_id();
        assert!(id2 > id1, "correlation IDs should increment");
        // ^ assert!: 断言宏，如果条件为false则测试失败
        //   第二个参数是失败时的错误信息
    }

    #[test]
    fn device_control_request_can_be_sent() {
        //   测试DeviceControlRequest能否正确通过通道发送
        let (tx, rx) = channel::<DeviceControlRequest>();
        //   创建测试用的通道
        let (response_tx, _response_rx) = channel();
        //   _response_rx: 下划线前缀表示未使用

        let request = DeviceControlRequest {
            device_id: "test-device".to_string(),
            operation: Operation::GetRegister,
            params: serde_json::json!({ "address": "h100" }),
            correlation_id: 1,
            respond_to: response_tx,
        };

        tx.send(request.clone()).unwrap();
        // ^ unwrap(): 如果Result是Err则panic，这里我们知道不会失败
        let received = rx.try_recv().unwrap();
        //   接收消息
        assert_eq!(received.device_id, "test-device");
        // ^ assert_eq!: 断言相等
        assert_eq!(received.correlation_id, 1);
    }
}
