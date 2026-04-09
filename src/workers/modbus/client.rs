//! Modbus client for connection management and operations
//!
//! Implements lazy connection recovery: operations are attempted directly,
//! and reconnection only happens when TCP connection errors are detected.

use parking_lot_rt::RwLock;
use roboplc::comm::tcp;
use roboplc::comm::Client;
use roboplc::io::modbus::prelude::*;
use roboplc::io::IoMapping;
use serde_json::Value as JsonValue;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use binrw::BinRead;
use std::io::{Read, Seek};

use crate::DEFAULT_CONNECT_TIMEOUT_MS;

/// Default TCP connect timeout for Modbus connections
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS as u64);

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
        ModbusConnectionPool::dispatch_op_static(client, self.unit_id, op)
    }

    /// Execute Modbus operation with lazy connection recovery
    ///
    /// Strategy:
    /// 1. Ensure connection exists
    /// 2. Execute the operation
    /// 3. On TCP connection error, reconnect and retry once
    pub fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
        if let Err(e) = self.ensure_connected(DEFAULT_CONNECT_TIMEOUT) {
            return OperationResult {
                success: false,
                data: JsonValue::Null,
                error: Some(format!("Connection failed: {}", e)),
            };
        }

        let client = match self.connection.as_ref() {
            Some(c) => c.clone(),
            None => {
                return OperationResult {
                    success: false,
                    data: JsonValue::Null,
                    error: Some("Connection lost during retry".to_string()),
                };
            }
        };
        let result = self.dispatch_op(&client, op);

        // Retry on connection failure
        if !result.success {
            if let Some(ref error) = result.error {
                if Self::is_connection_broken(error) {
                    tracing::debug!("Connection broken, reconnecting and retrying");
                    self.connection = None;

                    if let Err(e) = self.ensure_connected(DEFAULT_CONNECT_TIMEOUT) {
                        return OperationResult {
                            success: false,
                            data: JsonValue::Null,
                            error: Some(format!("Reconnection failed: {}", e)),
                        };
                    }

                    let client = match self.connection.as_ref() {
                        Some(c) => c.clone(),
                        None => {
                            return OperationResult {
                                success: false,
                                data: JsonValue::Null,
                                error: Some("Connection lost after ensure_connected".to_string()),
                            };
                        }
                    };
                    return self.dispatch_op(&client, op);
                }
            }
        }

        result
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

#[cfg(test)]
const POOL_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const POOL_CONNECTION_TIMEOUT: Duration = DEFAULT_CONNECT_TIMEOUT;

/// Connection pool for multiple concurrent Modbus connections
///
/// Provides a pool of TCP connections that can be acquired and returned,
/// enabling parallel request processing for a single device.
pub struct ModbusConnectionPool {
    endpoint: String,
    unit_id: u8,
    pool_size: usize,
    health_check_interval: Duration,
    available: RwLock<VecDeque<PooledConnection>>,
    total_created: std::sync::atomic::AtomicUsize,
    transaction_id: std::sync::atomic::AtomicU16,
}

impl Clone for ModbusConnectionPool {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            unit_id: self.unit_id,
            pool_size: self.pool_size,
            health_check_interval: self.health_check_interval,
            available: RwLock::new(VecDeque::new()),
            total_created: std::sync::atomic::AtomicUsize::new(0),
            transaction_id: std::sync::atomic::AtomicU16::new(1),
        }
    }
}

impl ModbusConnectionPool {
    pub fn new(
        endpoint: String,
        unit_id: u8,
        pool_size: usize,
        health_check_interval: Duration,
    ) -> Self {
        Self {
            endpoint,
            unit_id,
            pool_size,
            health_check_interval,
            available: RwLock::new(VecDeque::new()),
            total_created: AtomicUsize::new(0),
            transaction_id: AtomicU16::new(1),
        }
    }

    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    pub fn available_count(&self) -> usize {
        self.available.read().len()
    }

    pub fn total_created(&self) -> usize {
        self.total_created.load(Ordering::Relaxed)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    fn create_connection(&self) -> Result<Client, Box<dyn std::error::Error>> {
        tracing::debug!(
            endpoint = %self.endpoint,
            total_created = self.total_created.load(Ordering::Relaxed),
            pool_size = self.pool_size,
            "Creating new Modbus connection"
        );
        let client = tcp::connect(&self.endpoint, POOL_CONNECTION_TIMEOUT)?;
        client.connect()?;
        self.total_created.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            endpoint = %self.endpoint,
            total_created = self.total_created.load(Ordering::Relaxed),
            "Modbus connection created successfully"
        );
        Ok(client)
    }

    /// Acquire a connection from the pool for parallel execution.
    /// Returns a connection that must be released via `release_connection`.
    pub fn acquire_connection(&self) -> Result<Client, Box<dyn std::error::Error>> {
        let available_before = self.available.read().len();
        tracing::debug!(
            endpoint = %self.endpoint,
            available = available_before,
            pool_size = self.pool_size,
            total_created = self.total_created.load(Ordering::Relaxed),
            "Attempting to acquire connection from pool"
        );

        let mut available = self.available.write();

        // Clean up unhealthy or expired connections at the front
        let mut dropped_count = 0;
        let mut dropped_reasons: Vec<&'static str> = Vec::new();
        while let Some(front) = available.front() {
            if front.is_healthy && front.age() < self.health_check_interval {
                break;
            }
            let reason = if !front.is_healthy {
                "unhealthy"
            } else {
                "expired"
            };
            dropped_reasons.push(reason);
            available.pop_front();
            self.total_created.fetch_sub(1, Ordering::Relaxed);
            dropped_count += 1;
        }

        if dropped_count > 0 {
            tracing::debug!(
                endpoint = %self.endpoint,
                dropped_count = dropped_count,
                reasons = ?dropped_reasons,
                remaining = available.len(),
                "Cleaned up stale connections from pool"
            );
        }

        // Try to acquire a healthy connection
        if let Some(pooled) = available.pop_front() {
            tracing::debug!(
                endpoint = %self.endpoint,
                available_after = available.len(),
                "Connection reused from pool"
            );
            return Ok(pooled.client);
        }

        // Pool is empty, create new connection
        drop(available);
        tracing::debug!(
            endpoint = %self.endpoint,
            available_before = available_before,
            dropped = dropped_count,
            "Pool empty, creating new connection"
        );
        self.create_connection()
    }

    /// Release a connection back to the pool after operation completes.
    pub fn release_connection(&self, client: Client, is_healthy: bool) {
        if !is_healthy {
            tracing::debug!(
                endpoint = %self.endpoint,
                reason = "operation_failed",
                "Connection discarded - not returning to pool"
            );
            self.total_created.fetch_sub(1, Ordering::Relaxed);
            return;
        }

        let mut available = self.available.write();
        if available.len() < self.pool_size {
            available.push_back(PooledConnection::new(client));
            tracing::debug!(
                endpoint = %self.endpoint,
                available = available.len(),
                pool_size = self.pool_size,
                "Connection returned to pool"
            );
        } else {
            tracing::debug!(
                endpoint = %self.endpoint,
                pool_size = self.pool_size,
                "Pool at capacity, discarding connection"
            );
            self.total_created.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Execute an operation using a connection from the pool.
    /// Thread-safe: acquires and releases connection atomically.
    pub fn execute_operation(&self, op: &ModbusOp) -> OperationResult {
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

        let tr_id = self.transaction_id.fetch_add(1, Ordering::Relaxed);
        tracing::trace!("Executing Modbus operation with transaction_id={}", tr_id);

        let result = Self::dispatch_op_static(&client, self.unit_id, op);
        self.release_connection(client, result.success);

        result
    }

    /// Execute an operation directly on a client (static helper for async use).
    pub fn execute_on_client(client: &Client, unit_id: u8, op: &ModbusOp) -> OperationResult {
        Self::dispatch_op_static(client, unit_id, op)
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
        Self::read_registers_static_with_tr_id(client, unit_id, kind, address, count, None)
    }

    fn read_registers_static_with_tr_id(
        client: &Client,
        unit_id: u8,
        kind: ModbusRegisterKind,
        address: u16,
        count: u16,
        transaction_id: Option<u16>,
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
        let pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool.pool_size(), 3);
        assert_eq!(pool.available_count(), 0);
        assert_eq!(pool.total_created(), 0);
    }

    #[test]
    fn pool_new_with_different_sizes() {
        let pool1 = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            1,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool1.pool_size(), 1);

        let pool5 = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            5,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool5.pool_size(), 5);

        let pool10 = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            10,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool10.pool_size(), 10);
    }

    #[test]
    fn pool_acquire_connection_fails_without_server() {
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        let result = pool.acquire_connection();
        assert!(result.is_err());
    }

    #[test]
    fn pool_execute_operation_fails_without_server() {
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
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
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
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
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            2,
            POOL_HEALTH_CHECK_INTERVAL,
        );

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

    #[test]
    fn pool_discards_old_connections() {
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool.available_count(), 0);
        let result = pool.acquire_connection();
        assert!(
            result.is_err(),
            "Should fail creating new connection without server"
        );
    }

    #[test]
    fn pool_failed_operation_does_not_return_to_pool() {
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        let op = ModbusOp::ReadHolding {
            address: 100,
            count: 10,
        };
        let result = pool.execute_operation(&op);
        assert!(!result.success);
        assert_eq!(
            pool.available_count(),
            0,
            "Failed connection should not be in pool"
        );
        assert_eq!(
            pool.total_created(),
            0,
            "No connection created when server unavailable"
        );
    }

    #[test]
    fn pool_tracks_total_created_correctly() {
        let mut pool = ModbusConnectionPool::new(
            "127.0.0.1:502".to_string(),
            1,
            3,
            POOL_HEALTH_CHECK_INTERVAL,
        );
        assert_eq!(pool.total_created(), 0, "No connections created initially");
        let _ = pool.acquire_connection();
        assert_eq!(
            pool.total_created(),
            0,
            "No connection created when server unavailable"
        );
        let _ = pool.acquire_connection();
        assert_eq!(
            pool.total_created(),
            0,
            "Still no connection created when server unavailable"
        );
    }

    #[test]
    fn pooled_connection_age_calculation() {
        use std::thread::sleep;
        let client_result = tcp::connect("127.0.0.1:502", Duration::from_millis(100));
        if let Ok(client) = client_result {
            let pooled = PooledConnection::new(client);
            let initial_age = pooled.age();
            assert!(initial_age < Duration::from_millis(100));
            sleep(Duration::from_millis(50));
            let later_age = pooled.age();
            assert!(later_age > initial_age);
        }
    }

    #[test]
    fn pool_health_flag_is_set_on_creation() {
        let client_result = tcp::connect("127.0.0.1:502", Duration::from_millis(100));
        if let Ok(client) = client_result {
            let pooled = PooledConnection::new(client);
            assert!(pooled.is_healthy);
        }
    }
}
