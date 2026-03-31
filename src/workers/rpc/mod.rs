// =============================================================================
// RPC Worker Module - JSON-RPC 服务器实现
// =============================================================================
// Wave 3 重构: 合并 spawn_blocking 调用
// - 每个请求只占用 1 个 blocking thread (之前是 2 个)
// - request.rs 和 cleanup.rs 保留用于向后兼容和测试

pub mod cleanup;
pub mod connection;
pub mod handler;
pub mod request;
pub mod server;
pub mod types;
pub mod worker;

pub use types::{DeviceControlRequest, PendingRequest, ResponseSender, RpcMethod, RpcResultType};

pub use connection::handle_connection;
pub use handler::RpcHandler;
pub use server::run_async_server;
pub use worker::RpcWorker;

#[allow(deprecated)]
pub use request::handle_device_control_request;

#[allow(deprecated)]
pub use cleanup::cleanup_timed_out_requests;
