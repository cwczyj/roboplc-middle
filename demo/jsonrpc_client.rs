//! JSON-RPC 客户端示例
//!
//! 用于演示如何通过 JSON-RPC 与 roboplc-middleware 通信
//!
//! 使用方式:
//!   cargo run --bin jsonrpc_client
//! 或
//!   cargo run --bin jsonrpc_client -- read motor_control
//!   cargo run --bin jsonrpc_client -- write motor_control motor_speed 2000

use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// roboplc-rpc 请求结构 (使用标准 JSON-RPC 2.0 格式：method, params, id)
/// 这是 roboplc_rpc crate 的自定义格式
#[derive(serde::Serialize)]
struct RoboRpcRequest<'a> {
    jsonrpc: &'a str,
    #[serde(rename = "method")]
    method: &'a str,
    #[serde(rename = "params")]
    params: serde_json::Value,
    #[serde(rename = "id")]
    id: u64,
}

/// roboplc-rpc 响应结构 (使用标准 JSON-RPC 2.0 格式：id, result, error)
/// 这是 roboplc_rpc crate 的自定义格式
#[derive(serde::Deserialize, Debug)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default, rename = "i")]
    id: u64,
    #[serde(default, rename = "result")]
    result: Option<serde_json::Value>,
    #[serde(default, rename = "error")]
    error: Option<JsonRpcError>,
}

#[derive(serde::Deserialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
}

const RPC_HOST: &str = "127.0.0.1:8080";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    println!("🔧 JSON-RPC 客户端 - roboplc-middleware");
    println!("   RPC 地址：{}\n", RPC_HOST);

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "read" => {
            if args.len() < 3 {
                eprintln!("❌ 错误：请指定要读取的信号组名称");
                eprintln!("   用法：cargo run --bin jsonrpc_client -- read <signal_group>");
                return Ok(());
            }
            let signal_group = &args[2];
            read_signal_group(signal_group)?;
        }
        "write" => {
            if args.len() < 5 {
                eprintln!("❌ 错误：请指定信号组、字段名和值");
                eprintln!("   用法：cargo run --bin jsonrpc_client -- write <signal_group> <field> <value>");
                return Ok(());
            }
            let signal_group = &args[2];
            let field = &args[3];
            let value: u16 = args[4].parse()?;
            write_signal_field(signal_group, field, value)?;
        }
        "list" => {
            list_devices()?;
        }
        "status" => {
            get_system_status()?;
        }
        "interactive" => {
            interactive_mode()?;
        }
        _ => {
            eprintln!("❌ 未知命令：{}", args[1]);
            print_help();
        }
    }

    Ok(())
}

fn print_help() {
    println!("用法:");
    println!("  cargo run --bin jsonrpc_client -- <command> [arguments]");
    println!();
    println!("命令:");
    println!("  read <signal_group>          读取信号组数据");
    println!("  write <signal_group> <field> <value>  写入信号字段");
    println!("  list                         列出所有设备");
    println!("  status                       获取系统状态");
    println!("  interactive                  交互模式");
    println!();
    println!("示例:");
    println!("  读取电机控制信号组");
    println!("  cargo run --bin jsonrpc_client -- read motor_control");
    println!();
    println!("  读取温度传感器信号组");
    println!("  cargo run --bin jsonrpc_client -- read temperature_sensor");
    println!();
    println!("  写入电机速度");
    println!("  cargo run --bin jsonrpc_client -- write motor_control motor_speed 2000");
    println!();
    println!("  列出所有设备");
    println!("  cargo run --bin jsonrpc_client -- list");
    println!();
    println!("  获取系统状态");
    println!("  cargo run --bin jsonrpc_client -- status");
}

/// 读取信号组
/// 注意：RPC worker 期望的参数名是 device_id 和 group_name
fn read_signal_group(signal_group: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("📖 读取信号组：{}", signal_group);

    let params = serde_json::json!({
        "device_id": "mock-device",
        "group_name": signal_group
    });

    let request = RoboRpcRequest {
        jsonrpc: "2.0",
        method: "read_signal_group",
        params,
        id: 1,
    };

    let response = send_request(&request)?;

    if let Some(result) = response.result {
        println!("✅ 响应：{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(error) = response.error {
        println!("❌ 错误：{} (code: {})", error.message, error.code);
    }

    Ok(())
}

/// 写入信号字段
/// 注意：RPC worker 期望的参数名是 device_id, group_name 和 data
fn write_signal_field(
    signal_group: &str,
    field: &str,
    value: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("✏️ 写入信号：{}.{} = {}", signal_group, field, value);

    // RPC worker 期望写入数据的格式是 { "group_name": xxx, "data": { ... } }
    let params = serde_json::json!({
        "device_id": "mock-device",
        "group_name": signal_group,
        "data": {
            "field": field,
            "value": value
        }
    });

    let request = RoboRpcRequest {
        jsonrpc: "2.0",
        method: "write_signal_group",
        params,
        id: 2,
    };

    let response = send_request(&request)?;

    if let Some(result) = response.result {
        println!("✅ 响应：{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(error) = response.error {
        println!("❌ 错误：{} (code: {})", error.message, error.code);
    }

    Ok(())
}

/// 列出所有设备
fn list_devices() -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 列出所有设备");

    let params = serde_json::json!({});

    let request = RoboRpcRequest {
        jsonrpc: "2.0",
        method: "get_device_list",
        params,
        id: 3,
    };

    let response = send_request(&request)?;

    if let Some(result) = response.result {
        println!("✅ 设备列表：{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(error) = response.error {
        println!("❌ 错误：{} (code: {})", error.message, error.code);
    }

    Ok(())
}

/// 获取系统状态
/// 注意：get_status 需要 device_id 参数
fn get_system_status() -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 获取系统状态");

    let params = serde_json::json!({
        "device_id": "mock-device"
    });

    let request = RoboRpcRequest {
        jsonrpc: "2.0",
        method: "get_status",
        params,
        id: 4,
    };

    let response = send_request(&request)?;

    if let Some(result) = response.result {
        println!("✅ 系统状态：{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(error) = response.error {
        println!("❌ 错误：{} (code: {})", error.message, error.code);
    }

    Ok(())
}

/// 交互模式
fn interactive_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 进入交互模式 (输入 'quit' 退出)\n");

    let mut input = String::new();

    loop {
        print!("> ");
        io::stdout().flush()?;

        input.clear();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let input = input.trim();
        if input == "quit" || input == "exit" {
            println!("再见！");
            break;
        }

        if input.is_empty() {
            continue;
        }

        // 简单解析命令
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "read" => {
                if parts.len() > 1 {
                    read_signal_group(parts[1])?;
                } else {
                    println!("用法：read <signal_group>");
                }
            }
            "write" => {
                if parts.len() > 3 {
                    if let Ok(value) = parts[3].parse::<u16>() {
                        write_signal_field(parts[1], parts[2], value)?;
                    } else {
                        println!("错误：值必须是数字");
                    }
                } else {
                    println!("用法：write <signal_group> <field> <value>");
                }
            }
            "list" => list_devices()?,
            "status" => get_system_status()?,
            "help" => print_help(),
            _ => println!("未知命令：{}", parts[0]),
        }
    }

    Ok(())
}

/// 发送 JSON-RPC 请求 - 使用 TCP 连接直接发送（roboplc-rpc 使用 TCP 协议，不是 HTTP）
fn send_request(request: &RoboRpcRequest) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
    let json = serde_json::to_string(&request)?;

    println!("📤 发送：{}", json);

    // 直接连接到 RPC 端口（TCP 模式，不是 HTTP）
    let mut stream = TcpStream::connect_timeout(
        &RPC_HOST.parse::<std::net::SocketAddr>()?,
        Duration::from_secs(5),
    )?;

    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    // 发送 JSON 数据
    stream.write_all(json.as_bytes())?;

    // 关闭写入端，告诉服务器我们发送完了
    stream.shutdown(std::net::Shutdown::Write)?;

    // 读取响应
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer)?;

    let text = String::from_utf8_lossy(&buffer).to_string();
    println!("📥 接收：{}\n", text);

    let parsed: JsonRpcResponse = serde_json::from_str(&text)?;
    Ok(parsed)
}
