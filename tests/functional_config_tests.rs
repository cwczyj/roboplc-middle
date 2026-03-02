//! Functional tests for configuration loading and validation

use roboplc_middleware::config::{Config, ConfigError};
use std::fs;
use tempfile::NamedTempFile;

fn create_temp_config(content: &str) -> NamedTempFile {
    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), content).unwrap();
    temp_file
}

#[test]
fn config_loads_valid_toml() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.server.rpc_port, 8080);
    assert_eq!(config.server.http_port, 8081);
    assert_eq!(config.devices.len(), 1);
}

#[test]
fn config_rejects_duplicate_device_ids() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "same-id"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1

[[devices]]
id = "same-id"
type = "plc"
address = "192.168.1.101"
port = 502
unit_id = 1
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_err());
    match result {
        Err(ConfigError::DuplicateDeviceId(id)) => assert_eq!(id, "same-id"),
        _ => panic!("Expected DuplicateDeviceId error"),
    }
}

#[test]
fn config_supports_multiple_devices() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "plc-1"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1

[[devices]]
id = "robot-1"
type = "robot_arm"
address = "192.168.1.200"
port = 502
unit_id = 1
"#;
    let temp_file = create_temp_config(config_content);
    let config = Config::from_file(temp_file.path()).unwrap();

    assert_eq!(config.devices.len(), 2);
    assert_eq!(config.devices[0].id, "plc-1");
    assert_eq!(config.devices[1].id, "robot-1");
}

#[test]
fn config_accepts_empty_device_list() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true
"#;
    let temp_file = create_temp_config(config_content);
    let config = Config::from_file(temp_file.path()).unwrap();

    assert_eq!(config.devices.len(), 0);
}

#[test]
fn config_validates_address_format() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1

[[devices.register_mappings]]
signal_name = "test"
address = "h100"
data_type = "u16"
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_ok());
}

#[test]
fn config_rejects_invalid_address_format() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1

[[devices.register_mappings]]
signal_name = "test"
address = "x100"
data_type = "u16"
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn config_rejects_address_out_of_range() {
    let config_content = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/test.log"
daily_rotation = true

[[devices]]
id = "test-plc"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1

[[devices.register_mappings]]
signal_name = "test"
address = "h70000"
data_type = "u16"
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn config_missing_file_returns_error() {
    let result = Config::from_file("/nonexistent/path/config.toml");
    assert!(result.is_err());
}

#[test]
fn config_invalid_toml_returns_parse_error() {
    let config_content = r#"
[server
rpc_port = 8080
"#;
    let temp_file = create_temp_config(config_content);
    let result = Config::from_file(temp_file.path());

    assert!(result.is_err());
}
