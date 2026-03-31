//! # roboplc-middleware - 主入口
//!
//! 这是 RoboPLC 中间件的可执行文件入口。
//!
//! ## 功能
//!
//! 1. 初始化 RoboPLC 控制器
//! 2. 配置日志系统（从配置文件读取）
//! 3. 注册信号处理器（SIGINT, SIGTERM）
//! 4. 启动所有 workers 并进入主循环
//!
//! ## 运行模式
//!
//! ### 开发模式（模拟）
//! ```bash
//! ROBOPLC_SIMULATED=1 cargo run
//! ```
//! 跳过实时调度要求，适合开发和测试环境。
//!
//! ### 生产模式
//! ```bash
//! cargo run --release
//! ```
//! 使用实时调度（FIFO），需要 root 权限。
//!
//! ## Workers 初始化
//!
//! Workers 通过 `Controller::register_worker()` 注册到控制器。
//! 每个独立运行在自己的线程中，通过 Hub 进行消息传递。

use roboplc::controller::prelude::*;
use roboplc_middleware::{
    config::Config,
    workers::{
        config_loader::ConfigLoader, heartbeat_worker::HeartbeatWorker, http_worker::HttpWorker,
        latency_monitor::LatencyMonitor, manager::DeviceManager, modbus::ModbusWorker,
        rpc::worker::RpcWorker,
    },
    Message, Variables,
};
use std::path::Path;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, fmt::writer::MakeWriterExt, EnvFilter};

/// 程序主入口
///
/// 初始化 RoboPLC 框架，注册所有 workers，并启动消息循环。
///
/// # 返回值
///
/// - `Ok(())`: 正常退出
/// - `Err(...)`: 初始化过程中发生错误
///
/// # 错误处理
///
/// 任何初始化错误都会导致程序退出，错误信息会输出到日志。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 设置 panic 钩子，在程序崩溃时记录日志
    roboplc::setup_panic();

    // Enable simulated mode (no RT scheduling) only when explicitly requested
    // This allows RT scheduling in production by default
    if std::env::var("ROBOPLC_SIMULATED").is_ok() {
        roboplc::set_simulated();
        tracing::info!("Running in simulated mode (no RT scheduling)");
    }

    // 加载配置文件
    let config_path = "config.toml";
    let config = Config::from_file(config_path).expect("Failed to load config.toml");

    // 配置日志系统
    // 根据配置文件中的 logging 配置设置日志输出
    let _log_guard: Option<tracing_appender::non_blocking::WorkerGuard> =
        if config.logging.file.is_empty() {
            // 如果没有配置日志文件，只输出到控制台
            let filter = EnvFilter::new(&config.logging.level);
            fmt()
                .with_env_filter(filter)
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_level(true)
                .init();
            None
        } else {
            // 配置文件日志输出
            let log_path = Path::new(&config.logging.file);
            let parent = log_path.parent().ok_or_else(|| {
                format!(
                    "Invalid log file path (no parent directory): {}",
                    config.logging.file
                )
            })?;
            let filename = log_path.file_name().ok_or_else(|| {
                format!(
                    "Invalid log file path (no file name): {}",
                    config.logging.file
                )
            })?;
            let file_appender = if config.logging.daily_rotation {
                // 按天轮转日志文件
                rolling::daily(parent, filename)
            } else {
                // 不轮转，固定日志文件
                rolling::never(parent, filename)
            };
            // 使用非阻塞的文件追加器，避免阻塞主线程
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

            // 初始化日志订阅器，同时输出到文件和控制台
            let filter = EnvFilter::new(&config.logging.level);
            fmt()
                .with_writer(non_blocking.and(std::io::stderr))
                .with_env_filter(filter)
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_level(true)
                .init();
            eprintln!("Logging to file: {}", config.logging.file);
            Some(guard) // 保持 guard 存活到 main 结束
        };

    eprintln!("Log level: {}", config.logging.level);

    // 创建 RoboPLC 控制器
    // Controller 管理所有 workers 和消息路由（Hub）
    let mut controller: Controller<Message, Variables> = Controller::new();

    // 注册所有 workers

    // 1. RpcWorker - JSON-RPC 2.0 服务器 (端口 8080)
    controller.spawn_worker(RpcWorker::new(config.clone()))?;

    // 2. HttpWorker - HTTP API 服务器 (端口 8081)
    controller.spawn_worker(HttpWorker::new(config.clone()))?;

    // 3. DeviceManager - 设备管理器，路由消息
    controller.spawn_worker(DeviceManager::new(config.clone()))?;

    // 4. ConfigLoader - 配置热加载
    controller.spawn_worker(ConfigLoader::new(config_path.to_string(), config.clone()))?;

    // 5. LatencyMonitor - 延迟监控
    controller.spawn_worker(LatencyMonitor::new())?;

    // 6. 为每个设备创建一个 ModbusWorker
    for device in &config.devices {
        controller.spawn_worker(ModbusWorker::new(device.clone()))?;
    }

    // 7. HeartbeatWorker - 心跳检测
    // 注意：必须在 ModbusWorker 之后启动，因为它依赖 ModbusWorker 处理 GetStatus 请求
    controller.spawn_worker(HeartbeatWorker::new(config.clone()))?;

    // 注册信号处理器
    // 捕获 SIGINT (Ctrl+C) 和 SIGTERM 信号，优雅地关闭程序
    // 超时时间设置为 5 秒
    controller.register_signals(std::time::Duration::from_secs(5))?;

    // 阻塞主线程，运行消息循环
    // 在此期间，所有 workers 将并发运行，通过 Hub 交换消息
    // 当收到关闭信号时，控制器会优雅地停止所有 workers
    controller.block();

    Ok(())
}
