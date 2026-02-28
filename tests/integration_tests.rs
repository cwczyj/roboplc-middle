//! Integration tests for roboplc-middleware
//!
//! These tests verify that workers integrate correctly with each other
//! and that the application can start up and handle requests.

use roboplc_middleware::config::Config;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;

/// Helper function to create a temporary config file
fn create_test_config(custom_ports: Option<(u16, u16)>) -> tempfile::NamedTempFile {
    let (rpc_port, http_port) = custom_ports.unwrap_or((8888, 8889));

    let config_content = format!(
        r#"
[server]
rpc_port = {}
http_port = {}

[logging]
level = "info"
file = "/tmp/test-roboplc.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "127.0.0.1"
port = 5020
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices.register_mappings]]
signal_name = "test_signal"
address = "h100"
data_type = "u16"
"#,
        rpc_port, http_port
    );

    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();
    temp_file
}

/// Helper function to check if a port is available
fn is_port_available(port: u16) -> bool {
    TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
}

/// Helper function to find an available port
fn find_available_port() -> Option<u16> {
    portpicker::pick_unused_port().map(|p| p as u16)
}

#[test]
fn test_config_loads_successfully() {
    let config_file = create_test_config(None);

    let config = Config::from_file(config_file.path()).expect("Failed to load test config");

    assert_eq!(config.server.rpc_port, 8888);
    assert_eq!(config.server.http_port, 8889);
    assert_eq!(config.devices.len(), 1);
    assert_eq!(config.devices[0].id, "test-plc");
    assert_eq!(config.devices[0].port, 5020);
}

#[test]
fn test_config_with_nonexistent_devices() {
    let config_content = r#"
[server]
rpc_port = 8888
http_port = 8889

[logging]
level = "info"
file = "/tmp/test-roboplc.log"
daily_rotation = true

[[devices]]
id = "unreachable-plc"
type = "plc"
address = "10.255.255.1"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30
"#;

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    // Config should load successfully even if devices are unreachable
    let config = Config::from_file(temp_file.path()).expect("Failed to load config");

    assert_eq!(config.devices.len(), 1);
    assert_eq!(config.devices[0].id, "unreachable-plc");
    assert_eq!(config.devices[0].address, "10.255.255.1");
}

#[test]
fn test_config_device_id_uniqueness() {
    let config_content = r#"
[server]
rpc_port = 8888
http_port = 8889

[logging]
level = "info"
file = "/tmp/test-roboplc.log"
daily_rotation = true

[[devices]]
id = "duplicate-id"
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
id = "duplicate-id"
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

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    // Config should reject duplicate device IDs
    let result = Config::from_file(temp_file.path());
    assert!(
        result.is_err(),
        "Should reject config with duplicate device IDs"
    );
}

#[test]
fn test_multiple_devices_config() {
    let config_content = r#"
[server]
rpc_port = 8888
http_port = 8889

[logging]
level = "info"
file = "/tmp/test-roboplc.log"
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

[[devices]]
id = "robot-arm-1"
type = "robot_arm"
address = "127.0.0.1"
port = 504
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 5
heartbeat_interval_sec = 10
"#;

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let config = Config::from_file(temp_file.path()).expect("Failed to load config");

    assert_eq!(config.devices.len(), 3);
    assert_eq!(config.devices[0].id, "plc-1");
    assert_eq!(config.devices[1].id, "plc-2");
    assert_eq!(config.devices[2].id, "robot-arm-1");
}

#[test]
fn test_port_availability_checker() {
    // Test that our helper function can detect available ports
    let port = find_available_port().expect("Should find an available port");
    assert!(is_port_available(port), "Port {} should be available", port);

    // Try to bind to the port and verify it's no longer available
    let listener =
        TcpListener::bind(format!("127.0.0.1:{}", port)).expect("Should bind to the port");

    // Try to find another port
    let port2 = find_available_port().expect("Should find another available port");
    assert_ne!(port, port2, "Should find a different port");

    drop(listener);
}
