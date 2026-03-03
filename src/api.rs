//! HTTP API module using actix-web
//!
//! Provides REST endpoints for device management and monitoring.

// use 关键字用于导入其他模块或crate中的类型、函数、宏等
// actix_web 是 Rust 的 Web 框架，类似于 Python 的 Flask 或 Node.js 的 Express
// web 模块包含路由、请求处理等工具
// HttpResponse 用于构建 HTTP 响应
// Result 是 actix-web 的结果类型，用于处理请求处理中的错误
use actix_web::{web, HttpResponse, Result};
// serde 是 Rust 的序列化/反序列化框架
// Deserialize  trait 用于将 JSON 等格式转换为 Rust 结构体
// Serialize trait 用于将 Rust 结构体转换为 JSON 等格式
use serde::{Deserialize, Serialize};
// serde_json::json! 是一个宏，用于方便地创建 JSON 对象
use serde_json::json;
// HashMap 是 Rust 标准库中的哈希表实现，存储键值对
use std::collections::HashMap;
// Arc (Atomic Reference Counting) 是原子引用计数智能指针
// 用于在多个线程间安全地共享数据所有权
use std::sync::Arc;
// mpsc (multi-producer, single-consumer) 是多生产者单消费者通道
// Sender 是通道的发送端，用于向其他线程发送消息
use std::sync::mpsc::Sender;

// crate:: 表示从当前 crate 的根路径导入
// messages 模块中定义了 Message 和 Operation 枚举，用于 worker 间通信
use crate::messages::{Message, Operation};
// DeviceStatus 结构体定义了设备状态
use crate::DeviceStatus;
// parking_lot_rt 是 parking_lot 的实时版本，提供更高效的锁实现
// RwLock 是读写锁，允许多个读或一个写，比 Mutex 更灵活
use parking_lot_rt::RwLock;

// static 关键字定义静态变量，生命周期贯穿整个程序运行期间
// AtomicU64 是原子 64 位无符号整数，可以安全地在多线程间读写
// 不需要锁就能保证操作的原子性
static CORRELATION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// fn 关键字定义函数
// next_correlation_id 函数生成唯一的相关 ID，用于追踪请求
// -> u64 表示函数返回 u64 类型的值
fn next_correlation_id() -> u64 {
    // fetch_add 是原子操作，将当前值加 1 并返回旧值
    // Ordering::SeqCst 是内存排序规则，保证操作的全局顺序一致性
    CORRELATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// 文档注释（三个斜杠）用于为公共 API 生成文档
/// AppState 结构体存储在 HTTP 处理器之间共享的应用程序状态
/// 这是 actix-web 中共享数据的常见模式
pub struct AppState {
    /// Device states map (device_id -> DeviceStatus)
    /// Arc 允许多个线程共享所有权
    /// RwLock 允许并发读取或独占写入
    /// HashMap 存储设备 ID 到设备状态的映射
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    /// hub_sender 用于向其他 worker 发送消息
    /// Option 类型表示值可能存在也可能不存在（Some/None）
    /// #[allow(dead_code)] 属性告诉编译器允许未使用的代码，不发出警告
    #[allow(dead_code)]
    pub hub_sender: Option<Sender<Message>>,
}

/// GET /api/devices
///
/// 返回所有设备及其状态的列表
/// pub 关键字使函数在模块外部可见（公共）
/// async 关键字表示这是一个异步函数，可以在 .await 处挂起
/// 异步函数返回一个 Future，需要 await 来获取实际结果
pub async fn get_devices(_data: web::Data<AppState>) -> Result<HttpResponse> {
    // Ok 是 Result 枚举的变体，表示成功
    // HttpResponse::Ok() 创建 HTTP 200 OK 响应
    // .json() 方法将数据序列化为 JSON 格式并设置 Content-Type 头部
    Ok(HttpResponse::Ok().json(json!({"devices": []})))
}

/// GET /api/devices/{id}
///
/// 返回特定设备的状态
/// path 参数使用 web::Path<String> 提取 URL 路径参数
/// _data 以下划线开头表示有意不使用此参数，避免编译器警告
pub async fn get_device_by_id(
    path: web::Path<String>,
    _data: web::Data<AppState>,
) -> Result<HttpResponse> {
    // path.into_inner() 提取 Path 包装中的实际值（String 类型）
    Ok(HttpResponse::Ok().json(json!({"id": path.into_inner()})))
}

/// GET /api/health
///
/// 返回中间件的健康状态
/// 这个端点通常用于负载均衡器或监控系统检查服务是否正常
pub async fn get_health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"status": "healthy"})))
}

/// GET /api/config
///
/// 返回当前配置
pub async fn get_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"config": {}})))
}

/// POST /api/config/reload
///
/// 触发配置重新加载
pub async fn reload_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"reload": "ok"})))
}

/// Request body for setting a register
/// #[derive(...)] 是派生宏，自动为结构体实现指定的 trait
/// Debug 允许使用 {:?} 格式化打印调试信息
/// Deserialize 允许从 JSON 反序列化
/// Serialize 允许序列化为 JSON
#[derive(Debug, Deserialize, Serialize)]
pub struct SetRegisterRequest {
    /// Modbus address (e.g., "h100", "h101")
    /// String 是 Rust 的堆分配字符串类型
    pub address: String,
    /// Value to write
    /// u16 是 16 位无符号整数（0 到 65535）
    pub value: u16,
}

/// POST /api/devices/{id}/register
///
/// 在指定设备上设置寄存器值
/// body 参数使用 web::Json<T> 提取并反序列化 JSON 请求体
pub async fn set_register(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<SetRegisterRequest>,
) -> Result<HttpResponse> {
    // let 关键字用于绑定变量
    // into_inner() 提取包装类型中的值
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Create DeviceControl message
    // Message::DeviceControl 是 Message 枚举的变体
    // 使用结构体语法创建枚举变体
    let message = Message::DeviceControl {
        // clone() 创建 String 的深拷贝，因为 String 不实现 Copy trait
        device_id: device_id.clone(),
        operation: Operation::SetRegister,
        // json! 宏创建 JSON 对象
        params: json!({
            "address": request.address,
            "value": request.value,
        }),
        correlation_id: next_correlation_id(),
        respond_to: None,
    };

    // Send via Hub if available
    // if let Some(ref sender) 是模式匹配语法
    // ref 关键字借用值而不是获取所有权
    // Some(sender) 匹配 Option 的 Some 变体，提取其中的 Sender
    if let Some(ref sender) = data.hub_sender {
        // let _ = 表示故意忽略返回值
        // send() 方法可能失败（如果接收端已关闭），但这里选择忽略错误
        let _ = sender.send(message);
    }

    // 返回 JSON 响应
    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "set_register",
        "address": request.address,
        "value": request.value,
        "status": "sent"
    })))
}

/// Request body for batch operations
#[derive(Debug, Deserialize, Serialize)]
pub struct BatchOperationRequest {
    /// List of read addresses
    /// Vec<String> 是动态数组，存储多个 String
    /// #[serde(default)] 表示如果 JSON 中缺少该字段，使用默认值（空 Vec）
    #[serde(default)]
    pub read: Vec<String>,
    /// List of write operations (address, value pairs)
    #[serde(default)]
    pub write: Vec<WriteOperation>,
}

/// Single write operation
#[derive(Debug, Deserialize, Serialize)]
pub struct WriteOperation {
    pub address: String,
    pub value: u16,
}

/// POST /api/devices/{id}/batch
///
/// 对指定设备执行批量读/写操作
pub async fn batch_operations(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<BatchOperationRequest>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Determine operation type based on request content
    // if !request.write.is_empty() 检查 write 向量是否非空
    // ! 是逻辑非运算符
    let operation = if !request.write.is_empty() {
        Operation::WriteBatch
    } else {
        Operation::ReadBatch
    };

    // Create DeviceControl message
    let message = Message::DeviceControl {
        device_id: device_id.clone(),
        operation,
        params: json!({
            "read": request.read,
            "write": request.write,
        }),
        correlation_id: next_correlation_id(),
        respond_to: None,
    };

    // Send via Hub if available
    if let Some(ref sender) = data.hub_sender {
        let _ = sender.send(message);
    }

    // request.read.len() 返回向量的长度（元素个数）
    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "batch",
        "read_count": request.read.len(),
        "write_count": request.write.len(),
        "status": "sent"
    })))
}

/// Request body for robot arm movement
#[derive(Debug, Deserialize, Serialize)]
pub struct MoveRequest {
    /// Target position (e.g., "home", "pick", "place", or coordinates)
    pub position: String,
    /// Optional speed (0-100)
    /// Option<u8> 表示可能有也可能没有值
    /// u8 是 8 位无符号整数（0 到 255）
    #[serde(default)]
    pub speed: Option<u8>,
}

/// POST /api/devices/{id}/move
///
/// 移动机械臂到指定位置
pub async fn move_to(
    path: web::Path<String>,
    data: web::Data<AppState>,
    body: web::Json<MoveRequest>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let request = body.into_inner();

    // Create DeviceControl message
    let message = Message::DeviceControl {
        device_id: device_id.clone(),
        operation: Operation::MoveTo,
        params: json!({
            "position": request.position,
            // unwrap_or(100) 如果 Option 是 Some，返回值；如果是 None，返回默认值 100
            "speed": request.speed.unwrap_or(100),
        }),
        correlation_id: next_correlation_id(),
        respond_to: None,
    };

    // Send via Hub if available
    if let Some(ref sender) = data.hub_sender {
        let _ = sender.send(message);
    }

    Ok(HttpResponse::Ok().json(json!({
        "device_id": device_id,
        "operation": "move_to",
        "position": request.position,
        "status": "sent"
    })))
}

/// Configure routes for the HTTP API
///
/// Sets up all endpoint routes for the actix-web server.
/// &mut web::ServiceConfig 是对可变 ServiceConfig 的引用
/// mut 表示可变引用，允许修改所引用的值
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    // cfg.route() 注册路由
    // web::get() 指定 HTTP GET 方法
    // .to(get_devices) 将路由映射到处理函数
    cfg
        .route("/api/devices", web::get().to(get_devices))
        .route("/api/devices/{id}", web::get().to(get_device_by_id))
        .route("/api/devices/{id}/register", web::post().to(set_register))
        .route("/api/devices/{id}/batch", web::post().to(batch_operations))
        .route("/api/devices/{id}/move", web::post().to(move_to))
        .route("/api/health", web::get().to(get_health))
        .route("/api/config", web::get().to(get_config))
        .route("/api/config/reload", web::post().to(reload_config));
}

// #[cfg(test)] 属性表示以下代码只在测试时编译
#[cfg(test)]
// mod 关键字定义模块，tests 是测试模块的名称
mod tests {
    // use super::* 导入父模块（即 api 模块）的所有公共项
    use super::*;

    // #[test] 属性标记这是一个单元测试函数
    #[test]
    // test_appstate_creation 是测试函数名，以 test_ 开头是约定
    fn test_appstate_creation() {
        // Arc::new() 创建新的 Arc 智能指针
        // RwLock::new() 创建新的读写锁
        // HashMap::new() 创建新的空哈希表
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        // 创建 AppState 实例
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        // let _ = 表示创建但不使用，验证 AppState 可以成功创建
        let _ = state;
    }

    #[test]
    fn test_set_register_request_parse() {
        // r#"..."# 是原始字符串字面量，不需要转义引号
        let json = r#"{"address": "h100", "value": 42}"#;
        // serde_json::from_str() 将 JSON 字符串反序列化为结构体
        // unwrap() 如果 Result 是 Err，会 panic；这里用于测试
        let req: SetRegisterRequest = serde_json::from_str(json).unwrap();
        // assert_eq! 宏断言两个值相等，如果不等则测试失败
        assert_eq!(req.address, "h100");
        assert_eq!(req.value, 42);
    }

    #[test]
    fn test_batch_operation_request_parse() {
        let json = r#"{"read": ["h100", "h101"], "write": [{"address": "h200", "value": 100}]}"#;
        let req: BatchOperationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.read.len(), 2);
        assert_eq!(req.write.len(), 1);
    }

    #[test]
    fn test_move_request_parse() {
        let json = r#"{"position": "home", "speed": 50}"#;
        let req: MoveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.position, "home");
        // 比较 Option<u8> 和 Some(50)
        assert_eq!(req.speed, Some(50));
    }

    #[test]
    fn test_move_request_default_speed() {
        // 这个 JSON 缺少 speed 字段，应该使用默认值 None
        let json = r#"{"position": "pick"}"#;
        let req: MoveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.position, "pick");
        assert_eq!(req.speed, None);
    }

    // #[tokio::test] 是异步测试的属性宏
    // 需要 tokio 运行时来执行 async 函数
    #[tokio::test]
    async fn test_set_register_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        // web::Data::new() 创建 actix-web 的数据包装器
        let app_state = web::Data::new(state);

        // 创建请求体
        let req_body = SetRegisterRequest {
            // to_string() 将字符串字面量转换为 String 类型
            address: "h100".to_string(),
            value: 42,
        };

        // 调用异步处理函数
        // web::Path::from() 从字符串创建 Path 提取器
        // web::Json(req_body) 将请求体包装为 Json 提取器
        let result = set_register(
            web::Path::from("test-device".to_string()),
            app_state,
            web::Json(req_body),
        )
        // .await 等待异步操作完成
        .await;

        // assert!(condition) 断言条件为真
        // is_ok() 检查 Result 是否是 Ok 变体
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_batch_operations_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        let app_state = web::Data::new(state);

        // vec![] 宏创建 Vec（向量）
        let req_body = BatchOperationRequest {
            read: vec!["h100".to_string(), "h101".to_string()],
            write: vec![],  // 空向量
        };

        let result = batch_operations(
            web::Path::from("test-device".to_string()),
            app_state,
            web::Json(req_body),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_move_to_handler() {
        let device_states: Arc<RwLock<HashMap<String, DeviceStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            device_states,
            hub_sender: None,
        };
        let app_state = web::Data::new(state);

        let req_body = MoveRequest {
            position: "home".to_string(),
            speed: Some(75),  // Some() 创建 Option 的 Some 变体
        };

        let result = move_to(
            web::Path::from("robot-arm-1".to_string()),
            app_state,
            web::Json(req_body),
        )
        .await;

        assert!(result.is_ok());
    }
}
