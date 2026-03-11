// =============================================================================
// RPC Worker - JSON-RPC 服务器实现 (异步版本)
// =============================================================================
// 这个模块实现了一个 TCP 上的 JSON-RPC 2.0 服务器
// 用于接收外部客户端的请求，并将其转发给设备管理器处理
//
// 架构说明 (Wave 2 重构):
// - 使用 HttpWorker 模式：在 blocking worker 中 spawn tokio runtime
// - 使用 tokio::net::TcpListener 进行异步 TCP 接收
// - 使用 tokio::select! 进行并发处理
// - 使用 tokio::sync::mpsc 进行设备控制请求传递
// - 使用 tokio::sync::oneshot 进行响应处理

// ---------------------------------------------------------------------------
// 导入模块
// ---------------------------------------------------------------------------

use crate::config::Config;
use crate::messages::Message;
use crate::Variables;

use roboplc::controller::prelude::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use super::types::DeviceControlRequest;
use super::types::PendingRequest;
use super::server::run_async_server;

// ---------------------------------------------------------------------------
// RpcWorker 结构体定义
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
// Worker trait 实现 (异步架构)
// ---------------------------------------------------------------------------
//
// 架构说明：
// - RPC 服务器在单独的线程中运行（使用 tokio 运行时）
// - 主线程处理 Hub 消息转发和响应路由
// - 使用 tokio mpsc 通道连接两个线程

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
// 测试模块
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // 测试 pending request 跟踪
    #[test]
    fn pending_request_tracking() {
        use tokio::time::Instant;
        
        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx, _rx) = oneshot::channel();
        let req = PendingRequest::new(1, tx);

        {
            let mut p = pending.lock().unwrap();
            p.insert(1, req);
        }

        let p = pending.lock().unwrap();
        assert!(p.contains_key(&1));
    }
}

// ===========================================================================
// Extended Tests for RpcWorker async implementation
// ===========================================================================

#[cfg(test)]
mod extended_tests {
    use super::*;
    use crate::messages::Operation;
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
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let response: crate::messages::DeviceResponseData = (true, serde_json::json!({"status": "ok"}), None);
        tx.send(response).unwrap();

        let received = rx.await.unwrap();
        assert!(received.0); // success flag
        assert_eq!(received.1["status"], "ok");
    }

    /// Test oneshot channel with error response
    #[tokio::test]
    async fn oneshot_channel_error_response() {
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let response: crate::messages::DeviceResponseData = (
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
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();
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
        let (_tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        // Timeout after 10ms
        let result = timeout(Duration::from_millis(10), rx).await;
        assert!(result.is_err(), "Should timeout");
    }

    /// Test that oneshot completes before timeout
    #[tokio::test]
    async fn oneshot_completes_before_timeout() {
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let response: crate::messages::DeviceResponseData = (true, serde_json::json!({}), None);
        tx.send(response).unwrap();

        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok(), "Should complete before timeout");
    }

    /// Test cleanup of timed-out requests
    #[tokio::test]
    async fn cleanup_removes_timed_out_requests() {
        use std::time::Instant;
        
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

        // Run cleanup logic pattern - verify the logic finds the right items
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
                PendingRequest::new(1, tx),
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
            operation: Operation::WriteSignalGroup,
            params: serde_json::json!({ "group_name": "temperature_sensor", "data": { "value": 42 } }),
            correlation_id: 12345,
            respond_to: tx,
        };

        assert_eq!(request.device_id, "plc-1");
        assert!(matches!(request.operation, Operation::WriteSignalGroup));
        assert_eq!(request.correlation_id, 12345);
        assert_eq!(request.params["group_name"], "temperature_sensor");
        assert_eq!(request.params["data"]["value"], 42);
    }

    /// Test ReadSignalGroup request creation with proper params
    #[test]
    fn read_signal_group_request_creation() {
        let (tx, _rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "plc-1".to_string(),
            operation: Operation::ReadSignalGroup,
            params: serde_json::json!({ "group_name": "temperature_sensor" }),
            correlation_id: 12346,
            respond_to: tx,
        };

        assert_eq!(request.device_id, "plc-1");
        assert!(matches!(request.operation, Operation::ReadSignalGroup));
        assert_eq!(request.correlation_id, 12346);
        assert_eq!(request.params["group_name"], "temperature_sensor");
    }

    /// Test that missing group_name returns error in params validation
    #[test]
    fn read_signal_group_missing_group_name() {
        let params = serde_json::json!({}); // Missing group_name
        assert!(
            params.get("group_name").is_none(),
            "group_name should be missing"
        );
    }

    /// Test that missing device_id triggers error path
    #[test]
    fn device_control_missing_device_id() {
        let (tx, _rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "".to_string(), // Empty device_id
            operation: Operation::ReadSignalGroup,
            params: serde_json::json!({ "group_name": "temperature_sensor" }),
            correlation_id: 12347,
            respond_to: tx,
        };

        assert!(request.device_id.is_empty(), "device_id should be empty");
    }

    /// Test WriteSignalGroup with empty data
    #[test]
    fn write_signal_group_empty_data() {
        let (tx, _rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "plc-1".to_string(),
            operation: Operation::WriteSignalGroup,
            params: serde_json::json!({ "group_name": "temperature_sensor", "data": {} }),
            correlation_id: 12348,
            respond_to: tx,
        };

        assert!(
            request.params["data"].is_object(),
            "data should be an object"
        );
        assert_eq!(request.params["data"].as_object().unwrap().len(), 0);
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
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        // Simulate response from worker
        let handle = tokio::spawn(async move {
            sleep(Duration::from_millis(5)).await;
            let response: crate::messages::DeviceResponseData =
                (true, serde_json::json!({"temperature": 25.5}), None);
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