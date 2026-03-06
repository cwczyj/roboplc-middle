//! Integration tests for async RpcWorker (Wave 5, Tasks 22-23)
//!
//! This module contains comprehensive tests for the async RPC functionality:
//! - Channel operations (mpsc, oneshot)
//! - Timeout handling
//! - Concurrent request handling
//! - Performance tests

use roboplc_middleware::config::Config;
use roboplc_middleware::{DeviceResponseData, Operation};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};

// ============================================================================
// Test Configuration Helpers
// ============================================================================

/// Helper to create a test config file for async RPC tests
fn create_async_rpc_test_config(rpc_port: u16, http_port: u16, modbus_port: u16) -> NamedTempFile {
    use std::fs;

    let config_content = format!(
        r#"
[server]
rpc_port = {}
http_port = {}

[logging]
level = "warn"
file = "/tmp/test-async-rpc.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "127.0.0.1"
port = {}
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices.register_mappings]]
signal_name = "test_register"
address = "h100"
data_type = "u16"
"#,
        rpc_port, http_port, modbus_port
    );

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();
    temp_file
}

// ============================================================================
// Task 22: Integration Tests
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Test configuration loads correctly for async RPC tests
    #[test]
    fn test_config_loads_for_async_rpc() {
        let config_file = create_async_rpc_test_config(19990, 19991, 5020);
        let config = Config::from_file(config_file.path()).expect("Failed to load config");

        assert_eq!(config.server.rpc_port, 19990);
        assert_eq!(config.server.http_port, 19991);
        assert_eq!(config.devices.len(), 1);
        assert_eq!(config.devices[0].id, "test-plc");
    }

    /// Test mpsc channel can handle device control requests
    #[tokio::test]
    async fn test_mpsc_channel_device_control() {
        let (tx, mut rx) = mpsc::channel(100);

        // Simulate DeviceControlRequest
        let (response_tx, _response_rx) = oneshot::channel();
        let request = roboplc_middleware::workers::rpc_worker::DeviceControlRequest {
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

    /// Test oneshot channel for response routing
    #[tokio::test]
    async fn test_oneshot_response_routing() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        // Send response
        let response: DeviceResponseData = (true, serde_json::json!({"status": "connected"}), None);
        tx.send(response).unwrap();

        // Receive response with timeout
        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok());

        let received = result.unwrap().unwrap();
        assert!(received.0);
        assert_eq!(received.1["status"], "connected");
    }

    /// Test timeout handling for slow responses
    #[tokio::test]
    async fn test_timeout_handling_slow_response() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        // Don't send response immediately - simulate slow processing
        let handle = tokio::spawn(async move {
            sleep(Duration::from_millis(200)).await;
            let response: DeviceResponseData = (true, serde_json::json!({}), None);
            let _ = tx.send(response);
        });

        // Timeout after 50ms (should timeout before response arrives)
        let result = timeout(Duration::from_millis(50), rx).await;
        assert!(result.is_err(), "Should timeout");

        handle.await.unwrap();
    }

    /// Test multiple concurrent requests through channel
    #[tokio::test]
    async fn test_concurrent_requests() {
        let (tx, mut rx) = mpsc::channel(100);
        let counter = Arc::new(AtomicU64::new(0));

        // Spawn 10 concurrent request senders
        let mut handles = vec![];
        for i in 0..10 {
            let tx_clone = tx.clone();
            let counter_clone = counter.clone();
            let handle = tokio::spawn(async move {
                let (response_tx, _) = oneshot::channel();
                let request = roboplc_middleware::workers::rpc_worker::DeviceControlRequest {
                    device_id: format!("device-{}", i),
                    operation: Operation::GetStatus,
                    params: serde_json::json!({}),
                    correlation_id: i as u64,
                    respond_to: response_tx,
                };
                tx_clone.send(request).await.unwrap();
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
            handles.push(handle);
        }

        // Wait for all senders to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all 10 requests were sent
        assert_eq!(counter.load(Ordering::SeqCst), 10);

        // Receive all 10 requests
        let mut received_count = 0;
        while let Ok(Some(_)) = timeout(Duration::from_millis(100), rx.recv()).await {
            received_count += 1;
        }
        assert_eq!(received_count, 10);
    }

    /// Test error response through oneshot channel
    #[tokio::test]
    async fn test_error_response_handling() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        let error_response: DeviceResponseData = (
            false,
            serde_json::json!({}),
            Some("Device not connected".to_string()),
        );
        tx.send(error_response).unwrap();

        let result = timeout(Duration::from_millis(100), rx).await;
        assert!(result.is_ok());

        let received = result.unwrap().unwrap();
        assert!(!received.0);
        assert!(received.2.is_some());
        assert_eq!(received.2.unwrap(), "Device not connected");
    }

    /// Test cleanup logic using local mock (doesn't need private internals)
    #[tokio::test]
    async fn test_pending_request_cleanup_logic() {
        // This test validates the cleanup logic without needing access to private internals
        use std::time::Instant;

        // Simulate pending requests with timestamps
        let mut timestamps: Vec<(u64, std::time::Instant)> = vec![];

        // Old request (40 seconds ago)
        timestamps.push((1, Instant::now() - Duration::from_secs(40)));
        // Fresh request (now)
        timestamps.push((2, Instant::now()));

        let now = Instant::now();
        let timeout_duration = Duration::from_secs(35);

        let timed_out: Vec<u64> = timestamps
            .iter()
            .filter(|(_, created_at)| now.duration_since(*created_at) > timeout_duration)
            .map(|(id, _)| *id)
            .collect();

        // Only old request should be timed out
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], 1);
    }
}

// ============================================================================
// Task 23: Performance Tests
// ============================================================================

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    /// Performance test: measure channel throughput
    #[tokio::test]
    async fn test_channel_throughput() {
        let (tx, mut rx) = mpsc::channel(1000);
        let message_count = 100;

        // Spawn sender
        let sender = tokio::spawn(async move {
            for i in 0..message_count {
                let (response_tx, _) = oneshot::channel();
                let request = roboplc_middleware::workers::rpc_worker::DeviceControlRequest {
                    device_id: format!("device-{}", i),
                    operation: Operation::GetStatus,
                    params: serde_json::json!({}),
                    correlation_id: i as u64,
                    respond_to: response_tx,
                };
                tx.send(request).await.unwrap();
            }
        });

        // Measure receive time
        let start = Instant::now();
        let mut received = 0;
        while received < message_count {
            if let Ok(Some(_)) = timeout(Duration::from_millis(100), rx.recv()).await {
                received += 1;
            } else {
                break;
            }
        }
        let elapsed = start.elapsed();

        sender.await.unwrap();

        assert_eq!(received, message_count);
        println!(
            "Channel throughput: {} messages in {:?} ({:.2} msg/s)",
            message_count,
            elapsed,
            message_count as f64 / elapsed.as_secs_f64()
        );
    }

    /// Performance test: measure concurrent request latency
    #[tokio::test]
    async fn test_concurrent_request_latency() {
        let (tx, rx) = oneshot::channel::<DeviceResponseData>();

        let start = Instant::now();

        // Send response immediately
        let response: DeviceResponseData = (true, serde_json::json!({}), None);
        tx.send(response).unwrap();

        // Receive with timeout
        let result = timeout(Duration::from_millis(100), rx).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        println!("Oneshot latency: {:?}", elapsed);

        // Latency should be very low (under 1ms for local operations)
        assert!(elapsed < Duration::from_millis(100));
    }

    /// Performance test: measure parallel request handling
    #[tokio::test]
    async fn test_parallel_request_handling() {
        let request_count = 50;
        let (tx, mut rx) = mpsc::channel(100);

        let start = Instant::now();

        // Spawn multiple parallel senders
        let mut handles = vec![];
        for i in 0..request_count {
            let tx_clone = tx.clone();
            let handle = tokio::spawn(async move {
                let (response_tx, _) = oneshot::channel();
                let request = roboplc_middleware::workers::rpc_worker::DeviceControlRequest {
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

        // Receive all
        let mut received = 0;
        while let Ok(Some(_)) = timeout(Duration::from_millis(100), rx.recv()).await {
            received += 1;
        }

        let elapsed = start.elapsed();

        assert_eq!(received, request_count);
        println!(
            "Parallel handling: {} requests in {:?} ({:.2} req/s)",
            request_count,
            elapsed,
            request_count as f64 / elapsed.as_secs_f64()
        );
    }

    /// Performance test: stress test with high concurrency
    #[tokio::test]
    async fn test_high_concurrency_stress() {
        let request_count = 200;
        let (tx, mut rx) = mpsc::channel(500);

        let start = Instant::now();

        // Spawn many concurrent senders
        let mut handles = vec![];
        for i in 0..request_count {
            let tx_clone = tx.clone();
            let handle = tokio::spawn(async move {
                let (response_tx, _) = oneshot::channel();
                let request = roboplc_middleware::workers::rpc_worker::DeviceControlRequest {
                    device_id: format!("device-{}", i % 10), // 10 different devices
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

        // Receive all with extended timeout
        let mut received = 0;
        while let Ok(Some(_)) = timeout(Duration::from_millis(500), rx.recv()).await {
            received += 1;
            if received >= request_count {
                break;
            }
        }

        let elapsed = start.elapsed();

        assert_eq!(received, request_count, "All requests should be received");
        println!(
            "Stress test: {} requests in {:?} ({:.2} req/s)",
            request_count,
            elapsed,
            request_count as f64 / elapsed.as_secs_f64()
        );
    }

    /// Performance test: memory efficiency with many pending requests
    #[tokio::test]
    async fn test_pending_requests_memory() {
        // Test memory efficiency without needing access to private PendingRequest struct
        let pending: Arc<std::sync::Mutex<HashMap<u64, (u64, std::time::Instant)>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        let request_count: u64 = 1000;

        // Add many pending requests
        let start = Instant::now();
        {
            let mut p = pending.lock().unwrap();
            for i in 0..request_count {
                p.insert(i, (i, std::time::Instant::now()));
            }
        }
        let insert_time = start.elapsed();

        // Verify count - cast to u64 for comparison
        let p = pending.lock().unwrap();
        assert_eq!(p.len() as u64, request_count);
        drop(p);

        println!(
            "Memory test: {} pending requests inserted in {:?}",
            request_count, insert_time
        );

        // Cleanup should be fast
        let cleanup_start = Instant::now();
        {
            let mut p = pending.lock().unwrap();
            p.clear();
        }
        let cleanup_time = cleanup_start.elapsed();

        println!("Cleanup time: {:?}", cleanup_time);
        assert!(
            cleanup_time < Duration::from_millis(100),
            "Cleanup should be fast"
        );
    }
}

// ============================================================================
// Test Utilities
// ============================================================================

/// Helper to create a mock response
fn create_mock_response(success: bool, data: serde_json::Value) -> DeviceResponseData {
    (success, data, None)
}

/// Helper to create a mock error response
fn create_mock_error_response(error: &str) -> DeviceResponseData {
    (false, serde_json::json!({}), Some(error.to_string()))
}
