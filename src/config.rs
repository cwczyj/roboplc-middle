use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: Server,
    pub logging: Logging,
    #[serde(default)]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub rpc_port: u16,
    pub http_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    pub level: String,
    pub file: String,
    pub daily_rotation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    pub address: String,
    pub port: u16,
    pub unit_id: u8,
    #[serde(default)]
    pub addressing_mode: AddressingMode,
    #[serde(default)]
    pub byte_order: ByteOrder,
    #[serde(default = "default_tcp_nodelay")]
    pub tcp_nodelay: bool,
    #[serde(default = "default_max_concurrent_ops")]
    pub max_concurrent_ops: u8,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_sec: u32,
    #[serde(default)]
    pub register_mappings: Vec<RegisterMapping>,
}

fn default_tcp_nodelay() -> bool {
    true
}
fn default_max_concurrent_ops() -> u8 {
    3
}
fn default_heartbeat_interval() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    #[default]
    Plc,
    RobotArm,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AddressingMode {
    #[default]
    ZeroBased,
    OneBased,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    #[default]
    BigEndian,
    LittleEndian,
    LittleEndianByteSwap,
    MidBig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMapping {
    pub signal_name: String,
    pub address: String,
    #[serde(default)]
    pub data_type: DataType,
    #[serde(default)]
    pub access: AccessMode,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    #[default]
    U16,
    U32,
    I16,
    I32,
    F32,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Rw,
    Read,
    Write,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Duplicate device ID: {0}")]
    DuplicateDeviceId(String),
    #[error("Invalid port: {0}")]
    InvalidPort(u16),
    #[error("Invalid address format for device '{0}' register '{1}': {2}")]
    InvalidAddressFormat(String, String, String),
    #[error("Address out of range for device '{0}' register '{1}': {2}")]
    AddressOutOfRange(String, String, u32),
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut seen_ids = HashSet::new();
        for device in &self.devices {
            if !seen_ids.insert(&device.id) {
                return Err(ConfigError::DuplicateDeviceId(device.id.clone()));
            }

            for mapping in &device.register_mappings {
                let addr = mapping.address.trim();
                if addr.len() < 2 {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                let prefix = &addr[0..1].to_lowercase();
                let num_str = &addr[1..];

                if !matches!(prefix.as_str(), "h" | "d" | "c" | "i") {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }

                if let Ok(num) = num_str.parse::<u32>() {
                    if num > 65535 {
                        return Err(ConfigError::AddressOutOfRange(
                            device.id.clone(),
                            mapping.signal_name.clone(),
                            num,
                        ));
                    }
                } else {
                    return Err(ConfigError::InvalidAddressFormat(
                        device.id.clone(),
                        mapping.signal_name.clone(),
                        addr.to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config_str = r#"
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
"#;
        let config: Config = toml::from_str(config_str).unwrap();
        assert_eq!(config.server.rpc_port, 8080);
        assert_eq!(config.devices.len(), 0);
    }
}
