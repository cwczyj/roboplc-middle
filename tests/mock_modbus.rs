//! Mock Modbus TCP Server for Testing
//!
//! This module provides a mock Modbus TCP server that can be used for testing
//! the ModbusWorker and related components. It simulates Modbus TCP responses
//! without requiring actual hardware.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Modbus function codes
mod codes {
    pub const READ_COILS: u8 = 0x01;
    pub const READ_DISCRETE_INPUTS: u8 = 0x02;
    pub const READ_HOLDING_REGISTERS: u8 = 0x03;
    pub const READ_INPUT_REGISTERS: u8 = 0x04;
    pub const WRITE_SINGLE_COIL: u8 = 0x05;
    pub const WRITE_SINGLE_REGISTER: u8 = 0x06;
    pub const WRITE_MULTIPLE_REGISTERS: u8 = 0x10;
    pub const DIAGNOSTICS: u8 = 0x08;
}

/// Modbus exception codes
mod exceptions {
    pub const ILLEGAL_FUNCTION: u8 = 0x01;
    pub const ILLEGAL_DATA_ADDRESS: u8 = 0x02;
    pub const ILLEGAL_DATA_VALUE: u8 = 0x03;
    pub const SERVER_DEVICE_FAILURE: u8 = 0x04;
}

/// Configuration for the mock Modbus server
#[derive(Debug, Clone)]
pub struct MockModbusConfig {
    /// Port to listen on (0 = auto-assign)
    pub port: u16,
    /// Unit ID to respond to (255 = respond to all)
    pub unit_id: u8,
    /// Delay before responding (simulates latency)
    pub response_delay_ms: u64,
    /// Whether to accept connections
    pub accept_connections: bool,
    /// Simulate connection drops after N requests
    pub drop_after_requests: Option<usize>,
}

impl Default for MockModbusConfig {
    fn default() -> Self {
        Self {
            port: 0,
            unit_id: 255,
            response_delay_ms: 0,
            accept_connections: true,
            drop_after_requests: None,
        }
    }
}

/// Mock Modbus TCP Server
///
/// A simple Modbus TCP server that can be used for testing.
/// It maintains internal register state and responds to basic requests.
///
/// # Example
///
/// ```rust
/// use roboplc_middleware::tests::mock_modbus::{MockModbusServer, MockModbusConfig};
///
/// let server = MockModbusServer::start(MockModbusConfig::default()).unwrap();
/// println!("Server running on port {}", server.port());
///
/// // Configure some registers
/// server.set_holding_register(100, 42);
///
/// // ... run tests ...
///
/// server.stop();
/// ```
pub struct MockModbusServer {
    port: u16,
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    registers: Arc<std::sync::Mutex<MockModbusState>>,
}

#[derive(Default)]
pub struct MockModbusState {
    pub holding_registers: HashMap<u16, u16>,
    pub input_registers: HashMap<u16, u16>,
    pub coils: HashMap<u16, bool>,
    pub discrete_inputs: HashMap<u16, bool>,
    pub request_count: usize,
    pub fail_next: bool,
    pub custom_handler: Option<Box<dyn Fn(u8, &[u8]) -> Option<Vec<u8>> + Send + Sync>>,
}

impl MockModbusState {
    pub fn new() -> Self {
        Self {
            holding_registers: HashMap::new(),
            input_registers: HashMap::new(),
            coils: HashMap::new(),
            discrete_inputs: HashMap::new(),
            request_count: 0,
            fail_next: false,
            custom_handler: None,
        }
    }
}

impl MockModbusServer {
    /// Start a new mock Modbus server
    pub fn start(config: MockModbusConfig) -> Result<Self, std::io::Error> {
        let running = Arc::new(AtomicBool::new(true));
        let registers = Arc::new(std::sync::Mutex::new(MockModbusState::new()));

        let addr = format!("127.0.0.1:{}", config.port);
        let listener = TcpListener::bind(&addr)?;
        let actual_port = listener.local_addr()?.port();

        // Set non-blocking mode for accept
        listener.set_nonblocking(true)?;

        let running_clone = running.clone();
        let registers_clone = registers.clone();
        let config_clone = config.clone();

        let handle = thread::spawn(move || {
            Self::server_loop(listener, running_clone, registers_clone, config_clone);
        });

        // Give server a moment to start
        thread::sleep(Duration::from_millis(10));

        Ok(Self {
            port: actual_port,
            running,
            thread_handle: Some(handle),
            registers,
        })
    }

    /// Get the port the server is listening on
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the address to connect to
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Stop the server
    pub fn stop(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    /// Set a holding register value
    pub fn set_holding_register(&self, address: u16, value: u16) {
        let mut state = self.registers.lock().unwrap();
        state.holding_registers.insert(address, value);
    }

    /// Set multiple holding registers
    pub fn set_holding_registers(&self, start: u16, values: &[u16]) {
        let mut state = self.registers.lock().unwrap();
        for (i, &value) in values.iter().enumerate() {
            state.holding_registers.insert(start + i as u16, value);
        }
    }

    /// Get a holding register value
    pub fn get_holding_register(&self, address: u16) -> Option<u16> {
        let state = self.registers.lock().unwrap();
        state.holding_registers.get(&address).copied()
    }

    /// Set an input register value
    pub fn set_input_register(&self, address: u16, value: u16) {
        let mut state = self.registers.lock().unwrap();
        state.input_registers.insert(address, value);
    }

    /// Set a coil value
    pub fn set_coil(&self, address: u16, value: bool) {
        let mut state = self.registers.lock().unwrap();
        state.coils.insert(address, value);
    }

    /// Set a discrete input value
    pub fn set_discrete_input(&self, address: u16, value: bool) {
        let mut state = self.registers.lock().unwrap();
        state.discrete_inputs.insert(address, value);
    }

    /// Get request count
    pub fn request_count(&self) -> usize {
        let state = self.registers.lock().unwrap();
        state.request_count
    }

    /// Make the next request fail
    pub fn fail_next_request(&self) {
        let mut state = self.registers.lock().unwrap();
        state.fail_next = true;
    }

    /// Reset all state
    pub fn reset(&self) {
        let mut state = self.registers.lock().unwrap();
        state.holding_registers.clear();
        state.input_registers.clear();
        state.coils.clear();
        state.discrete_inputs.clear();
        state.request_count = 0;
        state.fail_next = false;
    }

    fn server_loop(
        listener: TcpListener,
        running: Arc<AtomicBool>,
        registers: Arc<std::sync::Mutex<MockModbusState>>,
        config: MockModbusConfig,
    ) {
        while running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) if config.accept_connections => {
                    let running_clone = running.clone();
                    let registers_clone = registers.clone();
                    let config_clone = config.clone();

                    thread::spawn(move || {
                        Self::handle_connection(
                            stream,
                            running_clone,
                            registers_clone,
                            config_clone,
                        );
                    });
                }
                Ok(_) => {
                    // Reject connection
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    fn handle_connection(
        mut stream: TcpStream,
        running: Arc<AtomicBool>,
        registers: Arc<std::sync::Mutex<MockModbusState>>,
        config: MockModbusConfig,
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

        let mut request_count = 0usize;

        while running.load(Ordering::SeqCst) {
            let mut header = [0u8; 7];
            match stream.read_exact(&mut header) {
                Ok(_) => {}
                Err(_) => break,
            }

            // Parse MBAP header
            let transaction_id = u16::from_be_bytes([header[0], header[1]]);
            let protocol_id = u16::from_be_bytes([header[2], header[3]]);
            let length = u16::from_be_bytes([header[4], header[5]]);
            let unit_id = header[6];

            if protocol_id != 0 {
                continue;
            }

            // Read the rest of the request
            let data_len = length as usize - 1;
            if data_len > 260 {
                continue; // Invalid request size
            }

            let mut data = vec![0u8; data_len];
            if stream.read_exact(&mut data).is_err() {
                break;
            }

            // Check unit ID
            if config.unit_id != 255 && unit_id != config.unit_id {
                continue;
            }

            // Apply response delay
            if config.response_delay_ms > 0 {
                thread::sleep(Duration::from_millis(config.response_delay_ms));
            }

            // Check if we should drop connection
            if let Some(drop_after) = config.drop_after_requests {
                request_count += 1;
                if request_count > drop_after {
                    break; // Simulate connection drop
                }
            }

            // Process request
            let response = {
                let mut state = registers.lock().unwrap();
                state.request_count += 1;

                if state.fail_next {
                    state.fail_next = false;
                    Self::build_exception_response(
                        transaction_id,
                        unit_id,
                        data[0],
                        exceptions::SERVER_DEVICE_FAILURE,
                    )
                } else {
                    Self::process_request(transaction_id, unit_id, &data, &mut state)
                }
            };

            if let Some(response) = response {
                let _ = stream.write_all(&response);
            }
        }
    }

    fn process_request(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &mut MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.is_empty() {
            return None;
        }

        // Check for custom handler
        if let Some(ref handler) = state.custom_handler {
            if let Some(response) = handler(unit_id, data) {
                return Some(Self::build_response(transaction_id, unit_id, &response));
            }
        }

        let function_code = data[0];

        match function_code {
            codes::READ_HOLDING_REGISTERS => {
                Self::handle_read_holding_registers(transaction_id, unit_id, &data[1..], state)
            }
            codes::READ_INPUT_REGISTERS => {
                Self::handle_read_input_registers(transaction_id, unit_id, &data[1..], state)
            }
            codes::WRITE_SINGLE_REGISTER => {
                Self::handle_write_single_register(transaction_id, unit_id, &data[1..], state)
            }
            codes::WRITE_MULTIPLE_REGISTERS => {
                Self::handle_write_multiple_registers(transaction_id, unit_id, &data[1..], state)
            }
            codes::READ_COILS => {
                Self::handle_read_coils(transaction_id, unit_id, &data[1..], state)
            }
            codes::READ_DISCRETE_INPUTS => {
                Self::handle_read_discrete_inputs(transaction_id, unit_id, &data[1..], state)
            }
            codes::WRITE_SINGLE_COIL => {
                Self::handle_write_single_coil(transaction_id, unit_id, &data[1..], state)
            }
            _ => Self::build_exception_response(
                transaction_id,
                unit_id,
                function_code,
                exceptions::ILLEGAL_FUNCTION,
            ),
        }
    }

    fn handle_read_holding_registers(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_HOLDING_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let start_addr = u16::from_be_bytes([data[0], data[1]]);
        let count = u16::from_be_bytes([data[2], data[3]]) as usize;

        if count > 125 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_HOLDING_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let byte_count = count * 2;
        let mut response_data = vec![codes::READ_HOLDING_REGISTERS, byte_count as u8];

        for i in 0..count {
            let addr = start_addr + i as u16;
            let value = state.holding_registers.get(&addr).copied().unwrap_or(0);
            response_data.extend_from_slice(&value.to_be_bytes());
        }

        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_read_input_registers(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_INPUT_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let start_addr = u16::from_be_bytes([data[0], data[1]]);
        let count = u16::from_be_bytes([data[2], data[3]]) as usize;

        if count > 125 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_INPUT_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let byte_count = count * 2;
        let mut response_data = vec![codes::READ_INPUT_REGISTERS, byte_count as u8];

        for i in 0..count {
            let addr = start_addr + i as u16;
            let value = state.input_registers.get(&addr).copied().unwrap_or(0);
            response_data.extend_from_slice(&value.to_be_bytes());
        }

        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_write_single_register(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &mut MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::WRITE_SINGLE_REGISTER,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let addr = u16::from_be_bytes([data[0], data[1]]);
        let value = u16::from_be_bytes([data[2], data[3]]);

        state.holding_registers.insert(addr, value);

        // Echo back the request
        let response_data = vec![
            codes::WRITE_SINGLE_REGISTER,
            data[0],
            data[1],
            data[2],
            data[3],
        ];
        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_write_multiple_registers(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &mut MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 5 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::WRITE_MULTIPLE_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let start_addr = u16::from_be_bytes([data[0], data[1]]);
        let count = u16::from_be_bytes([data[2], data[3]]) as usize;
        let byte_count = data[4] as usize;

        if data.len() < 5 + byte_count || count * 2 != byte_count {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::WRITE_MULTIPLE_REGISTERS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        for i in 0..count {
            let addr = start_addr + i as u16;
            let offset = 5 + i * 2;
            let value = u16::from_be_bytes([data[offset], data[offset + 1]]);
            state.holding_registers.insert(addr, value);
        }

        let response_data = vec![
            codes::WRITE_MULTIPLE_REGISTERS,
            data[0],
            data[1], // Start address
            data[2],
            data[3], // Count
        ];
        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_read_coils(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_COILS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let start_addr = u16::from_be_bytes([data[0], data[1]]);
        let count = u16::from_be_bytes([data[2], data[3]]) as usize;

        if count > 2000 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_COILS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let byte_count = (count + 7) / 8;
        let mut response_data = vec![codes::READ_COILS, byte_count as u8];

        for byte_idx in 0..byte_count {
            let mut byte_val = 0u8;
            for bit_idx in 0..8 {
                let coil_idx = byte_idx * 8 + bit_idx;
                if coil_idx < count {
                    let addr = start_addr + coil_idx as u16;
                    if state.coils.get(&addr).copied().unwrap_or(false) {
                        byte_val |= 1 << bit_idx;
                    }
                }
            }
            response_data.push(byte_val);
        }

        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_read_discrete_inputs(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_DISCRETE_INPUTS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let start_addr = u16::from_be_bytes([data[0], data[1]]);
        let count = u16::from_be_bytes([data[2], data[3]]) as usize;

        if count > 2000 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::READ_DISCRETE_INPUTS,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let byte_count = (count + 7) / 8;
        let mut response_data = vec![codes::READ_DISCRETE_INPUTS, byte_count as u8];

        for byte_idx in 0..byte_count {
            let mut byte_val = 0u8;
            for bit_idx in 0..8 {
                let input_idx = byte_idx * 8 + bit_idx;
                if input_idx < count {
                    let addr = start_addr + input_idx as u16;
                    if state.discrete_inputs.get(&addr).copied().unwrap_or(false) {
                        byte_val |= 1 << bit_idx;
                    }
                }
            }
            response_data.push(byte_val);
        }

        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn handle_write_single_coil(
        transaction_id: u16,
        unit_id: u8,
        data: &[u8],
        state: &mut MockModbusState,
    ) -> Option<Vec<u8>> {
        if data.len() < 4 {
            return Self::build_exception_response(
                transaction_id,
                unit_id,
                codes::WRITE_SINGLE_COIL,
                exceptions::ILLEGAL_DATA_VALUE,
            );
        }

        let addr = u16::from_be_bytes([data[0], data[1]]);
        let value = u16::from_be_bytes([data[2], data[3]]);

        state.coils.insert(addr, value == 0xFF00);

        // Echo back the request
        let response_data = vec![codes::WRITE_SINGLE_COIL, data[0], data[1], data[2], data[3]];
        Some(Self::build_response(
            transaction_id,
            unit_id,
            &response_data,
        ))
    }

    fn build_response(transaction_id: u16, unit_id: u8, data: &[u8]) -> Vec<u8> {
        let length = (data.len() + 1) as u16;
        let mut response = Vec::with_capacity(7 + data.len());

        // MBAP header
        response.extend_from_slice(&transaction_id.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes()); // Protocol ID
        response.extend_from_slice(&length.to_be_bytes());
        response.push(unit_id);

        // Data
        response.extend_from_slice(data);

        response
    }

    fn build_exception_response(
        transaction_id: u16,
        unit_id: u8,
        function_code: u8,
        exception_code: u8,
    ) -> Option<Vec<u8>> {
        let data = vec![function_code | 0x80, exception_code];
        Some(Self::build_response(transaction_id, unit_id, &data))
    }
}

impl Drop for MockModbusServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_server_starts_and_stops() {
        let server = MockModbusServer::start(MockModbusConfig::default()).unwrap();
        assert!(server.port() > 0);
        server.stop();
    }

    #[test]
    fn test_mock_server_auto_port() {
        let server1 = MockModbusServer::start(MockModbusConfig::default()).unwrap();
        let server2 = MockModbusServer::start(MockModbusConfig::default()).unwrap();

        assert_ne!(server1.port(), server2.port());

        server1.stop();
        server2.stop();
    }

    #[test]
    fn test_set_and_get_holding_register() {
        let server = MockModbusServer::start(MockModbusConfig::default()).unwrap();

        server.set_holding_register(100, 42);
        assert_eq!(server.get_holding_register(100), Some(42));

        server.stop();
    }

    #[test]
    fn test_request_count_increments() {
        let server = MockModbusServer::start(MockModbusConfig::default()).unwrap();

        // Initially no requests
        assert_eq!(server.request_count(), 0);

        server.stop();
    }
}
