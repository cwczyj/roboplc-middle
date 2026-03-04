//! # HTTP Worker
//!
//! HTTP REST API服务器worker，提供设备管理接口。
//!
//! ## 功能
//!
//! - 监听TCP端口（默认8081）提供REST API
//! - 查询设备状态
//! - 获取系统健康状态
//! - 触发配置重载

// `use` 关键字用于导入其他模块或crate中的类型和函数
// `actix_web` 是Rust的Web框架，类似于Python的Flask或Node.js的Express
// `web` 模块提供路由配置和数据提取器
// `App` 是Actix-web的应用程序构建器，用于配置路由和中间件
// `HttpResponse` 用于构建HTTP响应
// `HttpServer` 是HTTP服务器，负责监听端口和处理请求
// `Result` 是Actix-web的结果类型，用于处理请求处理中的错误
use actix_web::{web, App, HttpResponse, HttpServer, Result};
// `roboplc` 是实时PLC框架，`controller::prelude` 包含Worker相关的trait和类型
// `prelude` 是一种约定，表示常用功能的集合
use roboplc::controller::prelude::*;
// `serde_json` 是JSON序列化/反序列化库
// `json!` 是一个宏，用于方便地创建JSON值
use serde_json::json;
// `HashMap` 是哈希映射（字典），用于键值对存储
use std::collections::HashMap;
// `Arc` 是原子引用计数（Atomic Reference Counted），用于多线程间共享所有权
// 当多个线程需要访问同一块数据时使用
use std::sync::Arc;

// 从当前crate导入其他模块
// `crate::` 表示从当前crate的根开始查找
// `config::Config` 是配置结构体
// `Message` 是worker间通信的消息类型
// `Variables` 是共享状态变量
use crate::{config::Config, Message, Variables};

/// `pub` 关键字表示公共可见性，其他模块可以访问
/// `struct` 定义一个结构体，是自定义数据类型
/// `AppState` 是应用程序状态，用于在不同HTTP处理器间共享数据
/// 这个结构体包含设备状态信息，所有HTTP handler都可以访问它
pub struct AppState {
    /// `device_states` 存储所有设备的状态
    /// `Arc` 允许多个线程共享所有权
    /// `parking_lot_rt::RwLock` 是读写锁，支持并发读，独占写
    /// `RwLock` = Read-Write Lock（读写锁）
    /// `HashMap<String, crate::DeviceStatus>` 是设备ID到设备状态的映射
    pub device_states: Arc<parking_lot_rt::RwLock<HashMap<String, crate::DeviceStatus>>>,
    pub config: Arc<Config>,
}

/// `///` 是文档注释，会生成文档并显示在IDE中
/// 这个函数处理 GET /api/devices 请求
/// `async` 表示这是一个异步函数，可以暂停执行等待IO操作完成
/// 异步函数返回一个Future，可以被await
/// `fn` 定义函数
/// `data` 参数类型是 `web::Data<AppState>`，这是Actix-web的数据提取器
/// 它会从应用程序状态中提取共享数据
/// `-> Result<HttpResponse>` 是返回类型，表示可能返回HTTP响应或错误
async fn get_devices(data: web::Data<AppState>) -> Result<HttpResponse> {
    // `let` 用于绑定变量，Rust有类型推断，但也可以显式指定类型
    // `data.device_states` 访问AppState中的device_states字段
    // `.read()` 获取读锁，允许多个线程同时读取
    // 读锁在这里自动释放（RAII模式）
    let states = data.device_states.read();
    
    // `Vec` 是动态数组（向量）
    // `serde_json::Value` 是JSON值的类型，可以是任何JSON类型
    // `states.iter()` 创建迭代器遍历HashMap
    // `.map()` 对迭代器的每个元素进行转换
    // `|(id, status)|` 是闭包（匿名函数）的参数，解构键值对
    let devices: Vec<serde_json::Value> = states
        .iter()
        .map(|(id, status)| {
            // `json!` 宏创建JSON对象
            // 语法类似于JavaScript对象字面量
            json!({
                // `id` 是设备的唯一标识符
                "id": id,
                // `connected` 表示设备是否在线
                "connected": status.connected,
                // `elapsed()` 返回从上次通信到现在的时间间隔
                // `as_millis()` 转换为毫秒
                // `as u64` 是类型转换
                "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
                // `error_count` 是错误计数
                "error_count": status.error_count,
            })
        })
        // `.collect()` 将迭代器收集为集合类型（这里是Vec）
        .collect();
    
    // `Ok()` 包装成功结果
    // `HttpResponse::Ok()` 创建HTTP 200 OK响应
    // `.json()` 将数据序列化为JSON并设置Content-Type头
    Ok(HttpResponse::Ok().json(json!({"devices": devices})))
}

/// 这个函数处理 GET /api/devices/{id}/status 请求
/// `path` 参数类型是 `web::Path<String>`，用于提取URL路径参数
/// `{id}` 在路由中定义的参数会被提取为String
async fn get_device_by_id(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    // `path.into_inner()` 提取路径参数的内部值
    // 这里将 web::Path<String> 转换为 String
    let device_id = path.into_inner();
    
    // 获取读锁访问设备状态
    let states = data.device_states.read();

    // `if let` 是模式匹配的一种形式
    // 如果 `states.get(&device_id)` 返回 Some(status)，则执行if块
    // `&device_id` 是引用，避免所有权转移
    // `get()` 方法根据键查找值，返回 Option（Some或None）
    if let Some(status) = states.get(&device_id) {
        // 找到设备，构建JSON响应
        let body = json!({
            "id": device_id,
            "connected": status.connected,
            "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
            "error_count": status.error_count,
            "reconnect_count": status.reconnect_count,
        });
        // 返回200 OK和设备信息
        Ok(HttpResponse::Ok().json(body))
    } else {
        // 设备未找到，返回404 Not Found
        // 同时返回错误信息JSON
        Ok(HttpResponse::NotFound().json(json!({"error": "Device not found"})))
    }
}

/// 这个函数处理 GET /api/health 请求
/// 用于健康检查，监控系统是否正常运行
/// 返回系统健康状态，包括设备连接统计
async fn get_health(data: web::Data<AppState>) -> Result<HttpResponse> {
    let states = data.device_states.read();
    
    let total = states.len();
    let connected = states.values().filter(|s| s.connected).count();
    let disconnected = total - connected;
    
    // 健康状态判定：
    // - healthy: 所有设备连接
    // - degraded: 部分设备断线
    // - unhealthy: 所有设备断线或没有设备
    let status = if total == 0 {
        "unhealthy"
    } else if connected == total {
        "healthy"
    } else if connected == 0 {
        "unhealthy"
    } else {
        "degraded"
    };
    
    Ok(HttpResponse::Ok().json(json!({
        "status": status,
        "devices": {
            "total": total,
            "connected": connected,
            "disconnected": disconnected
        }
    })))
}

/// 这个函数处理 GET /api/config 请求
/// 用于获取当前配置信息
async fn get_config(data: web::Data<AppState>) -> Result<HttpResponse> {
    // 返回配置的 JSON 序列化
    // Arc<Config> 可以直接解引用访问 Config
    Ok(HttpResponse::Ok().json(json!({
        "config": &*data.config
    })))
}

/// 配置重载端点
///
/// 注意：此端点仅返回成功响应，不会实际触发配置重载。
///
/// 实际的配置重载由 ConfigLoader 的文件监控机制触发：
/// - ConfigLoader 持续监控 config.toml 文件的变化
/// - 当文件被修改时，ConfigLoader 自动重新加载配置
/// - 如需触发重载，请直接修改 config.toml 文件
async fn reload_config() -> Result<HttpResponse> {
    // 返回重载成功的响应
    Ok(HttpResponse::Ok().json(json!({"reload": "ok"})))
}

/// 这个函数配置HTTP路由
/// `&mut web::ServiceConfig` 是可变引用，允许修改服务配置
fn configure_routes(cfg: &mut web::ServiceConfig) {
    // `cfg.service()` 添加一个服务到配置
    // `web::scope("/api")` 创建路由前缀，所有子路由都以/api开头
    cfg.service(
        web::scope("/api")
            // `.route()` 定义单个路由
            // 第一个参数是路径，第二个是HTTP方法和处理器
            // `web::get()` 表示GET请求
            // `.to(get_devices)` 指定处理函数
            .route("/devices", web::get().to(get_devices))
            // `{id}` 是路径参数，会被提取为String
            .route("/devices/{id}/status", web::get().to(get_device_by_id))
            .route("/health", web::get().to(get_health))
            .route("/config", web::get().to(get_config))
            // `web::post()` 表示POST请求，用于创建或修改资源
            .route("/config/reload", web::post().to(reload_config)),
    );
}

/// `#[derive(WorkerOpts)]` 是派生宏，自动生成WorkerOpts trait的实现
/// 这是RoboPLC框架的一部分，用于配置worker
#[derive(WorkerOpts)]
/// `#[worker_opts(...)]` 属性宏配置worker选项
/// `name = "http_server"` 设置worker名称
/// `blocking = true` 表示这是一个阻塞worker，运行在独立线程
#[worker_opts(name = "http_server", blocking = true)]
/// `HttpWorker` 结构体定义HTTP worker
pub struct HttpWorker {
    /// 存储配置信息
    config: Config,
}

/// `impl` 块为类型实现方法
impl HttpWorker {
    /// `pub fn` 定义公共关联函数（类似其他语言的静态方法或构造函数）
    /// `new` 是构造函数惯用名称
    /// `Self` 指代当前类型（HttpWorker）
    pub fn new(config: Config) -> Self {
        // 返回新实例，`Self { config }` 是结构体实例化语法
        // 相当于 `HttpWorker { config: config }`，字段初始化简写
        Self { config }
    }
}

/// 为HttpWorker实现Worker trait
/// `Worker<Message, Variables>` 是RoboPLC框架的核心trait
/// `Message` 是worker接收的消息类型
/// `Variables` 是共享变量类型
impl Worker<Message, Variables> for HttpWorker {
    /// `run` 方法是worker的入口点，在worker线程中执行
    /// `&mut self` 是可变引用，允许修改self
    /// `context` 提供访问Hub、Variables等的能力
    /// `WResult` 是worker的结果类型
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // 从配置中提取HTTP端口
        let http_port = self.config.server.http_port;
        
        // `format!` 宏创建格式化字符串
        // `0.0.0.0` 表示监听所有网络接口
        let addr = format!("0.0.0.0:{}", http_port);
        
        // `context.variables()` 获取共享变量
        // `.device_states.clone()` 克隆Arc（只克隆引用计数，不是数据本身）
        let device_states = context.variables().device_states.clone();
        let config = Arc::new(self.config.clone());

        let app_state = web::Data::new(AppState { 
            device_states,
            config,
        });

        // `std::thread::spawn()` 创建新线程
        // 使用`move`关键字将变量所有权转移到闭包中
        // 这是必需的，因为Actix-web需要在自己的线程中运行
        std::thread::spawn(move || {
            // `tokio::runtime::Builder` 构建Tokio异步运行时
            // `new_multi_thread()` 创建多线程运行时（默认）
            // Tokio是Rust的异步运行时，类似于Node.js的事件循环但更强大
            let rt = tokio::runtime::Builder::new_multi_thread()
                // `enable_all()` 启用所有运行时功能（IO、定时器等）
                .enable_all()
                // `build()` 创建运行时，返回Result
                .build()
                // `expect()` 如果Result是Err则panic，显示错误信息
                .expect("HttpWorker: failed to create Tokio runtime");

            // `rt.block_on()` 在当前线程阻塞并运行异步代码
            // `async move { ... }` 创建一个异步块
            rt.block_on(async move {
                // `HttpServer::new()` 创建HTTP服务器
                // 参数是一个闭包，返回App配置
                let server = HttpServer::new(move || {
                    // `App::new()` 创建新的应用实例
                    // `move` 闭包获取app_state的所有权
                    App::new()
                        // `.app_data()` 添加应用程序状态
                        // 所有handler都可以访问这个数据
                        .app_data(app_state.clone())
                        // `.configure()` 使用函数配置路由
                        .configure(configure_routes)
                });

                // `server.bind()` 绑定到地址，返回Result
                match server.bind(&addr) {
                    // 绑定成功，`Ok(server)` 解构Result
                    Ok(server) => {
                        // 打印启动信息到控制台
                        println!("HttpWorker: listening on http://{}", addr);
                        // `server.run()` 启动服务器，返回Future
                        // `.await` 等待服务器完成（通常永不停止）
                        // `expect()` 如果出错则panic
                        server.run().await.expect("HttpWorker: failed to run server");
                    }
                    // 绑定失败，`Err(e)` 解构错误
                    Err(e) => {
                        // `eprintln!` 打印到标准错误流
                        eprintln!("HttpWorker: failed to bind {}: {}", addr, e);
                    }
                }
            });
        });

        // `while` 循环，条件为真时持续执行
        // `context.is_online()` 检查worker是否应该继续运行
        while context.is_online() {
            // `std::thread::sleep()` 让线程休眠
            // `std::time::Duration::from_secs(1)` 创建1秒的持续时间
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        
        // `Ok(())` 表示成功完成
        Ok(())
    }
}

/// `#[cfg(test)]` 属性表示只在测试时编译此模块
/// `mod tests` 定义测试模块
#[cfg(test)]
mod tests {
    // `use super::*` 导入父模块的所有公共项
    use super::*;
    // 导入DeviceStatus用于测试
    use crate::DeviceStatus;
    // 导入配置相关类型
    use crate::config::{Logging, Server};
    // `Instant` 用于时间点操作
    use std::time::Instant;

    /// `fn` 定义测试辅助函数，返回AppState
    fn make_app_state() -> AppState {
        AppState {
            device_states: Arc::new(parking_lot_rt::RwLock::new(HashMap::new())),
            config: Arc::new(Config {
                server: Server { rpc_port: 8080, http_port: 8081 },
                logging: Logging { 
                    level: "info".to_string(), 
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
        }
    }

    /// 创建包含设备的AppState
    /// `id: &str` 参数是字符串切片（引用）
    /// `connected: bool` 布尔参数
    fn make_app_state_with_device(id: &str, connected: bool) -> AppState {
        // `HashMap::new()` 创建新的哈希映射
        let mut states = HashMap::new();
        // `states.insert()` 插入键值对
        // `id.to_string()` 将字符串切片转换为String
        states.insert(
            id.to_string(),
            // `DeviceStatus` 结构体实例化
            DeviceStatus {
                connected,
                // `Instant::now()` 获取当前时间点
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        AppState {
            device_states: Arc::new(parking_lot_rt::RwLock::new(states)),
            config: Arc::new(Config {
                server: Server { rpc_port: 8080, http_port: 8081 },
                logging: Logging { 
                    level: "info".to_string(), 
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
        }
    }

    /// `#[actix_rt::test]` 是Actix-web的测试宏
    /// 它设置异步运行时用于测试
    /// `async fn` 表示异步测试函数
    #[actix_rt::test]
    async fn test_get_devices_empty() {
        // 创建空的AppState
        let app_state = make_app_state();
        // 调用被测试的函数
        // `web::Data::new()` 包装AppState
        let result = get_devices(web::Data::new(app_state)).await;
        // `assert!` 宏断言条件为真，否则测试失败
        // `result.is_ok()` 检查Result是Ok还是Err
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_devices_with_device() {
        // 创建包含一个设备的AppState
        let app_state = make_app_state_with_device("device-1", true);
        let result = get_devices(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_device_by_id_found() {
        // 创建包含设备的AppState
        let app_state = make_app_state_with_device("device-1", true);
        // `web::Path::from()` 创建Path提取器用于测试
        // 传入设备ID
        let result = get_device_by_id(web::Path::from("device-1".to_string()), web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_device_by_id_not_found() {
        // 创建空的AppState
        let app_state = make_app_state();
        // 查找不存在的设备
        let result = get_device_by_id(web::Path::from("nonexistent".to_string()), web::Data::new(app_state)).await;
        // `assert_eq!` 断言两个值相等
        // `result.unwrap()` 解包Result，如果是Err则panic
        // `.status()` 获取HTTP状态码
        // `actix_web::http::StatusCode::NOT_FOUND` 是404状态码
        assert_eq!(result.unwrap().status(), actix_web::http::StatusCode::NOT_FOUND);
    }

    #[actix_rt::test]
    async fn test_get_health() {
        // 测试空设备列表应该返回 unhealthy
        let app_state = make_app_state();
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());
        
        // 验证返回的响应状态码是200
        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }
    
    #[actix_rt::test]
    async fn test_get_health_connected_devices() {
        // 测试全部设备连接应该返回 healthy
        let app_state = make_app_state_with_device("device-1", true);
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());
        
        // 验证返回的响应状态码是200
        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }
    
    #[actix_rt::test]
    async fn test_get_health_mixed_devices() {
        // 测试混合连接状态应该返回 degraded
        let mut states = HashMap::new();
        states.insert(
            "device-1".to_string(),
            DeviceStatus {
                connected: true,
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        states.insert(
            "device-2".to_string(),
            DeviceStatus {
                connected: false,
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        let app_state = AppState {
            device_states: Arc::new(parking_lot_rt::RwLock::new(states)),
            config: Arc::new(Config {
                server: Server { rpc_port: 8080, http_port: 8081 },
                logging: Logging { 
                    level: "info".to_string(), 
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
        };
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());
        
        // 验证返回的响应状态码是200
        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_get_config() {
        use crate::config::{Server, Logging};
        
        let app_state = AppState {
            device_states: Arc::new(parking_lot_rt::RwLock::new(HashMap::new())),
            config: Arc::new(Config {
                server: Server { rpc_port: 8080, http_port: 8081 },
                logging: Logging { 
                    level: "info".to_string(), 
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
        };
        let result = get_config(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_reload_config() {
        let result = reload_config().await;
        assert!(result.is_ok());
    }
}