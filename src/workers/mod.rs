//! # Workers 模块
//!
//! 这个模块是整个中间件的核心，包含所有Worker的实现。
//!
//! ## 什么是 Worker？
//!
//! 在 RoboPLC 框架中，Worker 是一个独立的执行单元，类似于其他语言中的"线程"或"服务"。
//! 每个 Worker 负责特定的任务，它们通过 Hub 进行消息传递。
//!
//! ### Worker 的特点：
//! - 每个 Worker 有自己的事件循环（run 方法）
//! - 可以配置 CPU 亲和性和实时调度策略
//! - 通过 Hub 发送和接收消息
//! - 可以访问共享状态（Variables）
//!
//! ## 模块系统说明
//!
//! Rust 的模块系统使用 `mod` 关键字声明子模块：
//! - `pub mod xxx;` 表示公开这个子模块，外部代码可以访问
//! - 每个 `mod` 对应一个同名文件（如 `rpc_worker` 对应 `rpc_worker.rs`）
//!
//! ## 本项目中的 Workers
//!
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Worker 架构图                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │   ┌──────────────┐     ┌──────────────┐     ┌──────────┐   │
//! │   │  RpcWorker   │────▶│   Manager    │◀────│HttpWorker│   │
//! │   │  (JSON-RPC)  │     │  (消息路由)   │     │(HTTP API)│   │
//! │   └──────────────┘     └──────┬───────┘     └──────────┘   │
//! │                               │                             │
//! │                        ┌──────┴───────┐                     │
//! │                        ▼              ▼                     │
//! │                 ┌──────────┐    ┌──────────┐               │
//! │                 │ModbusWorker│   │ModbusWorker│  (每个设备一个) │
//! │                 │ (设备1)   │    │ (设备2)   │               │
//! │                 └──────────┘    └──────────┘               │
//! │                                                             │
//! │   ┌──────────────┐     ┌──────────────┐                     │
//! │   │ConfigLoader  │     │LatencyMonitor│                     │
//! │   │(配置热重载)   │     │(延迟监控)    │                     │
//! │   └──────────────┘     └──────────────┘                     │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//!
//! ### 消息传递流程示例：
//!
//! 1. **读寄存器请求**:
//!    ```text
//!    RpcWorker ──DeviceControl──▶ Manager ──DeviceControl──▶ ModbusWorker
//!         ▲                                                        │
//!         └────────DeviceResponse────────── Manager ◀──────────────┘
//!    ```
//!
//! 2. **HTTP 查询状态**:
//!    ```text
//!    HttpWorker ──SystemStatus──▶ Manager ──查询 Variables ──▶ 返回状态
//!    ```
//!
//! 3. **心跳消息**（广播给所有 Worker）:
//!    ```text
//!    ModbusWorker ──DeviceHeartbeat──▶ Hub ──▶ 所有订阅的 Workers
//!    ```

// ========== 核心 Workers ==========

/// JSON-RPC 2.0 服务器 Worker
///
/// 职责：监听指定端口（默认 8080），接收 JSON-RPC 请求
/// 消息流：接收外部请求 → 转换为 DeviceControl 消息 → 发送到 Manager
/// 对应文件：`rpc_worker.rs`
pub mod rpc_worker;

/// HTTP API Worker
///
/// 职责：提供 HTTP 管理接口（默认端口 8081）
/// 功能：查询设备状态、健康检查、配置重载等
/// 对应文件：`http_worker.rs`
pub mod http_worker;

/// 设备管理器 Worker
///
/// 职责：作为消息路由器，协调所有 Worker 之间的通信
/// 功能：
/// - 维护设备 ID 到 ModbusWorker 名称的映射
/// - 路由 DeviceControl 消息到正确的 ModbusWorker
/// - 收集 DeviceResponse 并返回给请求方
/// - 处理 SystemStatus 查询
/// 对应文件：`manager.rs`
pub mod manager;

/// Modbus TCP Worker
///
/// 职责：管理单个设备的 Modbus TCP 连接
/// 特点：每个设备对应一个独立的 ModbusWorker
/// 功能：
/// - 建立和维护 Modbus TCP 连接
/// - 执行读写寄存器操作
/// - 心跳检测和延迟监控
/// - 自动重连（指数退避策略）
/// 对应文件：`modbus_worker.rs`
pub mod modbus_worker;
pub mod modbus;

// ========== 支持性 Workers ==========

/// 配置加载 Worker
///
/// 职责：监视配置文件变化，实现热重载
/// 功能：
/// - 使用 notify crate 监听文件系统事件
/// - 配置变更时发送 ConfigUpdate 消息
/// - 避免不必要的重载（内容对比）
/// 对应文件：`config_loader.rs`
pub mod config_loader;

/// 配置更新处理器
///
/// 职责：处理配置更新逻辑
/// 注意：这个模块可能被 ConfigLoader 或其他组件使用
/// 对应文件：`config_updater.rs`
pub mod config_updater;

/// 延迟监控 Worker
///
/// 职责：监控设备响应延迟，检测异常
/// 算法：使用 3-sigma（3倍标准差）异常检测
/// 功能：
/// - 收集 ModbusWorker 上报的延迟样本
/// - 计算移动平均和标准差
/// - 标记超过 3-sigma 阈值的异常延迟
/// 对应文件：`latency_monitor.rs`
pub mod latency_monitor;

// ========== 如何使用这些模块 ==========
//
// 在其他文件中导入示例：
//
// ```rust
// // 导入整个 workers 模块
// use roboplc_middleware::workers;
//
// // 使用特定 Worker
// use roboplc_middleware::workers::manager::DeviceManager;
// use roboplc_middleware::workers::rpc_worker::RpcWorker;
//
// // 在 main.rs 中启动 Workers:
// let manager = workers::manager::DeviceManager::new(config.clone());
// controller.spawn_worker(manager, Hub::new()).unwrap();
// ```
