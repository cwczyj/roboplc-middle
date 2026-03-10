//! Mock Modbus Server
//!
//! 独立的 Mock Modbus TCP 服务器程序，用于测试 roboplc-middleware
//!
//! 启动方式:
//!   cargo run --bin mock_server
//! 或
//!   ROBOPLC_MOCK_PORT=5555 cargo run --bin mock_server

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
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
}

/// Modbus exception codes
mod exceptions {
    pub const ILLEGAL_FUNCTION: u8 = 0x01;
    pub const ILLEGAL_DATA_ADDRESS: u8 = 0x02;
    pub const ILLEGAL_DATA_VALUE: u8 = 0x03;
    pub const SERVER_DEVICE_FAILURE: u8 = 0x04;
}

/// Mock Modbus 状态
#[derive(Default)]
pub struct MockModbusState {
    pub holding_registers: HashMap<u16, u16>,
    pub input_registers: HashMap<u16, u16>,
    pub coils: HashMap<u16, bool>,
    pub discrete_inputs: HashMap<u16, bool>,
    pub request_count: usize,
    pub fail_next: bool,
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
        }
    }
}

/// Mock Modbus TCP Server
pub struct MockModbusServer {
    port: u16,
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    registers: Arc<std::sync::Mutex<MockModbusState>>,
}

impl MockModbusServer {
    /// 启动 Mock Modbus 服务器
    pub fn start(port: u16) -> Result<Self, std::io::Error> {
        let running = Arc::new(AtomicBool::new(true));
        let registers = Arc::new(std::sync::Mutex::new(MockModbusState::new()));

        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr)?;
        let actual_port = listener.local_addr()?.port();

        listener.set_nonblocking(true)?;

        let running_clone = running.clone();
        let registers_clone = registers.clone();

        let handle = thread::spawn(move || {
            Self::server_loop(listener, running_clone, registers_clone);
        });

        thread::sleep(Duration::from_millis(10));

        println!("✅ Mock Modbus Server 启动在端口：{}", actual_port);
        println!("   地址：127.0.0.1:{}", actual_port);

        Ok(Self {
            port: actual_port,
            running,
            thread_handle: Some(handle),
            registers,
        })
    }

    /// 获取服务器端口
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 停止服务器
    pub fn stop(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        println!("Mock Modbus Server 已停止");
    }

    /// 设置保持寄存器的值
    pub fn set_holding_register(&self, address: u16, value: u16) {
        let mut state = self.registers.lock().unwrap();
        state.holding_registers.insert(address, value);
    }

    /// 批量设置保持寄存器
    pub fn set_holding_registers(&self, start: u16, values: &[u16]) {
        let mut state = self.registers.lock().unwrap();
        for (i, &value) in values.iter().enumerate() {
            state.holding_registers.insert(start + i as u16, value);
        }
    }

    /// 初始化 demo 数据（电机控制和温度传感器）
    pub fn init_demo_data(&self) {
        // 电机控制寄存器 (h100-h104)
        self.set_holding_register(100, 1500); // motor_speed = 1500 RPM
        self.set_holding_register(101, 1); // motor_status = 运行中
        self.set_holding_register(102, 1); // motor_direction = 正转
        self.set_holding_register(103, 0); // error_code = 无错误
        self.set_holding_register(104, 0); // fault_flag = 无故障

        // 温度传感器寄存器 (h200-h209)
        // F32 温度 1 = 25.5°C (0x41CC0000 = 42.0, 使用整数近似)
        self.set_holding_register(200, 0x41CC); // temperature_1 高字节
        self.set_holding_register(201, 0x0000); // temperature_1 低字节
                                                // F32 温度 2 = 30.2°C
        self.set_holding_register(202, 0x41F1); // temperature_2 高字节
        self.set_holding_register(203, 0x3333); // temperature_2 低字节
        self.set_holding_register(204, 65); // humidity = 65%
        self.set_holding_register(205, 1); // sensor_status = 正常
        self.set_holding_register(206, 0); // alarm_code = 无报警

        println!("   Demo 数据已初始化:");
        println!("   - 电机速度：1500 RPM, 状态：运行中，方向：正转");
        println!("   - 温度 1: 25.5°C, 温度 2: 30.2°C, 湿度：65%");
    }

    fn server_loop(
        listener: TcpListener,
        running: Arc<AtomicBool>,
        registers: Arc<std::sync::Mutex<MockModbusState>>,
    ) {
        while running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    println!("📡 新连接：{}", addr);
                    let running_clone = running.clone();
                    let registers_clone = registers.clone();

                    thread::spawn(move || {
                        Self::handle_connection(stream, running_clone, registers_clone);
                    });
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
    ) {
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

        while running.load(Ordering::SeqCst) {
            let mut header = [0u8; 7];
            match stream.read_exact(&mut header) {
                Ok(_) => {}
                Err(_) => break,
            }

            // 解析 MBAP 头部
            let transaction_id = u16::from_be_bytes([header[0], header[1]]);
            let protocol_id = u16::from_be_bytes([header[2], header[3]]);
            let length = u16::from_be_bytes([header[4], header[5]]);
            let unit_id = header[6];

            if protocol_id != 0 {
                continue;
            }

            let data_len = length as usize - 1;
            if data_len > 260 {
                break;
            }

            let mut data = vec![0u8; data_len];
            if stream.read_exact(&mut data).is_err() {
                break;
            }

            // 处理请求
            let response = {
                let mut state = registers.lock().unwrap();
                state.request_count += 1;
                Self::process_request(transaction_id, unit_id, &data, &mut state)
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

        println!("📝 写入寄存器：地址={}，值={}", addr, value);
        state.holding_registers.insert(addr, value);

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

        println!("📝 批量写入寄存器：起始地址={}，数量={}", start_addr, count);
        for i in 0..count {
            let addr = start_addr + i as u16;
            let offset = 5 + i * 2;
            let value = u16::from_be_bytes([data[offset], data[offset + 1]]);
            state.holding_registers.insert(addr, value);
        }

        let response_data = vec![
            codes::WRITE_MULTIPLE_REGISTERS,
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

        response.extend_from_slice(&transaction_id.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&length.to_be_bytes());
        response.push(unit_id);
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

fn main() {
    let port: u16 = std::env::var("ROBOPLC_MOCK_PORT")
        .unwrap_or_else(|_| "5555".to_string())
        .parse()
        .expect("无效的端口号");

    println!("🚀 启动 Mock Modbus Server...");

    match MockModbusServer::start(port) {
        Ok(server) => {
            server.init_demo_data();
            println!("\n按 Ctrl+C 停止服务器\n");

            // 保持运行直到 Ctrl+C
            loop {
                thread::sleep(Duration::from_secs(1));
            }
        }
        Err(e) => {
            eprintln!("❌ 启动失败：{}", e);
            std::process::exit(1);
        }
    }
}
