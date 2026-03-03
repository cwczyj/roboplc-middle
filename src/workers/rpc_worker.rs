// =============================================================================
// RPC Worker - JSON-RPC服务器实现 (异步版本)
// =============================================================================
// 这个模块实现了一个TCP上的JSON-RPC 2.0服务器
// 用于接收外部客户端的请求，并将其转发给设备管理器处理
//
// 架构说明 (Wave 2 重构):
// - 使用 HttpWorker 模式: 在 blocking worker 中 spawn tokio runtime
// - 使用 tokio::net::TcpListener 进行异步 TCP 接收
// - 使用 tokio::select! 进行并发处理
// - 使用 tokio::sync::mpsc 进行设备控制请求传递
// - 使用 tokio::sync::oneshot 进行响应处理

// ---------------------------------------------------------------------------
// 第一部分：导入模块
// ---------------------------------------------------------------------------

use crate::config::Config;
use crate::messages::{DeviceResponseData, Message, Operation};
use crate::Variables;

use roboplc::controller::prelude::*;
use roboplc::prelude::Hub;
use roboplc_rpc::{dataformat::Json, server::RpcServer, server::RpcServerHandler, RpcResult};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender as StdSender};

use std::net::SocketAddr;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration, Instant};

// ---------------------------------------------------------------------------
// 第二部分：静态变量和关联函数
// ---------------------------------------------------------------------------

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_correlation_id() -> u64 {
    CORRELATION_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// 第三部分：RPC方法枚举定义
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "m",
    content = "p",
    rename_all = "lowercase",
    deny_unknown_fields
)]
enum RpcMethod<'a> {
    Ping {},
    GetVersion {},
    GetDeviceList {},
    GetStatus {
        device_id: &'a str,
    },
    SetRegister {
        device_id: &'a str,
        address: String,
        value: u16,
    },
    GetRegister {
        device_id: &'a str,
        address: String,
    },
    MoveTo {
        device_id: &'a str,
        position: String,
    },
    ReadBatch {
        device_id: &'a str,
        addresses: Vec<String>,
    },
    WriteBatch {
        device_id: &'a str,
        values: Vec<(String, u16)>,
    },
}

// ---------------------------------------------------------------------------
// 第四部分：RPC响应结果枚举
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RpcResultType {
    Success {
        success: bool,
    },
    Version {
        version: String,
    },
    DeviceList {
        devices: Vec<String>,
    },
    Data {
        data: serde_json::Value,
    },
    Status {
        connected: bool,
        last_communication_ms: u64,
        error_count: u32,
    },
    Error {
        error: String,
    },
}

// ---------------------------------------------------------------------------
// 第五部分：类型别名和请求结构体
// ---------------------------------------------------------------------------

/// ResponseSender uses tokio oneshot channel for async-safe response handling
pub type ResponseSender = oneshot::Sender<DeviceResponseData>;

/// Device control request sent from RpcHandler to the main loop
pub struct DeviceControlRequest {
    pub device_id: String,
    pub operation: Operation,
    pub params: JsonValue,
    pub correlation_id: u64,
    pub respond_to: ResponseSender,
}

/// Pending request tracking for cleanup
struct PendingRequest {
    correlation_id: u64,
    created_at: Instant,
    respond_to: ResponseSender,
}

// ---------------------------------------------------------------------------
// 第六部分：RPC处理器结构体
// ---------------------------------------------------------------------------

struct RpcHandler {
    device_ids: Vec<String>,
    device_control_tx: mpsc::Sender<DeviceControlRequest>,
    hub: Hub<Message>,
}

impl Clone for RpcHandler {
    fn clone(&self) -> Self {
        Self {
            device_ids: self.device_ids.clone(),
            device_control_tx: self.device_control_tx.clone(),
            hub: self.hub.clone(),
        }
    }
}
impl RpcHandler {
    pub fn new(
        device_ids: Vec<String>,
        device_control_tx: mpsc::Sender<DeviceControlRequest>,
        hub: Hub<Message>,
    ) -> Self {
        Self {
            device_ids,
            device_control_tx,
            hub,
        }
    }
}

// ---------------------------------------------------------------------------
// 第七部分：实现RpcServerHandler trait
// ---------------------------------------------------------------------------

impl<'a> RpcServerHandler<'a> for RpcHandler {
    type Method = RpcMethod<'a>;
    type Result = RpcResultType;
    type Source = SocketAddr;

    fn handle_call(
        &'a self,
        method: Self::Method,
        _source: Self::Source,
    ) -> RpcResult<Self::Result> {
        match method {
            RpcMethod::Ping {} => Ok(RpcResultType::Success { success: true }),
            RpcMethod::GetVersion {} => Ok(RpcResultType::Version {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            RpcMethod::GetDeviceList {} => Ok(RpcResultType::DeviceList {
                devices: self.device_ids.clone(),
            }),
            RpcMethod::GetStatus { device_id } => {
                self.send_device_control(device_id, Operation::GetStatus, serde_json::json!({}))
            }
            RpcMethod::SetRegister {
                device_id,
                address,
                value,
            } => {
                let params = serde_json::json!({ "address": address, "value": value });
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
        let correlation_id = next_correlation_id();

        let (response_tx, response_rx) = oneshot::channel();

        let request = DeviceControlRequest {
            device_id: device_id.to_string(),
            operation,
            params,
            correlation_id,
            respond_to: response_tx,
        };

        // Use blocking_send to send from sync context to async channel
        if let Err(error) = self.device_control_tx.blocking_send(request) {
            tracing::error!(%error, "failed to send DeviceControl request");
            return Ok(RpcResultType::Error {
                error: format!("Internal error: {}", error),
            });
        }

        // Use blocking_recv to wait for response in sync context
        match response_rx.blocking_recv() {
            Ok((success, data, error)) => {
                if success {
                    Ok(RpcResultType::Data { data })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                }
            }
            Err(_) => {
                tracing::warn!(correlation_id, "Request timed out, sending cleanup");
                self.hub.send(Message::TimeoutCleanup { correlation_id });
                Ok(RpcResultType::Error {
                    error: "Request timed out".to_string(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 第九部分：RpcWorker结构体定义
// ---------------------------------------------------------------------------

#[derive(WorkerOpts)]
#[worker_opts(name = "rpc_server", blocking = true)]
pub struct RpcWorker {
    config: Config,
}

impl RpcWorker {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

// ---------------------------------------------------------------------------
// 第十部分：Worker trait实现 (异步架构)
// ---------------------------------------------------------------------------
//
// 架构说明：
// - RPC服务器在单独的线程中运行（使用tokio运行时）
// - 主线程处理Hub消息转发和响应路由
// - 使用tokio mpsc通道连接两个线程

impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let port = self.config.server.rpc_port;
        let bind_addr = format!("0.0.0.0:{}", port);

        let device_ids: Vec<String> = self.config.devices.iter().map(|d| d.id.clone()).collect();

        // Create tokio mpsc channel for device control requests
        // Buffer size of 100 to handle burst traffic
        let (device_control_tx, device_control_rx) = mpsc::channel::<DeviceControlRequest>(100);

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Pending requests tracking for cleanup
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Clone for use in the async server
        let hub = context.hub().clone();
        let bind_addr_clone = bind_addr.clone();
        let device_ids_clone = device_ids.clone();
        let pending_clone = pending.clone();

        // Spawn RPC server in a separate thread with tokio runtime
        // This follows the same pattern as HttpWorker
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("RpcWorker: failed to create Tokio runtime");

            rt.block_on(async move {
                if let Err(e) = run_async_server(
                    bind_addr_clone,
                    device_ids_clone,
                    device_control_tx,
                    device_control_rx,
                    hub,
                    shutdown_rx,
                    pending_clone,
                )
                .await
                {
                    tracing::error!(error = %e, "RPC Server error");
                }
            });
        });

        tracing::info!("RPC Server Worker started, main loop running");

        // Main loop: wait for shutdown signal
        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        // Send shutdown signal
        let _ = shutdown_tx.send(());

        tracing::info!("RPC Server Worker stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 第十一部分：异步服务器实现
// ---------------------------------------------------------------------------

/// Main async server loop using tokio::select! for concurrent handling
async fn run_async_server(
    bind_addr: String,
    device_ids: Vec<String>,
    // Note: device_control_tx is moved to RpcHandler, not used in select
    #[allow(unused_variables)] device_control_tx: mpsc::Sender<DeviceControlRequest>,
    mut device_control_rx: mpsc::Receiver<DeviceControlRequest>,
    hub: Hub<Message>,
    mut shutdown_rx: oneshot::Receiver<()>,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Use tokio::net::TcpListener for async accept
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("RPC Server started on {}", bind_addr);

    // Create handler with device_control_tx
    let handler = Arc::new(RpcHandler::new(device_ids, device_control_tx.clone(), hub.clone()));

    // Main select loop
    loop {
        tokio::select! {
            // Handle incoming TCP connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let handler = handler.clone();
                        // Spawn connection handler
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, handler).await {
                                tracing::debug!(addr = %addr, error = %e, "Connection error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Accept error");
                    }
                }
            }

            // Handle device control requests from RpcHandler
            // These need to be forwarded to the Hub
            Some(request) = async { device_control_rx.recv().await } => {
                handle_device_control_request(request, hub.clone(), pending.clone());
            }

            // Handle shutdown signal
            _ = &mut shutdown_rx => {
                tracing::info!("Shutdown signal received, stopping RPC server");
                break;
            }

            // Periodic cleanup of timed-out requests
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                cleanup_timed_out_requests(pending.clone(), hub.clone());
            }
        }
    }

    Ok(())
}

/// Handle a single TCP connection
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), std::io::Error> {
    // Read request with timeout
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // Connection closed
            Ok(Ok(n)) => {
                request_payload.extend_from_slice(&buf[..n]);
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, error = %e, "Read error");
                return Err(e);
            }
            Err(_) => {
                // Timeout - no more data coming
                break;
            }
        }
    }

    if request_payload.is_empty() {
        return Ok(());
    }

    // Create RpcServer for this connection (fresh each time to avoid generic complexity)
    let server = RpcServer::new((*handler).clone());

    // Process request
    if let Some(response_payload) = server.handle_request_payload::<Json>(&request_payload, addr) {
        // Write response with timeout
        timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await??;
    }

    Ok(())
}

/// Handle device control request by forwarding to Hub
fn handle_device_control_request(
    request: DeviceControlRequest,
    hub: Hub<Message>,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    let correlation_id = request.correlation_id;
    let respond_to = request.respond_to;

    // Track pending request for cleanup
    {
        let mut pending_lock = pending.lock().unwrap();
        pending_lock.insert(
            correlation_id,
            PendingRequest {
                correlation_id,
                created_at: Instant::now(),
                respond_to,
            },
        );
    }

    // Create std::sync::mpsc channel for Message compatibility
    let (std_tx, std_rx): (StdSender<DeviceResponseData>, _) = channel();

    let message = Message::DeviceControl {
        device_id: request.device_id,
        operation: request.operation,
        params: request.params,
        correlation_id,
        respond_to: Some(std_tx),
    };

    // Send to DeviceManager via Hub
    hub.send(message);

    // Bridge std::sync::mpsc response to tokio::sync::oneshot
    // Use spawn_blocking to wait on the std channel
    let hub_for_cleanup = hub.clone();
    let pending_for_cleanup = pending.clone();
    tokio::task::spawn_blocking(move || {
        match std_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(response) => {
                // Remove from pending and send response
                let mut pending_lock = pending_for_cleanup.lock().unwrap();
                if let Some(req) = pending_lock.remove(&correlation_id) {
                    let _ = req.respond_to.send(response);
                }
            }
            Err(_) => {
                tracing::warn!(correlation_id, "Request timed out in bridge");
                hub_for_cleanup.send(Message::TimeoutCleanup { correlation_id });

                // Remove from pending and send error response
                let mut pending_lock = pending_for_cleanup.lock().unwrap();
                if let Some(req) = pending_lock.remove(&correlation_id) {
                    let _ = req.respond_to.send((false, serde_json::json!({}), Some("Request timed out".to_string())));
                }
            }
        }
    });
}

/// Cleanup timed-out requests
fn cleanup_timed_out_requests(
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    hub: Hub<Message>,
) {
    let timeout_duration = Duration::from_secs(35); // Slightly longer than request timeout

    let mut pending_lock = pending.lock().unwrap();
    let now = Instant::now();

    let timed_out: Vec<u64> = pending_lock
        .iter()
        .filter(|(_, req)| now.duration_since(req.created_at) > timeout_duration)
        .map(|(&id, _)| id)
        .collect();

    for id in timed_out {
        if let Some(req) = pending_lock.remove(&id) {
            // Send error response
            let _ = req.respond_to.send((
                false,
                serde_json::json!({}),
                Some("Request timed out during cleanup".to_string()),
            ));

            // Notify Hub about timeout
            hub.send(Message::TimeoutCleanup { correlation_id: id });
            tracing::warn!(correlation_id = id, "Cleaned up timed-out request");
        }
    }
}

// ---------------------------------------------------------------------------
// 第十二部分：Trait实现
// ---------------------------------------------------------------------------

// Need AsyncRead/AsyncWrite for tokio::net::TcpStream
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// 第十三部分：测试模块
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_id_increments() {
        let id1 = next_correlation_id();
        let id2 = next_correlation_id();
        assert!(id2 > id1, "correlation IDs should increment");
    }

    #[test]
    fn pending_request_tracking() {
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx, _rx) = oneshot::channel();
        let req = PendingRequest {
            correlation_id: 1,
            created_at: Instant::now(),
            respond_to: tx,
        };

        {
            let mut p = pending.lock().unwrap();
            p.insert(1, req);
        }

        let p = pending.lock().unwrap();
        assert!(p.contains_key(&1));
    }
}

// ===========================================================================
// Extended Tests for RpcWorker async implementation (Wave 5, Tasks 20-21)
// ===========================================================================
// Additional tests for channels, timeout handling, and concurrent requests
// These tests are in a separate module to avoid conflicts with existing tests

#[cfg(test)]
mod extended_tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    // =========================================================================
    // Task 20: Channel Unit Tests
    // =========================================================================

    /// Test that mpsc channel send and receive work correctly
    #[tokio::test]
    async fn mpsc_channel_send_receive() {
        let (tx, mut rx) = mpsc::channel::<DeviceControlRequest>(10);

        let (response_tx, _response_rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "test-device".to_string(),
            operation: Operation::GetStatus,
            params: serde_json::json!({}),
            correlation_id: 1,
            respond_to: response_tx,
        };

        // Send request
        tx.send(request).await.unwrap();

        // Receive request
        let received = rx.recv().await.unwrap();
        assert_eq!(received.device_id, "test-device");
        assert_eq!(received.correlation_id, 1);
    }

    /// Test that mpsc channel handles multiple messages in order
    #[tokio::test]
    async fn mpsc_channel_ordering() {
        let (tx, mut rx) = mpsc::channel::<DeviceControlRequest>(10);

        for i in 0..5 {
            let (response_tx, _) = oneshot::channel();
            let request = DeviceControlRequest {
                device_id: format!("device-{}", i),
                operation: Operation::GetStatus,
                params: serde_json::json!({}),
                correlation_id: i as u64,
                respond_to: response_tx,
            };
            tx.send(request).await.unwrap();
        }

        // Verify ordering
        for i in 0..5 {
            let received = rx.recv().await.unwrap();
            assert_eq!(received.correlation_id, i as u64);
        }
    }

    /// Test that mpsc channel respects buffer size
    #[tokio::test]
    async fn mpsc_channel_buffer_size() {
        let (tx, mut rx) = mpsc::channel::<DeviceControlRequest>(2);

        // Fill buffer
        for i in 0..2 {
            let (response_tx, _) = oneshot::channel();
            let request = DeviceControlRequest {
                device_id: "test".to_string(),
                operation: Operation::GetStatus,
                params: serde_json::json!({}),
                correlation_id: i,
                respond_to: response_tx,
            };
            tx.clone().send(request).await.unwrap();
        }

        // Verify we can receive all messages
        assert!(rx.recv().await.is_some());
        assert!(rx.recv().await.is_some());
    }

    /// Test oneshot channel send and receive
    #[tokio::test]
    async fn oneshot_channel_basic() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        let response: DeviceResponseData = (true, serde_json::json!({"status": "ok"}), None);
        tx.send(response).unwrap();

        let received = rx.await.unwrap();
        assert!(received.0); // success flag
        assert_eq!(received.1["status"], "ok");
    }

    /// Test oneshot channel with error response
    #[tokio::test]
    async fn oneshot_channel_error_response() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        let response: DeviceResponseData = (
            false,
            serde_json::json!({}),
            Some("Device not found".to_string()),
        );
        tx.send(response).unwrap();

        let received = rx.await.unwrap();
        assert!(!received.0);
        assert!(received.2.is_some());
        assert_eq!(received.2.unwrap(), "Device not found");
    }

    /// Test that oneshot receiver detects sender drop
    #[tokio::test]
    async fn oneshot_channel_sender_dropped() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();
        drop(tx);

        let result = rx.await;
        assert!(result.is_err(), "Should detect sender was dropped");
    }

    // =========================================================================
    // Task 21: Timeout Handling Unit Tests
    // =========================================================================

    /// Test that tokio::timeout works for oneshot channel
    #[tokio::test]
    async fn oneshot_timeout_detection() {
        let (_tx, rx) = oneshot::channel::<DeviceResponseData>();

        // Timeout after 10ms
        let result = timeout(Duration::from_millis(10), rx).await;
        assert!(result.is_err(), "Should timeout");
    }

    /// Test that oneshot completes before timeout
    #[tokio::test]
    async fn oneshot_completes_before_timeout() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        let response: DeviceResponseData = (true, serde_json::json!({}), None);
        tx.send(response).unwrap();

        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok(), "Should complete before timeout");
    }

    /// Test cleanup of timed-out requests
    #[tokio::test]
    async fn cleanup_removes_timed_out_requests() {
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Add an old request (simulated as created 40 seconds ago)
        let (tx, _) = oneshot::channel();
        {
            let mut p = pending.lock().unwrap();
            p.insert(
                1,
                PendingRequest {
                    correlation_id: 1,
                    created_at: Instant::now() - Duration::from_secs(40),
                    respond_to: tx,
                },
            );
        }

        // Add a fresh request
        let (tx2, _) = oneshot::channel();
        {
            let mut p = pending.lock().unwrap();
            p.insert(
                2,
                PendingRequest {
                    correlation_id: 2,
                    created_at: Instant::now(),
                    respond_to: tx2,
                },
            );
        }

        // Run cleanup logic pattern
        let mut pending_lock = pending.lock().unwrap();
        let now = Instant::now();
        let timeout_duration = Duration::from_secs(35);

        let timed_out: Vec<u64> = pending_lock
            .iter()
            .filter(|(_, req)| now.duration_since(req.created_at) > timeout_duration)
            .map(|(&id, _)| id)
            .collect();

        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], 1);

        // Remove timed out
        for id in timed_out {
            pending_lock.remove(&id);
        }

        assert_eq!(pending_lock.len(), 1);
        assert!(pending_lock.contains_key(&2));
    }

    /// Test timeout with mpsc receiver
    #[tokio::test]
    async fn mpsc_receiver_timeout() {
        let (_tx, mut rx) = mpsc::channel::<DeviceControlRequest>(10);

        // Timeout on empty channel
        let result = timeout(Duration::from_millis(10), rx.recv()).await;
        assert!(result.is_err(), "Should timeout on empty channel");
    }

    // =========================================================================
    // Correlation ID Tests
    // =========================================================================

    #[test]
    fn correlation_id_is_unique() {
        let ids: Vec<u64> = (0..100).map(|_| next_correlation_id()).collect();

        // Check all IDs are unique
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "All correlation IDs should be unique");
    }

    // =========================================================================
    // Pending Request Tracking Tests
    // =========================================================================

    #[test]
    fn pending_request_removal() {
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx, _) = oneshot::channel();
        {
            let mut p = pending.lock().unwrap();
            p.insert(
                1,
                PendingRequest {
                    correlation_id: 1,
                    created_at: Instant::now(),
                    respond_to: tx,
                },
            );
        }

        // Remove the request
        {
            let mut p = pending.lock().unwrap();
            p.remove(&1);
        }

        let p = pending.lock().unwrap();
        assert!(!p.contains_key(&1));
        assert_eq!(p.len(), 0);
    }

    // =========================================================================
    // DeviceControlRequest Tests
    // =========================================================================

    #[test]
    fn device_control_request_creation() {
        let (tx, _rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "plc-1".to_string(),
            operation: Operation::SetRegister,
            params: serde_json::json!({"address": "h100", "value": 42}),
            correlation_id: 12345,
            respond_to: tx,
        };

        assert_eq!(request.device_id, "plc-1");
        assert!(matches!(request.operation, Operation::SetRegister));
        assert_eq!(request.correlation_id, 12345);
    }

    // =========================================================================
    // Concurrent Request Simulation Tests
    // =========================================================================

    /// Simulate multiple concurrent requests with channel
    #[tokio::test]
    async fn concurrent_requests_via_channel() {
        let (tx, mut rx) = mpsc::channel::<DeviceControlRequest>(100);

        // Spawn multiple senders
        let mut handles = vec![];
        for i in 0..10 {
            let tx_clone = tx.clone();
            let handle = tokio::spawn(async move {
                let (response_tx, _) = oneshot::channel();
                let request = DeviceControlRequest {
                    device_id: format!("device-{}", i),
                    operation: Operation::GetStatus,
                    params: serde_json::json!({}),
                    correlation_id: i as u64,
                    respond_to: response_tx,
                };
                tx_clone.send(request).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all senders
        for handle in handles {
            handle.await.unwrap();
        }

        // Receive all messages
        let mut received_count = 0;
        while let Ok(Some(_)) = timeout(Duration::from_millis(100), rx.recv()).await {
            received_count += 1;
        }

        assert_eq!(received_count, 10);
    }

    /// Test response routing with oneshot channels
    #[tokio::test]
    async fn response_routing_via_oneshot() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        // Simulate response from worker
        let handle = tokio::spawn(async move {
            sleep(Duration::from_millis(5)).await;
            let response: DeviceResponseData = (
                true,
                serde_json::json!({"temperature": 25.5}),
                None,
            );
            tx.send(response).unwrap();
        });

        // Wait for response with timeout
        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok());

        let response = result.unwrap().unwrap();
        assert!(response.0);
        assert_eq!(response.1["temperature"], 25.5);

        handle.await.unwrap();
    }
}
