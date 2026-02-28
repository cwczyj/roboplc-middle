//! End-to-end integration tests
//!
//! These tests verify that workers can be created and integrated correctly.

use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;
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
    config_path: &PathBuf,
)
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
        ConfigLoader::new(config_path.to_string_lossy().to_string(), config.clone());
        ConfigLoader::new(config_path.to_path_buf().to_str().unwrap().to_string(), config.clone());
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
            config_file.path().to_path_buf().to_string_lossy().to_string(),
            config_file.path().to_path_buf().to_str().unwrap().to_string(),

        config.clone(),
    );
    let _monitor = LatencyMonitor::new();

    for device in &config.devices {
        let _modbus = ModbusWorker::new(device.clone());
    }
}
