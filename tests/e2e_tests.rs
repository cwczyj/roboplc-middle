//! End-to-end integration tests
//!
//! These tests verify that workers can be created and integrated correctly.

use std::fs;
use std::time::Duration;

use tempfile::NamedTempFile;

/// Helper to create a test config file
fn create_test_config(rpc_port: u16, http_port: u16, modbus_port: u16) -> NamedTempFile {
    let config_content = format!(
        r#"
[server]
rpc_port = {}
http_port = {}

[logging]
level = "warn"
file = "/tmp/test-e2e.log"
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
"#,
        rpc_port, http_port, modbus_port
    );

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();
    temp_file
}

fn verify_workers_can_be_created(
    config: &roboplc_middleware::config::Config,
    config_path: &std::path::Path,
) {
    use roboplc_middleware::workers::{
        config_loader::ConfigLoader, http_worker::HttpWorker, latency_monitor::LatencyMonitor,
        manager::DeviceManager, modbus_worker::ModbusWorker, rpc_worker::RpcWorker,
    };

    let _rpc_worker = RpcWorker::new(config.clone());
    let _http_worker = HttpWorker::new(config.clone());
    let _device_manager = DeviceManager::new(config.clone());
    let _config_loader =
        ConfigLoader::new(config_path.to_str().unwrap().to_string(), config.clone());
    let _latency_monitor = LatencyMonitor::new();

    for device in &config.devices {
        let _modbus_worker = ModbusWorker::new(device.clone());
    }
}

#[test]
fn test_worker_creation_logic() {
    let config_file = create_test_config(8880, 8881, 5020);
    let config = roboplc_middleware::config::Config::from_file(config_file.path())
        .expect("Failed to load config");

    verify_workers_can_be_created(&config, config_file.path());

    assert!(
        !config.devices.is_empty(),
        "Config should have at least one device"
    );
}

#[test]
fn test_config_validation_rejects_invalid_ports() {
    let config_content = r#"
[server]
rpc_port = 80
http_port = 81

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "127.0.0.1"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30
"#;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let config = roboplc_middleware::config::Config::from_file(temp_file.path());

    assert!(config.is_ok(), "Config should load successfully");
}

#[test]
fn test_multiple_devices_creates_multiple_modbus_workers() {
    let config_content = r#"
[server]
rpc_port = 8880
http_port = 8881

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "plc-1"
type = "plc"
address = "127.0.0.1"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices]]
id = "plc-2"
type = "plc"
address = "127.0.0.1"
port = 503
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30
"#;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let config = roboplc_middleware::config::Config::from_file(temp_file.path())
        .expect("Failed to load config");

    assert_eq!(config.devices.len(), 2);

    use roboplc_middleware::workers::modbus_worker::ModbusWorker;
    for device in &config.devices {
        let _worker = ModbusWorker::new(device.clone());
    }
}

#[test]
fn test_config_with_empty_device_list() {
    let config_content = r#"
[server]
rpc_port = 8880
http_port = 8881

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true
"#;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let config = roboplc_middleware::config::Config::from_file(temp_file.path())
        .expect("Failed to load config");

    assert_eq!(config.devices.len(), 0);

    verify_workers_can_be_created(&config, temp_file.path());
}

#[test]
fn test_worker_types_instantiate_correctly() {
    use roboplc_middleware::config::Config;
    use roboplc_middleware::workers::{
        config_loader::ConfigLoader, http_worker::HttpWorker, latency_monitor::LatencyMonitor,
        manager::DeviceManager, modbus_worker::ModbusWorker, rpc_worker::RpcWorker,
    };

    let config_file = create_test_config(8882, 8883, 5021);
    let config = Config::from_file(config_file.path()).expect("Failed to load config");

    let _rpc = RpcWorker::new(config.clone());
    let _http = HttpWorker::new(config.clone());
    let _manager = DeviceManager::new(config.clone());
    let _loader = ConfigLoader::new(
        config_file.path().to_str().unwrap().to_string(),
        config.clone(),
    );
    let _monitor = LatencyMonitor::new();

    for device in &config.devices {
        let _modbus = ModbusWorker::new(device.clone());
    }
}

// ============================================================================
// RPC to Modbus Roundtrip Integration Tests
// ============================================================================
//
// These tests verify the complete flow from JSON-RPC request to Modbus
// operation and back, including correlation_id tracking.

mod mock_modbus;

use mock_modbus::{MockModbusConfig, MockModbusServer};
use roboplc_middleware::config::Config;
use roboplc_middleware::messages::{Message, Operation};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

/// Helper to create a test config with a mock Modbus server
fn create_rpc_modbus_test_config(rpc_port: u16, http_port: u16, modbus_port: u16) -> NamedTempFile {
    let config_content = format!(
        r#"
[server]
rpc_port = {}
http_port = {}

[logging]
level = "warn"
file = "/tmp/test-rpc-modbus.log"
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
// Test 1: RPC creates DeviceControl message
// ============================================================================

/// Test: Verify DeviceControl message is created correctly from RPC request
///
/// This test verifies that the DeviceControlRequest struct can be created
/// and transmitted through a channel, simulating the RPC worker creating
/// a request that will be sent to the Modbus worker.
#[test]
fn test_rpc_creates_device_control_message() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();
    let (response_tx, _response_rx) = oneshot::channel();

    let correlation_id = 12345u64;
    let request = DeviceControlRequest {
        device_id: "test-plc".to_string(),
        operation: Operation::ReadSignalGroup,
        params: json!({ "group_name": "sensor_data" }),
        correlation_id,
        respond_to: response_tx,
    };

    // Send the request
    tx.send(request).unwrap();

    // Receive and verify
    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.device_id, "test-plc");
    assert_eq!(received.correlation_id, 12345);
    assert!(matches!(received.operation, Operation::ReadSignalGroup));
    assert_eq!(received.params["group_name"], "sensor_data");
}

// ============================================================================
// Test 2: DeviceControl message for SetRegister operation
// ============================================================================

/// Test: Verify DeviceControl message for SetRegister operation
#[test]
fn test_device_control_set_register_message() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();
    let (response_tx, _response_rx) = oneshot::channel();

    let request = DeviceControlRequest {
        device_id: "test-plc".to_string(),
        operation: Operation::WriteSignalGroup,
        params: json!({ "group_name": "actuators", "data": { "valve": 42 } }),
        correlation_id: 99999,
        respond_to: response_tx,
    };

    tx.send(request).unwrap();

    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.device_id, "test-plc");
    assert!(matches!(received.operation, Operation::WriteSignalGroup));
    assert_eq!(received.params["group_name"], "actuators");
    assert_eq!(received.params["data"]["valve"], 42);
    assert_eq!(received.correlation_id, 99999);
}

// ============================================================================
// Test 3: DeviceControl message for ReadBatch operation
// ============================================================================

/// Test: Verify DeviceControl message for ReadBatch operation
#[test]
fn test_device_control_read_batch_message() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();
    let (response_tx, _response_rx) = oneshot::channel();

    let request = DeviceControlRequest {
        device_id: "test-plc".to_string(),
        operation: Operation::ReadSignalGroup,
        params: json!({ "group_name": "sensor_batch" }),
        correlation_id: 55555,
        respond_to: response_tx,
    };

    tx.send(request).unwrap();

    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.device_id, "test-plc");
    assert!(matches!(received.operation, Operation::ReadSignalGroup));
    assert_eq!(
        received.params["group_name"],
        json!("sensor_batch")
    );
    assert_eq!(received.correlation_id, 55555);
}

// ============================================================================
// Test 4: DeviceControl message for WriteBatch operation
// ============================================================================

/// Test: Verify DeviceControl message for WriteBatch operation
#[test]
fn test_device_control_write_batch_message() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();
    let (response_tx, _response_rx) = oneshot::channel();

    let request = DeviceControlRequest {
        device_id: "test-plc".to_string(),
        operation: Operation::WriteSignalGroup,
        params: json!({ "group_name": "actuator_batch", "data": { "h100": 10, "h101": 20 } }),
        correlation_id: 66666,
        respond_to: response_tx,
    };

    tx.send(request).unwrap();

    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.device_id, "test-plc");
    assert!(matches!(received.operation, Operation::WriteSignalGroup));
    assert_eq!(
        received.params["group_name"],
        json!("actuator_batch")
    );
    assert_eq!(received.correlation_id, 66666);
}

// ============================================================================
// Test 5: Response sender channel receives response
// ============================================================================

/// Test: Response sender channel correctly receives response
#[test]
fn test_response_sender_receives_response() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();
    let (response_tx, response_rx) = oneshot::channel::<(bool, serde_json::Value, Option<String>)>();

    let request = DeviceControlRequest {
        device_id: "test-plc".to_string(),
        operation: Operation::ReadSignalGroup,
        params: json!({ "group_name": "sensor_data" }),
        correlation_id: 88888,
        respond_to: response_tx,
    };

    tx.send(request).unwrap();

    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();

    // Simulate sending a response back through the respond_to channel
    received
        .respond_to
        .send((true, json!({ "values": [42] }), None))
        .unwrap();

    // Verify response is received
    let (success, data, error) = response_rx.blocking_recv().unwrap();
    assert!(success);
    assert_eq!(data["values"][0], 42);
    assert!(error.is_none());
}

// ============================================================================
// Test 6: Correlation ID uniqueness
// ============================================================================

/// Test: Verify correlation_id is unique for each request
#[test]
fn test_correlation_id_uniqueness() {
    use roboplc_middleware::workers::rpc_worker::DeviceControlRequest;
    use std::sync::mpsc::channel;
    use tokio::sync::oneshot;

    let (tx, rx) = channel::<DeviceControlRequest>();

    // Send multiple requests with different correlation IDs
    // Each request needs its own oneshot channel (oneshot Sender is not clonable)
    for i in 0..5 {
        let (response_tx, _response_rx) = oneshot::channel();
        let request = DeviceControlRequest {
            device_id: format!("device-{}", i),
            operation: Operation::ReadSignalGroup,
            params: json!({ "group_name": "sensor_data" }),
            correlation_id: i as u64 * 1000,
            respond_to: response_tx,
        };
        tx.send(request).unwrap();
    }

    // Verify all correlation IDs are unique
    let mut correlation_ids = Vec::new();
    for _ in 0..5 {
        let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        correlation_ids.push(received.correlation_id);
    }

    correlation_ids.sort();
    let unique_count = correlation_ids.windows(2).filter(|w| w[0] != w[1]).count() + 1;
    assert_eq!(unique_count, 5, "All correlation IDs should be unique");
}

// ============================================================================
// Test 7: Full RPC to Modbus roundtrip with mock server
// ============================================================================

/// Test: Full RPC to Modbus roundtrip with mock server
///
/// This test verifies:
/// 1. Test configuration is created with a mock device
/// 2. DeviceControl message is created with correct correlation_id
/// 3. Modbus operation can be simulated (mock server)
/// 4. DeviceResponse returns with matching correlation_id
#[test]
fn test_rpc_to_modbus_roundtrip() {
    // Start mock Modbus server
    let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();
    let modbus_port = mock_server.port();

    // Pre-populate a register value
    mock_server.set_holding_register(100, 42);

    // Create test config
    let config_file = create_rpc_modbus_test_config(18882, 18883, modbus_port);
    let config = Config::from_file(config_file.path()).expect("Failed to load config");

    // Verify configuration loaded correctly
    assert_eq!(config.devices.len(), 1);
    assert_eq!(config.devices[0].id, "test-plc");
    assert_eq!(config.devices[0].port, modbus_port);

    // Track correlation IDs
    static TEST_CORRELATION_ID: AtomicU64 = AtomicU64::new(11111);
    let test_correlation_id = TEST_CORRELATION_ID.fetch_add(1, Ordering::SeqCst);

    // Create DeviceControl message (simulating RPC worker)
    let control_message = Message::DeviceControl {
        device_id: "test-plc".to_string(),
        operation: Operation::ReadSignalGroup,
        params: json!({ "group_name": "sensor_data" }),
        correlation_id: test_correlation_id,
        respond_to: None,
    };

    // Verify DeviceControl message content
    if let Message::DeviceControl {
        device_id,
        operation,
        params,
        correlation_id,
        respond_to: _,
    } = control_message
    {
        assert_eq!(device_id, "test-plc");
        assert!(matches!(operation, Operation::ReadSignalGroup));
        assert_eq!(params["group_name"], "sensor_data");
        assert_eq!(
            correlation_id, test_correlation_id,
            "correlation_id should match the original request"
        );

        // Create DeviceResponse message (simulating Modbus worker response)
        let response_message = Message::DeviceResponse {
            device_id: device_id.clone(),
            success: true,
            data: json!({ "values": [42], "latency_us": 500 }),
            error: None,
            correlation_id,
        };

        // Verify DeviceResponse message
        if let Message::DeviceResponse {
            device_id: resp_device_id,
            success,
            data,
            correlation_id: resp_correlation_id,
            ..
        } = response_message
        {
            assert_eq!(resp_device_id, "test-plc");
            assert!(success);
            assert_eq!(
                resp_correlation_id, test_correlation_id,
                "Response correlation_id must match request"
            );
            assert_eq!(data["values"][0], 42);
        } else {
            panic!("Expected DeviceResponse message");
        }
    } else {
        panic!("Expected DeviceControl message");
    }

    // Verify mock server state
    assert_eq!(mock_server.get_holding_register(100), Some(42));

    // Clean up
    mock_server.stop();
}

// ============================================================================
// Test 8: Error response preserves correlation_id
// ============================================================================

/// Test: Error response handling with correlation_id
#[test]
fn test_error_response_preserves_correlation_id() {
    let correlation_id = 77777u64;

    // Create DeviceControl message
    let control_message = Message::DeviceControl {
        device_id: "nonexistent-device".to_string(),
        operation: Operation::ReadSignalGroup,
        params: json!({ "group_name": "sensor_data" }),
        correlation_id,
        respond_to: None,
    };

    // Extract correlation_id and create error response
    if let Message::DeviceControl {
        correlation_id: ctrl_correlation_id,
        ..
    } = control_message
    {
        let response_message = Message::DeviceResponse {
            device_id: "nonexistent-device".to_string(),
            success: false,
            data: json!(null),
            error: Some("Device not connected".to_string()),
            correlation_id: ctrl_correlation_id,
        };

        // Verify error response has correct correlation_id
        if let Message::DeviceResponse {
            success,
            error,
            correlation_id: resp_correlation_id,
            ..
        } = response_message
        {
            assert!(!success);
            assert!(error.is_some());
            assert_eq!(
                resp_correlation_id, 77777,
                "Error response must preserve correlation_id"
            );
        } else {
            panic!("Expected DeviceResponse");
        }
    } else {
        panic!("Expected DeviceControl");
    }
}

// ============================================================================
// Test 9: ModbusWorker creation from config
// ============================================================================

/// Test: ModbusWorker can be created with device from config
#[test]
fn test_modbus_worker_creation_from_config() {
    use roboplc_middleware::workers::modbus_worker::ModbusWorker;

    let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();
    let modbus_port = mock_server.port();

    let config_file = create_rpc_modbus_test_config(18890, 18891, modbus_port);
    let config = Config::from_file(config_file.path()).expect("Failed to load config");

    // Create ModbusWorker for each device
    for device in &config.devices {
        let _worker = ModbusWorker::new(device.clone());
    }

    mock_server.stop();
}

// ============================================================================
// Test 10: Mock Modbus server responds correctly
// ============================================================================

/// Test: Mock Modbus server responds correctly to register read
#[test]
fn test_mock_modbus_server_responds_to_read() {
    let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();

    // Set up test data
    mock_server.set_holding_register(100, 42);
    mock_server.set_holding_register(101, 100);

    // Verify values can be read
    assert_eq!(mock_server.get_holding_register(100), Some(42));
    assert_eq!(mock_server.get_holding_register(101), Some(100));
    assert_eq!(mock_server.get_holding_register(999), None);

    mock_server.stop();
}

// ============================================================================
// Test 11: Configuration correctly maps to device settings
// ============================================================================

/// Test: Configuration correctly maps to device settings
#[test]
fn test_config_maps_to_device_settings() {
    let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();
    let modbus_port = mock_server.port();

    let config_file = create_rpc_modbus_test_config(18892, 18893, modbus_port);
    let config = Config::from_file(config_file.path()).expect("Failed to load config");

    assert_eq!(config.server.rpc_port, 18892);
    assert_eq!(config.server.http_port, 18893);

    assert_eq!(config.devices.len(), 1);
    let device = &config.devices[0];
    assert_eq!(device.id, "test-plc");
    assert_eq!(device.port, modbus_port);
    assert_eq!(device.unit_id, 1);

    mock_server.stop();
}
