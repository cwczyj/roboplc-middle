// =============================================================================
// RPC Worker - JSON-RPC 服务器实现
// =============================================================================
// Wave 3 重构: 合并 spawn_blocking 调用
// - 移除中间 mpsc 通道 (请求直接通过 Hub 发送)
// - 每个请求只占用 1 个 blocking thread (之前是 2 个)
// - 简化 worker 实现

use crate::config::Config;
use crate::messages::Message;
use crate::Variables;

use roboplc::controller::prelude::*;

use tokio::sync::oneshot;
use std::thread::JoinHandle;

use super::server::run_async_server;

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

impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let port = self.config.server.rpc_port;
        let bind_addr = format!("0.0.0.0:{}", port);

        let device_ids: Vec<String> = self.config.devices.iter().map(|d| d.id.clone()).collect();

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let hub = context.hub().clone();
        let bind_addr_clone = bind_addr.clone();
        let device_ids_clone = device_ids.clone();

        let runtime_handle: JoinHandle<()> = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .max_blocking_threads(128)
                .enable_all()
                .build()
                .expect("RpcWorker: failed to create Tokio runtime");

            if let Err(e) = rt.block_on(async move {
                run_async_server(bind_addr_clone, device_ids_clone, hub, shutdown_rx).await
            }) {
                tracing::error!(error = %e, "RPC Server error");
            }

            rt.shutdown_timeout(std::time::Duration::from_secs(5));
        });

        tracing::info!("RPC Server Worker started");

        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        tracing::info!("RPC Server Worker shutting down...");

        let _ = shutdown_tx.send(());

        match runtime_handle.join() {
            Ok(()) => tracing::info!("RPC Server Worker stopped gracefully"),
            Err(_) => tracing::warn!("RPC Server thread panicked"),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::Operation;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::{sleep, timeout};

    use super::super::types::{DeviceControlRequest, PendingRequest};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn pending_request_tracking() {
        use tokio::time::Instant;

        let pending: Arc<Mutex<HashMap<u64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let req = PendingRequest {
            correlation_id: 1,
            created_at: Instant::now(),
            respond_to: oneshot::channel().0,
        };

        {
            let mut p = pending.lock().unwrap();
            p.insert(1, req);
        }

        let p = pending.lock().unwrap();
        assert!(p.contains_key(&1));
    }

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

        tx.send(request).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.device_id, "test-device");
        assert_eq!(received.correlation_id, 1);
    }

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

        for i in 0..5 {
            let received = rx.recv().await.unwrap();
            assert_eq!(received.correlation_id, i as u64);
        }
    }

    #[tokio::test]
    async fn oneshot_channel_basic() {
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let response: crate::messages::DeviceResponseData =
            (true, serde_json::json!({"status": "ok"}), None);
        tx.send(response).unwrap();

        let received = rx.await.unwrap();
        assert!(received.0);
        assert_eq!(received.1["status"], "ok");
    }

    #[tokio::test]
    async fn oneshot_timeout_detection() {
        let (_tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let result = timeout(Duration::from_millis(10), rx).await;
        assert!(result.is_err(), "Should timeout");
    }

    #[tokio::test]
    async fn oneshot_completes_before_timeout() {
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let response: crate::messages::DeviceResponseData = (true, serde_json::json!({}), None);
        tx.send(response).unwrap();

        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok(), "Should complete before timeout");
    }

    #[test]
    fn device_control_request_creation() {
        let (tx, _rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: "plc-1".to_string(),
            operation: Operation::WriteSignalGroup,
            params: serde_json::json!(
                { "group_name": "temperature_sensor", "data": { "value": 42 } }
            ),
            correlation_id: 12345,
            respond_to: tx,
        };

        assert_eq!(request.device_id, "plc-1");
        assert!(matches!(request.operation, Operation::WriteSignalGroup));
        assert_eq!(request.correlation_id, 12345);
    }

    #[tokio::test]
    async fn concurrent_requests_via_channel() {
        let (tx, mut rx) = mpsc::channel::<DeviceControlRequest>(100);

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

        for handle in handles {
            handle.await.unwrap();
        }

        let mut received_count = 0;
        while let Ok(Some(_)) = timeout(Duration::from_millis(100), rx.recv()).await {
            received_count += 1;
        }

        assert_eq!(received_count, 10);
    }

    #[tokio::test]
    async fn response_routing_via_oneshot() {
        let (tx, rx) = oneshot::channel::<crate::messages::DeviceResponseData>();

        let handle = tokio::spawn(async move {
            sleep(Duration::from_millis(5)).await;
            let response: crate::messages::DeviceResponseData =
                (true, serde_json::json!({"temperature": 25.5}), None);
            tx.send(response).unwrap();
        });

        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok());

        let response = result.unwrap().unwrap();
        assert!(response.0);
        assert_eq!(response.1["temperature"], 25.5);

        handle.await.unwrap();
    }
}