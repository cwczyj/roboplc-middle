//! Modbus client for connection management and operations

use roboplc::comm::tcp;
use roboplc::comm::Client;
use roboplc::io::modbus::prelude::*;
use roboplc::io::IoMapping;
use serde_json::Value as JsonValue;
use std::time::{Duration, SystemTime};

// ==================== ModbusOp ====================

#[derive(Debug, Clone)]
pub enum ModbusOp {
    ReadCoil { address: u16, count: u16 },
    ReadDiscrete { address: u16, count: u16 },
    ReadInput { address: u16, count: u16 },
    ReadHolding { address: u16, count: u16 },
    WriteSingle { address: u16, value: u16 },
    WriteMultiple { address: u16, values: Vec<u16> },
}

/// Result of a Modbus operation
#[derive(Debug)]
pub struct OperationResult {
    pub success: bool,
    pub data: JsonValue,
    pub error: Option<String>,
}

/// Queued operation with tracking information
#[allow(dead_code)]
pub struct QueuedOperation {
    pub operation: ModbusOp,
    pub correlation_id: u64,
}

// ==================== ModbusClient ====================

pub struct ModbusClient {
    endpoint: String,
    connection: Option<Client>,
    unit_id: u8,
}

impl ModbusClient {
    pub fn new(endpoint: String, unit_id: u8) -> Self {
        Self {
            endpoint,
            connection: None,
            unit_id,
        }
    }

    pub fn connect(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let client = tcp::connect(&self.endpoint, timeout)?;
        client.connect()?;
        self.connection = Some(client);
        Ok(())
    }

    fn reconnect(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = &self.connection {
            client.reconnect();
        }
        self.connection = None;
        self.connect(timeout)
    }

    pub fn ensure_connected(
        &mut self,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match &self.connection {
            Some(client) => {
                if client.connect().is_err() {
                    self.reconnect(timeout)?;
                }
            }
            None => {
                self.connect(timeout)?;
            }
        }
        Ok(())
    }

    pub fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
        let client = match &self.connection {
            Some(c) => c.clone(),
            None => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Not connected".to_string()),
                }
            }
        };

        match op {
            ModbusOp::ReadCoil { address, count } => self.read_coil(&client, *address, *count),
            ModbusOp::ReadDiscrete { address, count } => {
                self.read_discrete(&client, *address, *count)
            }
            ModbusOp::ReadInput { address, count } => self.read_input(&client, *address, *count),
            ModbusOp::ReadHolding { address, count } => {
                self.read_holding(&client, *address, *count)
            }
            ModbusOp::WriteSingle { address, value } => {
                self.write_single(&client, *address, *value)
            }
            ModbusOp::WriteMultiple { address, values } => {
                self.write_multiple(&client, *address, values)
            }
        }
    }

    fn read_coil(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Coil, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let _mapping = mapping;
        let start = SystemTime::now();
        let mut values = Vec::with_capacity(count as usize);
        let mut all_success = true;
        for i in 0..count {
            let reg = ModbusRegister::new(ModbusRegisterKind::Coil, address + i);
            if let Ok(m) = ModbusMapping::create(client, self.unit_id, reg, 1) {
                let mut m = m;
                match m.read::<u8>() {
                    Ok(v) => values.push(if v != 0 { 1u16 } else { 0u16 }),
                    Err(_) => all_success = false,
                }
            }
        }

        if all_success && values.len() == count as usize {
            let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
            OperationResult {
                success: true,
                data: serde_json::json!({
                    "values": values,
                    "latency_us": latency
                }),
                error: None,
            }
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to read all coils".to_string()),
            }
        }
    }

    fn read_discrete(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Discrete, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let _mapping = mapping;
        let start = SystemTime::now();
        let mut values = Vec::with_capacity(count as usize);
        let mut all_success = true;
        for i in 0..count {
            let reg = ModbusRegister::new(ModbusRegisterKind::Discrete, address + i);
            if let Ok(m) = ModbusMapping::create(client, self.unit_id, reg, 1) {
                let mut m = m;
                match m.read::<u8>() {
                    Ok(v) => values.push(if v != 0 { 1u16 } else { 0u16 }),
                    Err(_) => all_success = false,
                }
            }
        }

        if all_success && values.len() == count as usize {
            let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
            OperationResult {
                success: true,
                data: serde_json::json!({
                    "values": values,
                    "latency_us": latency
                }),
                error: None,
            }
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to read all discrete inputs".to_string()),
            }
        }
    }

    fn read_input(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Input, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let _mapping = mapping;
        let start = SystemTime::now();
        let mut values = Vec::with_capacity(count as usize);
        let mut all_success = true;
        for i in 0..count {
            let reg = ModbusRegister::new(ModbusRegisterKind::Input, address + i);
            if let Ok(m) = ModbusMapping::create(client, self.unit_id, reg, 1) {
                let mut m = m;
                match m.read::<u16>() {
                    Ok(v) => values.push(v),
                    Err(_) => all_success = false,
                }
            }
        }

        if all_success && values.len() == count as usize {
            let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
            OperationResult {
                success: true,
                data: serde_json::json!({
                    "values": values,
                    "latency_us": latency
                }),
                error: None,
            }
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to read all input registers".to_string()),
            }
        }
    }

    fn read_holding(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let _mapping = mapping;
        let start = SystemTime::now();
        // Read raw register values
        let mut values = Vec::with_capacity(count as usize);
        let mut all_success = true;
        for i in 0..count {
            let reg = ModbusRegister::new(ModbusRegisterKind::Holding, address + i);
            if let Ok(m) = ModbusMapping::create(client, self.unit_id, reg, 1) {
                let mut m = m;
                match m.read::<u16>() {
                    Ok(v) => values.push(v),
                    Err(_) => all_success = false,
                }
            }
        }

        if all_success && values.len() == count as usize {
            let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
            OperationResult {
                success: true,
                data: serde_json::json!({
                    "values": values,
                    "latency_us": latency
                }),
                error: None,
            }
        } else {
            OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Failed to read all registers".to_string()),
            }
        }
    }

    fn write_single(&self, client: &Client, address: u16, value: u16) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mut mapping = match ModbusMapping::create(client, self.unit_id, register, 1) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let start = SystemTime::now();

        // Write single u16 value
        match mapping.write(value) {
            Ok(()) => {
                let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "value": value,
                        "latency_us": latency
                    }),
                    error: None,
                }
            }
            Err(e) => OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Write failed: {}", e)),
            },
        }
    }

    fn write_multiple(&self, client: &Client, address: u16, values: &[u16]) -> OperationResult {
        let count = values.len() as u16;
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mut mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                }
            }
        };

        let start = SystemTime::now();

        // Write the values as Vec<u16>
        match mapping.write(values.to_vec()) {
            Ok(()) => {
                let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "count": count,
                        "latency_us": latency
                    }),
                    error: None,
                }
            }
            Err(e) => OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Write multiple failed: {}", e)),
            },
        }
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modbus_client_new_starts_disconnected() {
        let client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        assert!(client.connection.is_none());
        assert_eq!(client.endpoint, "127.0.0.1:502");
        assert_eq!(client.unit_id, 1);
    }
}
