//! # roboplc-middleware - 主入口
//!
//! 这是 RoboPLC 中间件的可执行文件入口。
//!
//! ## 功能
//!
//! 1. 初始化 RoboPLC 控制器
//! 2. 配置日志系统
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
        config_loader::ConfigLoader,
        http_worker::HttpWorker,
        latency_monitor::LatencyMonitor,
        manager::DeviceManager,
        modbus_worker::ModbusWorker,
        rpc_worker::RpcWorker,
    },
    Message, Variables,
};

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

    // 配置日志系统，Info 级别及以上输出
    roboplc::configure_logger(roboplc::LevelFilter::Info);

    // 设置模拟模式，跳过实时调度要求
    // 在生产环境中，移除此行以使用实时调度
    roboplc::set_simulated();

    // 创建 RoboPLC 控制器
    // Controller 管理所有 workers 和消息路由（Hub）
    let mut controller: Controller<Message, Variables> = Controller::new();

    // 加载配置文件
    let config_path = "config.toml";
    let config = Config::from_file(config_path).expect("Failed to load config.toml");

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
