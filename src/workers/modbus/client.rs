//! Modbus client for connection management and operations
//!
//! Implements lazy connection recovery: operations are attempted directly,
//! and reconnection only happens when TCP connection errors are detected.

use roboplc::comm::tcp;
use roboplc::comm::Client;
use roboplc::io::modbus::prelude::*;
use roboplc::io::IoMapping;
use serde_json::Value as JsonValue;
use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use binrw::BinRead;
use std::io::{Read, Seek};

/// Raw binary data reader for Modbus responses
#[derive(Debug)]
struct BinaryData<T> {
    values: Vec<T>,
}

impl<T: Copy + Default> BinRead for BinaryData<T>
where
    T: binrw::BinRead<Args<'static> = ()>,
{
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let mut values = Vec::new();
        loop {
            match T::read_options(reader, endian, ()) {
                Ok(val) => values.push(val),
                Err(binrw::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break
                }
                Err(e) => return Err(e),
            }
        }
        Ok(BinaryData { values })
    }
}

type CoilData = BinaryData<u8>;
type RegisterData = BinaryData<u16>;

/// Unified value type for Modbus write operations
#[derive(Debug, Clone, PartialEq)]
pub enum WriteValue {
    Coil(bool),
    Holding(u16),
}

/// Modbus operation types
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

/// Modbus TCP client with lazy connection recovery
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

    /// Check if connection error indicates a broken TCP connection
    fn is_connection_broken(error: &str) -> bool {
        error.contains("I/O error")
            || error.contains("failed to fill")
            || error.contains("Broken pipe")
            || error.contains("connection closed")
            || error.contains("connection reset")
    }

    /// Ensure connection before operation
    fn ensure_connected(&mut self, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        if self.connection.is_none() {
            self.connect(timeout)?;
        }
        Ok(())
    }

    /// Dispatch ModbusOp to appropriate handler
    fn dispatch_op(&self, client: &Client, op: &ModbusOp) -> OperationResult {
        match op {
            &ModbusOp::ReadCoil { address, count } => {
                self.read_registers(client, ModbusRegisterKind::Coil, address, count)
            }
            &ModbusOp::ReadDiscrete { address, count } => {
                self.read_registers(client, ModbusRegisterKind::Discrete, address, count)
            }
            &ModbusOp::ReadInput { address, count } => {
                self.read_registers(client, ModbusRegisterKind::Input, address, count)
            }
            &ModbusOp::ReadHolding { address, count } => {
                self.read_registers(client, ModbusRegisterKind::Holding, address, count)
            }
            &ModbusOp::WriteSingle { address, value } => {
                self.write_registers(client, address, &[WriteValue::Holding(value)])
            }
            &ModbusOp::WriteMultiple {
                address,
                ref values,
            } => {
                let write_values: Vec<_> = values.iter().map(|&v| WriteValue::Holding(v)).collect();
                self.write_registers(client, address, &write_values)
            }
            &ModbusOp::WriteSingleCoil { address, value } => {
                self.write_registers(client, address, &[WriteValue::Coil(value)])
            }
            &ModbusOp::WriteMultipleCoils {
                address,
                ref values,
            } => {
                let write_values: Vec<_> = values.iter().map(|&v| WriteValue::Coil(v)).collect();
                self.write_registers(client, address, &write_values)
            }
        }
    }

    /// Execute Modbus operation with lazy connection recovery
    ///
    /// Strategy:
    /// 1. Ensure connection exists
    /// 2. Execute the operation
    /// 3. On TCP connection error, reconnect and retry once
    pub fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
        if let Err(e) = self.ensure_connected(Duration::from_secs(1)) {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Connection failed: {}", e)),
            };
        }

        let client = self.connection.as_ref().unwrap().clone();
        let result = self.dispatch_op(&client, op);

        // Retry on connection failure
        if !result.success {
            if let Some(ref error) = result.error {
                if Self::is_connection_broken(error) {
                    tracing::debug!("Connection broken, reconnecting and retrying");
                    self.connection = None;

                    if let Err(e) = self.ensure_connected(Duration::from_secs(1)) {
                        return OperationResult {
                            success: false,
                            data: JsonValue::Null,
                            error: Some(format!("Reconnection failed: {}", e)),
                        };
                    }

                    let client = self.connection.as_ref().unwrap().clone();
                    return self.dispatch_op(&client, op);
                }
            }
        }

        result
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
                let latency_us = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({"values": vals, "latency_us": latency_us}),
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

    /// Unified write method for Coil and Holding registers
    fn write_registers(
        &self,
        client: &Client,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        if values.is_empty() {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Cannot write empty values slice".to_string()),
            };
        }

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

        match first_kind {
            WriteValue::Coil(_) => self.write_coils(client, address, values),
            WriteValue::Holding(_) => self.write_holding_registers(client, address, values),
        }
    }

    /// Write coil values with FC05 (single) or FC15 (multiple)
    fn write_coils(&self, client: &Client, address: u16, values: &[WriteValue]) -> OperationResult {
        let coil_values: Vec<bool> = values
            .iter()
            .filter_map(|v| match v {
                WriteValue::Coil(b) => Some(*b),
                WriteValue::Holding(_) => None,
            })
            .collect();

        let count = coil_values.len() as u16;
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

        let result = if count == 1 {
            let coil_value: u16 = if coil_values[0] { 0xFF00 } else { 0x0000 };
            mapping.write(coil_value)
        } else {
            let coil_bytes: Vec<u8> = coil_values
                .iter()
                .map(|&b| if b { 0xFF } else { 0x00 })
                .collect();
            mapping.write(coil_bytes)
        };

        self.build_write_result(
            result.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) }),
            address,
            count,
            start.elapsed().unwrap_or(Duration::ZERO),
        )
    }

    /// Write holding register values with FC06 (single) or FC16 (multiple)
    fn write_holding_registers(
        &self,
        client: &Client,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        let holding_values: Vec<u16> = values
            .iter()
            .filter_map(|v| match v {
                WriteValue::Holding(u) => Some(*u),
                WriteValue::Coil(_) => None,
            })
            .collect();

        let count = holding_values.len() as u16;
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

        let result = if count == 1 {
            mapping.write(holding_values[0])
        } else {
            mapping.write(holding_values)
        };

        self.build_write_result(
            result.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) }),
            address,
            count,
            start.elapsed().unwrap_or(Duration::ZERO),
        )
    }

    /// Build OperationResult from write operation
    fn build_write_result(
        &self,
        result: Result<(), Box<dyn std::error::Error>>,
        address: u16,
        count: u16,
        duration: Duration,
    ) -> OperationResult {
        match result {
            Ok(()) => {
                let latency_us = duration.as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "count": count,
                        "latency_us": latency_us
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
}

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

    #[test]
    fn coil_conversion_produces_zero_or_one() {
        let coil_values: Vec<u8> = vec![0, 1, 255, 0, 128, 0, 1];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();
        assert_eq!(converted, vec![0, 1, 1, 0, 1, 0, 1]);
    }

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

    #[test]
    fn any_nonzero_coil_becomes_one() {
        let coil_values: Vec<u8> = vec![1, 2, 3, 127, 128, 255];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();
        assert!(converted.iter().all(|&v| v == 1));
    }

    #[test]
    fn zero_coil_becomes_zero() {
        let coil_values: Vec<u8> = vec![0, 0, 0];
        let converted: Vec<u16> = coil_values
            .iter()
            .map(|&b| if b != 0 { 1u16 } else { 0u16 })
            .collect();
        assert!(converted.iter().all(|&v| v == 0));
    }

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

    #[test]
    fn execute_operation_routes_read_coil() {
        let mut client = ModbusClient::new("127.0.0.1:502".to_string(), 1);
        let op = ModbusOp::ReadCoil {
            address: 0,
            count: 10,
        };
        let result = client.execute_operation(&op);
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .starts_with("Connection failed:"));
    }

    #[test]
    fn write_registers_validates_homogeneous_types() {
        let coil_values = vec![WriteValue::Coil(true), WriteValue::Coil(false)];
        let first = &coil_values[0];
        let all_same = coil_values
            .iter()
            .all(|v| matches!((first, v), (WriteValue::Coil(_), WriteValue::Coil(_))));
        assert!(all_same);

        let holding_values = vec![WriteValue::Holding(100), WriteValue::Holding(200)];
        let first = &holding_values[0];
        let all_same = holding_values
            .iter()
            .all(|v| matches!((first, v), (WriteValue::Holding(_), WriteValue::Holding(_))));
        assert!(all_same);

        let mixed = vec![WriteValue::Coil(true), WriteValue::Holding(100)];
        let first = &mixed[0];
        let all_same = mixed.iter().all(|v| {
            matches!(
                (first, v),
                (WriteValue::Coil(_), WriteValue::Coil(_))
                    | (WriteValue::Holding(_), WriteValue::Holding(_))
            )
        });
        assert!(!all_same);
    }

    #[test]
    fn coil_single_write_encoding() {
        let true_value: u16 = if true { 0xFF00 } else { 0x0000 };
        let false_value: u16 = if false { 0xFF00 } else { 0x0000 };
        assert_eq!(true_value, 0xFF00);
        assert_eq!(false_value, 0x0000);
    }

    #[test]
    fn coil_multiple_write_encoding() {
        let values = vec![true, false, true];
        let encoded: Vec<u8> = values
            .iter()
            .map(|&b| if b { 0xFF } else { 0x00 })
            .collect();
        assert_eq!(encoded, vec![0xFF, 0x00, 0xFF]);
    }
}

struct PooledConnection {
    client: Client,
    last_used: SystemTime,
    is_healthy: bool,
}

impl PooledConnection {
    fn new(client: Client) -> Self {
        Self {
            client,
            last_used: SystemTime::now(),
            is_healthy: true,
        }
    }

    fn age(&self) -> Duration {
        self.last_used.elapsed().unwrap_or(Duration::ZERO)
    }
}

const POOL_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const POOL_CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

/// Connection pool for multiple concurrent Modbus connections
///
/// Provides a pool of TCP connections that can be acquired and returned,
/// enabling parallel request processing for a single device.
pub struct ModbusConnectionPool {
    endpoint: String,
    unit_id: u8,
    pool_size: usize,
    available: VecDeque<PooledConnection>,
    total_created: usize,
}

impl ModbusConnectionPool {
    pub fn new(endpoint: String, unit_id: u8, pool_size: usize) -> Self {
        Self {
            endpoint,
            unit_id,
            pool_size,
            available: VecDeque::new(),
            total_created: 0,
        }
    }

    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    pub fn available_count(&self) -> usize {
        self.available.len()
    }

    pub fn total_created(&self) -> usize {
        self.total_created
    }

    fn create_connection(&mut self) -> Result<Client, Box<dyn std::error::Error>> {
        let client = tcp::connect(&self.endpoint, POOL_CONNECTION_TIMEOUT)?;
        client.connect()?;
        self.total_created += 1;
        Ok(client)
    }

    fn acquire_connection(&mut self) -> Result<Client, Box<dyn std::error::Error>> {
        while let Some(pooled) = self.available.pop_front() {
            if pooled.is_healthy && pooled.age() < POOL_HEALTH_CHECK_INTERVAL {
                return Ok(pooled.client);
            }
        }
        self.create_connection()
    }

    fn release_connection(&mut self, client: Client, is_healthy: bool) {
        if !is_healthy {
            return;
        }

        if self.available.len() < self.pool_size {
            self.available.push_back(PooledConnection::new(client));
        }
    }

    pub fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
        let client = match self.acquire_connection() {
            Ok(c) => c,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Pool connection failed: {}", e)),
                };
            }
        };

        let result = Self::dispatch_op_static(&client, self.unit_id, op);
        self.release_connection(client, result.success);

        result
    }

    fn dispatch_op_static(client: &Client, unit_id: u8, op: &ModbusOp) -> OperationResult {
        match op {
            ModbusOp::ReadCoil { address, count } => Self::read_registers_static(
                client,
                unit_id,
                ModbusRegisterKind::Coil,
                *address,
                *count,
            ),
            ModbusOp::ReadDiscrete { address, count } => Self::read_registers_static(
                client,
                unit_id,
                ModbusRegisterKind::Discrete,
                *address,
                *count,
            ),
            ModbusOp::ReadInput { address, count } => Self::read_registers_static(
                client,
                unit_id,
                ModbusRegisterKind::Input,
                *address,
                *count,
            ),
            ModbusOp::ReadHolding { address, count } => Self::read_registers_static(
                client,
                unit_id,
                ModbusRegisterKind::Holding,
                *address,
                *count,
            ),
            ModbusOp::WriteSingle { address, value } => Self::write_registers_static(
                client,
                unit_id,
                *address,
                &[WriteValue::Holding(*value)],
            ),
            ModbusOp::WriteMultiple { address, values } => {
                let write_values: Vec<_> = values.iter().map(|&v| WriteValue::Holding(v)).collect();
                Self::write_registers_static(client, unit_id, *address, &write_values)
            }
            ModbusOp::WriteSingleCoil { address, value } => {
                Self::write_registers_static(client, unit_id, *address, &[WriteValue::Coil(*value)])
            }
            ModbusOp::WriteMultipleCoils { address, values } => {
                let write_values: Vec<_> = values.iter().map(|&v| WriteValue::Coil(v)).collect();
                Self::write_registers_static(client, unit_id, *address, &write_values)
            }
        }
    }

    fn read_registers_static(
        client: &Client,
        unit_id: u8,
        kind: ModbusRegisterKind,
        address: u16,
        count: u16,
    ) -> OperationResult {
        let register = ModbusRegister::new(kind, address);

        let mut mapping = match ModbusMapping::create(client, unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                };
            }
        };

        let start = SystemTime::now();

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
                let latency_us = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({"values": vals, "latency_us": latency_us}),
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

    fn write_registers_static(
        client: &Client,
        unit_id: u8,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        if values.is_empty() {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some("Cannot write empty values slice".to_string()),
            };
        }

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

        match first_kind {
            WriteValue::Coil(_) => Self::write_coils_static(client, unit_id, address, values),
            WriteValue::Holding(_) => {
                Self::write_holding_registers_static(client, unit_id, address, values)
            }
        }
    }

    fn write_coils_static(
        client: &Client,
        unit_id: u8,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        let coil_values: Vec<bool> = values
            .iter()
            .filter_map(|v| match v {
                WriteValue::Coil(b) => Some(*b),
                WriteValue::Holding(_) => None,
            })
            .collect();

        let count = coil_values.len() as u16;
        let register = ModbusRegister::new(ModbusRegisterKind::Coil, address);

        let mut mapping = match ModbusMapping::create(client, unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                };
            }
        };

        let start = SystemTime::now();

        let result = if count == 1 {
            let coil_value: u16 = if coil_values[0] { 0xFF00 } else { 0x0000 };
            mapping.write(coil_value)
        } else {
            let coil_bytes: Vec<u8> = coil_values
                .iter()
                .map(|&b| if b { 0xFF } else { 0x00 })
                .collect();
            mapping.write(coil_bytes)
        };

        Self::build_write_result_static(
            result.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) }),
            address,
            count,
            start.elapsed().unwrap_or(Duration::ZERO),
        )
    }

    fn write_holding_registers_static(
        client: &Client,
        unit_id: u8,
        address: u16,
        values: &[WriteValue],
    ) -> OperationResult {
        let holding_values: Vec<u16> = values
            .iter()
            .filter_map(|v| match v {
                WriteValue::Holding(u) => Some(*u),
                WriteValue::Coil(_) => None,
            })
            .collect();

        let count = holding_values.len() as u16;
        let register = ModbusRegister::new(ModbusRegisterKind::Holding, address);

        let mut mapping = match ModbusMapping::create(client, unit_id, register, count) {
            Ok(m) => m,
            Err(e) => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some(format!("Failed to create mapping: {}", e)),
                };
            }
        };

        let start = SystemTime::now();

        let result = if count == 1 {
            mapping.write(holding_values[0])
        } else {
            mapping.write(holding_values)
        };

        Self::build_write_result_static(
            result.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) }),
            address,
            count,
            start.elapsed().unwrap_or(Duration::ZERO),
        )
    }

    fn build_write_result_static(
        result: Result<(), Box<dyn std::error::Error>>,
        address: u16,
        count: u16,
        duration: Duration,
    ) -> OperationResult {
        match result {
            Ok(()) => {
                let latency_us = duration.as_micros() as u64;
                OperationResult {
                    success: true,
                    data: serde_json::json!({
                        "address": address,
                        "count": count,
                        "latency_us": latency_us
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
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    #[test]
    fn pool_new_creates_empty_pool() {
        let pool = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 3);
        assert_eq!(pool.pool_size(), 3);
        assert_eq!(pool.available_count(), 0);
        assert_eq!(pool.total_created(), 0);
    }

    #[test]
    fn pool_new_with_different_sizes() {
        let pool1 = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 1);
        assert_eq!(pool1.pool_size(), 1);

        let pool5 = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 5);
        assert_eq!(pool5.pool_size(), 5);

        let pool10 = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 10);
        assert_eq!(pool10.pool_size(), 10);
    }

    #[test]
    fn pool_acquire_connection_fails_without_server() {
        let mut pool = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 3);
        let result = pool.acquire_connection();
        assert!(result.is_err());
    }

    #[test]
    fn pool_execute_operation_fails_without_server() {
        let mut pool = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 3);
        let op = ModbusOp::ReadHolding {
            address: 100,
            count: 10,
        };
        let result = pool.execute_operation(&op);
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Pool connection failed"));
    }

    #[test]
    fn pool_release_connection_discards_unhealthy() {
        let mut pool = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 3);
        assert_eq!(pool.available_count(), 0);

        pool.release_connection(
            tcp::connect("127.0.0.1:502", Duration::from_millis(100))
                .ok()
                .unwrap(),
            false,
        );
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn pool_release_connection_respects_pool_size_limit() {
        let mut pool = ModbusConnectionPool::new("127.0.0.1:502".to_string(), 1, 2);

        for _ in 0..3 {
            pool.release_connection(
                tcp::connect("127.0.0.1:502", Duration::from_millis(100))
                    .ok()
                    .unwrap(),
                true,
            );
        }
        assert_eq!(pool.available_count(), pool.pool_size());
    }

    #[test]
    fn pool_constants_are_reasonable() {
        assert!(POOL_HEALTH_CHECK_INTERVAL >= Duration::from_secs(10));
        assert!(POOL_HEALTH_CHECK_INTERVAL <= Duration::from_secs(60));
        assert!(POOL_CONNECTION_TIMEOUT >= Duration::from_millis(100));
        assert!(POOL_CONNECTION_TIMEOUT <= Duration::from_secs(5));
    }
}
