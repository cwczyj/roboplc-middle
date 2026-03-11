// =============================================================================
// RPC Worker Module - JSON-RPC 服务器实现
// =============================================================================
//
// 这个模块实现了基于 TCP 的 JSON-RPC 2.0 服务器，用于接收外部客户端的请求，
// 并将其转发给设备管理器处理。
//
// 模块结构:
// - worker.rs: RpcWorker 主 worker 实现
// - handler.rs: RpcHandler 和 RpcServerHandler trait 实现
// - server.rs: 异步服务器主循环 (run_async_server)
// - connection.rs: TCP 连接处理
// - request.rs: 设备控制请求处理和响应路由
// - cleanup.rs: 超时请求清理逻辑
// - types.rs: 类型定义 (RpcMethod, RpcResultType, DeviceControlRequest 等)
//
// 架构说明:
// - 使用 HttpWorker 模式：在 blocking worker 中 spawn tokio runtime
// - 使用 tokio::net::TcpListener 进行异步 TCP 接收
// - 使用 tokio::select! 进行并发处理
// - 使用 tokio::sync::mpsc 进行设备控制请求传递
// - 使用 tokio::sync::oneshot 进行响应处理

pub mod types;
pub mod handler;
pub mod server;
pub mod connection;
pub mod request;
pub mod cleanup;
pub mod worker;

// 重新导出主要类型
pub use types::{
    RpcMethod,
    RpcResultType,
    DeviceControlRequest,
    PendingRequest,
    ResponseSender,
};

pub use handler::{RpcHandler, next_correlation_id};
pub use worker::RpcWorker;
pub use server::run_async_server;
pub use connection::handle_connection;
pub use request::handle_device_control_request;
pub use cleanup::cleanup_timed_out_requests;