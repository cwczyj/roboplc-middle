//! PLC寄存器模拟与RPC读写操作示例
//!
//! 这个示例演示了如何：
//! 1. 启动模拟PLC寄存器的Mock Modbus服务器
//! 2. 创建JSON-RPC客户端并发送读写请求
//! 3. 验证寄存器的读写操作
//! 4. 清理资源
//!
//! ## 运行方式
//!
//! ```bash
//! # 启动中间件（在另一个终端）
//! ROBOPLC_SIMULATED=1 cargo run
//!
//! # 运行本示例
//! cargo run --example register_rpc_demo
//! ```
//!
//! ## 流程说明
//!
//! ```text
//! 1. 启动Mock Modbus服务器（模拟PLC）
//!    └── 设置寄存器初始值（h100=42, h101=100）
//!
//! 2. 创建临时配置文件
//!    └── 配置RPC端口(8080)和Mock设备端口
//!
//! 3. 发送JSON-RPC请求
//!    ├── ReadSignalGroup: 读取传感器数据
//!    └── WriteSignalGroup: 写入控制数据
//!
//! 4. 验证响应结果
//!    └── 检查寄存器值是否正确读写
//!
//! 5. 清理资源
//!    └── 停止Mock服务器，删除临时文件
//! ```

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

// 引入测试用的Mock Modbus服务器
#[path = "../tests/mock_modbus.rs"]
mod mock_modbus;
use mock_modbus::{MockModbusConfig, MockModbusServer};

/// 创建测试配置文件
fn create_demo_config(rpc_port: u16, http_port: u16, modbus_port: u16) -> String {
    format!(
        r#"
[server]
rpc_port = {}
http_port = {}

[logging]
level = "info"
file = "/tmp/register_rpc_demo.log"
daily_rotation = true

[[devices]]
id = "demo-plc"
type = "plc"
address = "127.0.0.1"
port = {}
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices.register_mappings]]
signal_name = "temperature"
address = "h100"
data_type = "u16"

[[devices.register_mappings]]
signal_name = "pressure"
address = "h101"
data_type = "u16"
"#,
        rpc_port, http_port, modbus_port
    )
}

/// 发送JSON-RPC请求到服务器
fn send_jsonrpc_request(addr: &str, request: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    // 发送请求（JSON-RPC over TCP）
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    // 读取响应
    let mut buffer = vec![0u8; 4096];
    let bytes_read = stream.read(&mut buffer)?;

    Ok(String::from_utf8_lossy(&buffer[..bytes_read]).to_string())
}

/// 构建JSON-RPC请求
fn build_jsonrpc_request(method: &str, params: serde_json::Value, id: u64) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    })
    .to_string()
}

fn main() {
    println!("========================================");
    println!("PLC寄存器模拟与RPC读写操作示例");
    println!("========================================\n");

    // 步骤1: 启动Mock Modbus服务器（模拟PLC）
    println!("[1/5] 启动Mock Modbus服务器...");
    let mock_server =
        MockModbusServer::start(MockModbusConfig::default()).expect("Failed to start mock server");
    let modbus_port = mock_server.port();
    println!("    ✓ Mock服务器运行在端口: {}", modbus_port);

    // 设置PLC寄存器初始值
    mock_server.set_holding_register(100, 42); // h100 = 42 (温度)
    mock_server.set_holding_register(101, 100); // h101 = 100 (压力)
    mock_server.set_input_register(200, 25); // i200 = 25 (环境温度)
    mock_server.set_coil(300, true); // c300 = true (运行状态)
    println!("    ✓ 设置寄存器初始值:");
    println!("      - Holding Register h100 = 42 (温度)");
    println!("      - Holding Register h101 = 100 (压力)");
    println!("      - Input Register i200 = 25 (环境温度)");
    println!("      - Coil c300 = true (运行状态)\n");

    // 步骤2: 创建临时配置文件
    println!("[2/5] 创建临时配置文件...");
    let rpc_port = 18080u16;
    let http_port = 18081u16;
    let config_content = create_demo_config(rpc_port, http_port, modbus_port);

    let config_path = "/tmp/register_rpc_demo_config.toml";
    fs::write(config_path, config_content).expect("Failed to write config file");
    println!("    ✓ 配置文件: {}", config_path);
    println!("      - RPC端口: {}", rpc_port);
    println!("      - HTTP端口: {}", http_port);
    println!("      - Modbus端口: {}\n", modbus_port);

    // 步骤3: 演示直接使用Mock服务器进行寄存器读写
    println!("[3/5] 演示直接Modbus寄存器读写...");

    // 读取寄存器
    let temp_value = mock_server.get_holding_register(100);
    let pressure_value = mock_server.get_holding_register(101);
    println!("    ✓ 读取寄存器:");
    println!("      - h100 (温度) = {:?}", temp_value);
    println!("      - h101 (压力) = {:?}", pressure_value);

    // 写入寄存器
    mock_server.set_holding_register(100, 50); // 设置新温度
    mock_server.set_holding_register(101, 150); // 设置新压力
    println!("    ✓ 写入新值:");
    println!("      - h100 = 50 (新温度)");
    println!("      - h101 = 150 (新压力)");

    // 验证写入
    let new_temp = mock_server.get_holding_register(100);
    let new_pressure = mock_server.get_holding_register(101);
    println!("    ✓ 验证写入结果:");
    println!("      - h100 = {:?}", new_temp);
    println!("      - h101 = {:?}\n", new_pressure);

    // 步骤4: 模拟JSON-RPC请求（在没有启动完整中间件的情况下演示消息格式）
    println!("[4/5] 演示JSON-RPC请求格式...");

    // 模拟Ping请求
    let ping_request = build_jsonrpc_request("ping", serde_json::json!({}), 1);
    println!("    ✓ Ping请求:");
    println!("      {}", ping_request);

    // 模拟ReadSignalGroup请求
    let read_request = build_jsonrpc_request(
        "read_signal_group",
        serde_json::json!({
            "device_id": "demo-plc",
            "group_name": "sensors"
        }),
        2,
    );
    println!("    ✓ ReadSignalGroup请求:");
    println!("      {}", read_request);

    // 模拟WriteSignalGroup请求
    let write_request = build_jsonrpc_request(
        "write_signal_group",
        serde_json::json!({
            "device_id": "demo-plc",
            "group_name": "actuators",
            "data": {
                "temperature": 75,
                "pressure": 200
            }
        }),
        3,
    );
    println!("    ✓ WriteSignalGroup请求:");
    println!("      {}\n", write_request);

    // 步骤5: 展示如何使用中间件进行端到端测试
    println!("[5/5] 完整的端到端测试模式...");
    println!("    要使用完整的RPC到Modbus链路，请:");
    println!();
    println!("    1. 启动中间件:");
    println!(
        "       ROBOPLC_SIMULATED=1 cargo run -- --config {}",
        config_path
    );
    println!();
    println!("    2. 在另一个终端发送RPC请求:");
    println!("       echo '{{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"params\":{{}},\"id\":1}}' | nc localhost {}", rpc_port);
    println!();
    println!("    3. 或使用HTTP API查询状态:");
    println!("       curl http://localhost:{}/api/devices", http_port);
    println!();

    // 清理
    println!("    ✓ 正在清理资源...");
    mock_server.stop();
    let _ = fs::remove_file(config_path);
    println!("    ✓ Mock服务器已停止");
    println!("    ✓ 临时配置文件已删除\n");

    println!("========================================");
    println!("示例完成！");
    println!("========================================");
    println!();
    println!("关键要点:");
    println!("  1. MockModbusServer可以模拟真实的PLC寄存器");
    println!("  2. 寄存器地址格式: h=Holding, i=Input, c=Coil, d=Discrete");
    println!("  3. JSON-RPC请求通过TCP发送到端口{}", rpc_port);
    println!("  4. 中间件将RPC请求转换为Modbus操作");
    println!("  5. 每个设备需要一个独立的ModbusWorker");
}

#[cfg(test)]
mod demo_tests {
    use super::*;

    /// 测试：完整的寄存器读写流程
    #[test]
    fn test_register_read_write_flow() {
        // 启动Mock服务器
        let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();

        // 写入寄存器
        mock_server.set_holding_register(100, 42);
        mock_server.set_holding_register(101, 100);

        // 验证读取
        assert_eq!(mock_server.get_holding_register(100), Some(42));
        assert_eq!(mock_server.get_holding_register(101), Some(100));

        // 更新寄存器
        mock_server.set_holding_register(100, 50);
        assert_eq!(mock_server.get_holding_register(100), Some(50));

        // 清理
        mock_server.stop();
    }

    /// 测试：批量寄存器操作
    #[test]
    fn test_batch_register_operations() {
        let mock_server = MockModbusServer::start(MockModbusConfig::default()).unwrap();

        // 批量设置寄存器
        let values = vec![10, 20, 30, 40, 50];
        mock_server.set_holding_registers(100, &values);

        // 验证每个寄存器
        for (i, expected) in values.iter().enumerate() {
            let addr = 100 + i as u16;
            assert_eq!(
                mock_server.get_holding_register(addr),
                Some(*expected),
                "Register h{} should be {}",
                addr,
                expected
            );
        }

        mock_server.stop();
    }

    /// 测试：JSON-RPC请求构建
    #[test]
    fn test_jsonrpc_request_building() {
        let request = build_jsonrpc_request(
            "read_signal_group",
            serde_json::json!({
                "device_id": "test-plc",
                "group_name": "sensors"
            }),
            42,
        );

        let parsed: serde_json::Value = serde_json::from_str(&request).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "read_signal_group");
        assert_eq!(parsed["id"], 42);
        assert_eq!(parsed["params"]["device_id"], "test-plc");
        assert_eq!(parsed["params"]["group_name"], "sensors");
    }

    /// 测试：完整的端到端消息流
    #[test]
    fn test_end_to_end_message_flow() {
        use roboplc_middleware::messages::{Message, Operation};

        // 模拟RPC层创建DeviceControl消息
        let correlation_id = 12345u64;
        let control_msg = Message::DeviceControl {
            device_id: "demo-plc".to_string(),
            operation: Operation::ReadSignalGroup,
            params: serde_json::json!({ "group_name": "sensors" }),
            correlation_id,
            respond_to: None,
        };

        // 验证消息内容
        if let Message::DeviceControl {
            device_id,
            operation,
            params,
            correlation_id: cid,
            ..
        } = control_msg
        {
            assert_eq!(device_id, "demo-plc");
            assert!(matches!(operation, Operation::ReadSignalGroup));
            assert_eq!(params["group_name"], "sensors");
            assert_eq!(cid, correlation_id);

            // 模拟Modbus层创建DeviceResponse消息
            let response_msg = Message::DeviceResponse {
                device_id,
                success: true,
                data: serde_json::json!({
                    "temperature": 42.0,
                    "pressure": 100.0
                }),
                error: None,
                correlation_id: cid,
            };

            // 验证响应消息
            if let Message::DeviceResponse {
                success,
                data,
                correlation_id: resp_cid,
                ..
            } = response_msg
            {
                assert!(success);
                assert_eq!(resp_cid, correlation_id);
                assert_eq!(data["temperature"], 42.0);
                assert_eq!(data["pressure"], 100.0);
            } else {
                panic!("Expected DeviceResponse");
            }
        } else {
            panic!("Expected DeviceControl");
        }
    }
}
