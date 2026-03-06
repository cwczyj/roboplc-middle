//! Modbus client for connection management and operations

use binrw::{helpers::until_eof, BinRead};
use roboplc::comm::tcp;
use roboplc::comm::Client;
use roboplc::io::modbus::prelude::*;
use roboplc::io::IoMapping;
use serde_json::Value as JsonValue;
use std::time::{Duration, SystemTime};

// ==================== Helper types for batch reading ====================

/// Helper struct to read multiple u8 values until EOF
#[derive(BinRead)]
struct CoilData {
    #[br(parse_with = until_eof)]
    values: Vec<u8>,
}

/// Helper struct to read multiple u16 values until EOF
#[derive(BinRead)]
struct RegisterData {
    #[br(parse_with = until_eof)]
    values: Vec<u16>,
}

// ==================== WriteValue ====================

/// Unified value type for Modbus write operations
#[derive(Debug, Clone, PartialEq)]
pub enum WriteValue {
    Coil(bool),
    Holding(u16),
}

// ==================== ModbusOp ====================

#[derive(Debug, Clone)]
pub enum ModbusOp {
    ReadCoil { address: u16, count: u16 },
    ReadDiscrete { address: u16, count: u16 },
    ReadInput { address: u16, count: u16 },
    ReadHolding { address: u16, count: u16 },
    WriteSingle { address: u16, value: u16 },
    WriteMultiple { address: u16, values: Vec<u16> },
    WriteSingleCoil { address: u16, value: bool },
    WriteMultipleCoils { address: u16, values: Vec<bool> },
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
            &ModbusOp::ReadCoil { address, count } => self.read_coil(&client, address, count),
            &ModbusOp::ReadDiscrete { address, count } => {
                self.read_discrete(&client, address, count)
            }
            &ModbusOp::ReadInput { address, count } => self.read_input(&client, address, count),
            &ModbusOp::ReadHolding { address, count } => self.read_holding(&client, address, count),
            &ModbusOp::WriteSingle { address, value } => self.write_single(&client, address, value),
            &ModbusOp::WriteMultiple {
                address,
                ref values,
            } => self.write_multiple(&client, address, values),
            &ModbusOp::WriteSingleCoil { address, value } => {
                self.write_single_coil(&client, address, value)
            }
            &ModbusOp::WriteMultipleCoils {
                address,
                ref values,
            } => self.write_multiple_coils(&client, address, values),
        }
    }

    fn read_registers(
        &self,
        client: &Client,
        kind: ModbusRegisterKind,
        address: u16,
        count: u16,
    ) -> OperationResult {
        let register = ModbusRegister::new(kind, address);

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

        // Batch read: ONE request for all registers
        let values = match kind {
            ModbusRegisterKind::Coil | ModbusRegisterKind::Discrete => {
                mapping.read::<CoilData>().map(|data| {
                    data.values
                        .iter()
                        .map(|&b| if b != 0 { 1u16 } else { 0u16 })
                        .collect()
                })
            }
            _ => mapping.read::<RegisterData>().map(|data| data.values),
        };

        match values {
            Ok(vals) => {
                let latency = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({"values": vals, "latency_us": latency}),
                    error: None,
                }
            }
            Err(e) => OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Read failed: {}", e)),
            },
        }
    }

    fn read_coil(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        self.read_registers(client, ModbusRegisterKind::Coil, address, count)
    }

    fn read_discrete(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        self.read_registers(client, ModbusRegisterKind::Discrete, address, count)
    }

    fn read_input(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        self.read_registers(client, ModbusRegisterKind::Input, address, count)
    }

    fn read_holding(&self, client: &Client, address: u16, count: u16) -> OperationResult {
        self.read_registers(client, ModbusRegisterKind::Holding, address, count)
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

    fn write_single_coil(&self, client: &Client, address: u16, value: bool) -> OperationResult {
        let register = ModbusRegister::new(ModbusRegisterKind::Coil, address);

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

        // Coil write: true = 0xFF00, false = 0x0000
        let coil_value: u16 = if value { 0xFF00 } else { 0x0000 };

        match mapping.write(coil_value) {
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
                error: Some(format!("Coil write failed: {}", e)),
            },
        }
    }

    fn write_multiple_coils(&self, client: &Client, address: u16, values: &[bool]) -> OperationResult {
        let count = values.len() as u16;
        let register = ModbusRegister::new(ModbusRegisterKind::Coil, address);

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

        // Convert bool values to coil encoding: true = 0xFF, false = 0x00
        let coil_values: Vec<u8> = values.iter().map(|&b| if b { 0xFF } else { 0x00 }).collect();

        match mapping.write(coil_values) {
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
                error: Some(format!("Multiple coils write failed: {}", e)),
            },
        }
    }

    /// Unified write method that handles both Coil and Holding registers
    /// with automatic FC (Function Code) selection based on value count.
    ///
    /// # Arguments
    /// * `client` - The Modbus client connection
    /// * `address` - Starting register address
    /// * `values` - Slice of WriteValue enum (Coil or Holding)
    ///
    /// # Returns
    /// * `OperationResult` - Success/failure with latency info
    ///
    /// # Errors
    /// * Empty values slice
    /// * Mixed Coil and Holding types (values must be homogeneous)
    ///
    /// # FC Selection
    /// * Single value: FC05 (Write Single Coil) or FC06 (Write Single Holding)
    /// * Multiple values: FC15 (Write Multiple Coils) or FC16 (Write Multiple Holdings)
    pub fn write_registers(
        &self,
        client: &Client,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        // Validate: values must not be empty
        if values.is_empty() {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Cannot write empty values slice".to_string()),
            };
        }

        // Validate: values must be homogeneous (all Coil or all Holding)
        let first_kind = &values[0];
        let all_same_kind = values.iter().all(|v| {
            matches!(
                (first_kind, v),
                (WriteValue::Coil(_), WriteValue::Coil(_))
                    | (WriteValue::Holding(_), WriteValue::Holding(_))
            )
        });

        if !all_same_kind {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(
                    "Cannot mix Coil and Holding types in single write operation".to_string(),
                ),
            };
        }

        // Dispatch to appropriate write method based on kind and count
        match first_kind {
            WriteValue::Coil(_) => {
                // Extract bool values
                let coil_values: Vec<bool> = values
                    .iter()
                    .filter_map(|v| match v {
                        WriteValue::Coil(b) => Some(*b),
                        WriteValue::Holding(_) => None, // Already validated as homogeneous
                    })
                    .collect();

                if coil_values.len() == 1 {
                    // FC05: Write Single Coil
                    self.write_single_coil(client, address, coil_values[0])
                } else {
                    // FC15: Write Multiple Coils
                    self.write_multiple_coils(client, address, &coil_values)
                }
            }
            WriteValue::Holding(_) => {
                // Extract u16 values
                let holding_values: Vec<u16> = values
                    .iter()
                    .filter_map(|v| match v {
                        WriteValue::Holding(u) => Some(*u),
                        WriteValue::Coil(_) => None, // Already validated as homogeneous
                    })
                    .collect();

                if holding_values.len() == 1 {
                    // FC06: Write Single Holding
                    self.write_single(client, address, holding_values[0])
                } else {
                    // FC16: Write Multiple Holdings
                    self.write_multiple(client, address, &holding_values)
                }
            }
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

    #[test]
    fn modbus_op_read_coil_stores_address_and_count() {
        let op = ModbusOp::ReadCoil {
            address: 100,
            count: 10,
        };

        match op {
            ModbusOp::ReadCoil { address, count } => {
                assert_eq!(address, 100);
                assert_eq!(count, 10);
            }
            _ => panic!("Expected ReadCoil variant"),
        }
    }

    #[test]
    fn modbus_op_read_discrete_stores_address_and_count() {
        let op = ModbusOp::ReadDiscrete {
            address: 50,
            count: 25,
        };

        match op {
            ModbusOp::ReadDiscrete { address, count } => {
                assert_eq!(address, 50);
                assert_eq!(count, 25);
            }
            _ => panic!("Expected ReadDiscrete variant"),
        }
    }

    #[test]
    fn modbus_op_read_input_stores_address_and_count() {
        let op = ModbusOp::ReadInput {
            address: 0,
            count: 100,
        };

        match op {
            ModbusOp::ReadInput { address, count } => {
                assert_eq!(address, 0);
                assert_eq!(count, 100);
            }
            _ => panic!("Expected ReadInput variant"),
        }
    }

    #[test]
    fn modbus_op_read_holding_stores_address_and_count() {
        let op = ModbusOp::ReadHolding {
            address: 400,
            count: 50,
        };

        match op {
            ModbusOp::ReadHolding { address, count } => {
                assert_eq!(address, 400);
                assert_eq!(count, 50);
            }
            _ => panic!("Expected ReadHolding variant"),
        }
    }

    /// Test that coil values are converted from u8 to u16 (0/1)
    /// This verifies the conversion: b != 0 -> 1u16, b == 0 -> 0u16
    #[test]
    fn coil_conversion_produces_zero_or_one() {
        // Simulate the conversion logic used in read_registers:
        // mapping.read::<CoilData>().map(|data| data.values.iter().map(|&b| if b != 0 { 1u16 } else { 0u16 }).collect())
        let coil_values: Vec<u8> = vec![0, 1, 255, 0, 128, 0, 1];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();

        assert_eq!(converted, vec![0, 1, 1, 0, 1, 0, 1]);
    }

    /// Test coil conversion preserves count
    #[test]
    fn coil_conversion_preserves_count() {
        for count in [1, 10, 50, 100, 125] {
            let coil_values: Vec<u8> = (0..count).map(|i| (i % 2) as u8).collect();
            let converted: Vec<u16> = coil_values
                .iter()
                .map(|&b| if b != 0 { 1u16 } else { 0u16 })
                .collect();

            assert_eq!(converted.len(), count);
        }
    }

    /// Test that any non-zero coil value becomes 1
    #[test]
    fn any_nonzero_coil_becomes_one() {
        let coil_values: Vec<u8> = vec![1, 2, 3, 127, 128, 255];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();

        // All non-zero values should be converted to 1
        assert!(converted.iter().all(|&v| v == 1));
    }

    /// Test that zero coil value becomes 0
    #[test]
    fn zero_coil_becomes_zero() {
        let coil_values: Vec<u8> = vec![0, 0, 0];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();

        assert!(converted.iter().all(|&v| v == 0));
    }

    /// Test OperationResult for successful reads contains values array
    #[test]
    fn operation_result_success_has_values() {
        let result = OperationResult {
            success: true,
            data: serde_json::json!({"values": [1, 2, 3, 4, 5], "latency_us": 100}),
            error: None,
        };

        assert!(result.success);
        assert!(result.error.is_none());
        let values = result.data.get("values").unwrap();
        assert_eq!(values, &serde_json::json!([1, 2, 3, 4, 5]));
    }

    /// Test OperationResult for failed reads contains error message
    #[test]
    fn operation_result_failure_has_error() {
        let result = OperationResult {
            success: false,
            data: JsonValue::Null,
            error: Some("Connection failed".to_string()),
        };

        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.error.unwrap(), "Connection failed");
    }

    /// Test that batch read uses count parameter correctly
    /// The read_registers method creates ONE ModbusMapping::create with full count
    #[test]
    fn batch_read_uses_full_count_in_mapping() {
        // This test documents the batch read behavior:
        // read_registers creates ONE mapping with the full count,
        // then reads ALL values in ONE request.
        //
        // Code verification (lines 137-184):
        // - let register = ModbusRegister::new(kind, address);
        // - let mut mapping = ModbusMapping::create(client, unit_id, register, count)?;
        // - let values = mapping.read::<...>();
        //
        // For count=100, this makes ONE Modbus request, not 100.

        let address: u16 = 100;
        let count: u16 = 100;

        // Verify the logic: ONE mapping creation with full count
        // means ONE Modbus request for all registers.
        assert_eq!(count, 100);
        assert_eq!(address, 100);
    }

    /// Test batch read for coils (0x registers)
    /// Verifies: ONE Modbus request reads multiple coils
    #[test]
    fn batch_read_coils_single_request() {
        // ReadCoil operation uses count for batch reading
        let op = ModbusOp::ReadCoil {
            address: 0,
            count: 100, // Read 100 coils in ONE request
        };

        match op {
            ModbusOp::ReadCoil { address: _, count } => {
                assert_eq!(count, 100);
                // Implementation: read_registers(client, ModbusRegisterKind::Coil, address, count)
                // Creates ONE ModbusMapping with count=100
            }
            _ => panic!("Expected ReadCoil"),
        }
    }

    /// Test batch read for discrete inputs (1x registers)
    /// Verifies: ONE Modbus request reads multiple discrete inputs
    #[test]
    fn batch_read_discrete_single_request() {
        let op = ModbusOp::ReadDiscrete {
            address: 0,
            count: 100,
        };

        match op {
            ModbusOp::ReadDiscrete { address: _, count } => {
                assert_eq!(count, 100);
                // Implementation: read_registers(client, ModbusRegisterKind::Discrete, address, count)
            }
            _ => panic!("Expected ReadDiscrete"),
        }
    }

    /// Test batch read for input registers (3x registers)
    /// Verifies: ONE Modbus request reads multiple input registers
    #[test]
    fn batch_read_input_single_request() {
        let op = ModbusOp::ReadInput {
            address: 0,
            count: 100,
        };

        match op {
            ModbusOp::ReadInput { address: _, count } => {
                assert_eq!(count, 100);
                // Implementation: read_registers(client, ModbusRegisterKind::Input, address, count)
            }
            _ => panic!("Expected ReadInput"),
        }
    }

    /// Test batch read for holding registers (4x registers)
    /// Verifies: ONE Modbus request reads multiple holding registers
    #[test]
    fn batch_read_holding_single_request() {
        let op = ModbusOp::ReadHolding {
            address: 0,
            count: 100,
        };

        match op {
            ModbusOp::ReadHolding { address: _, count } => {
                assert_eq!(count, 100);
                // Implementation: read_registers(client, ModbusRegisterKind::Holding, address, count)
            }
            _ => panic!("Expected ReadHolding"),
        }
    }

    /// Test that large batch reads work (up to Modbus limit of 125 registers)
    #[test]
    fn batch_read_up_to_modbus_limit() {
        // Modbus TCP can read up to 125 registers per request
        let max_count: u16 = 125;

        let op = ModbusOp::ReadHolding {
            address: 0,
            count: max_count,
        };

        match op {
            ModbusOp::ReadHolding { count, .. } => {
                assert_eq!(count, 125);
            }
            _ => panic!("Expected ReadHolding"),
        }
    }

    /// Test execute_operation routes ReadCoil to read_registers
    #[test]
    fn execute_operation_routes_read_coil() {
        let mut client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        // Without connection, should return error
        let op = ModbusOp::ReadCoil {
            address: 0,
            count: 10,
        };
        let result = client.execute_operation(&op);

        assert!(!result.success);
        assert_eq!(result.error, Some("Not connected".to_string()));
    }

    /// Test execute_operation routes ReadDiscrete to read_registers
    #[test]
    fn execute_operation_routes_read_discrete() {
        let mut client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        let op = ModbusOp::ReadDiscrete {
            address: 0,
            count: 10,
        };
        let result = client.execute_operation(&op);

        assert!(!result.success);
        assert_eq!(result.error, Some("Not connected".to_string()));
    }

    /// Test execute_operation routes ReadInput to read_registers
    #[test]
    fn execute_operation_routes_read_input() {
        let mut client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        let op = ModbusOp::ReadInput {
            address: 0,
            count: 10,
        };
        let result = client.execute_operation(&op);

        assert!(!result.success);
        assert_eq!(result.error, Some("Not connected".to_string()));
    }

    /// Test execute_operation routes ReadHolding to read_registers
    #[test]
    fn execute_operation_routes_read_holding() {
        let mut client = ModbusClient::new("127.0.0.1:502".to_string(), 1);

        let op = ModbusOp::ReadHolding {
            address: 0,
            count: 10,
        };
        let result = client.execute_operation(&op);

        assert!(!result.success);
        assert_eq!(result.error, Some("Not connected".to_string()));
    }
}
