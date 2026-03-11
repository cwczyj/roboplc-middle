# Mock Modbus 测试指南

本指南说明如何使用 Mock Modbus 服务器测试 roboplc-middleware 中间件。

## 目录

- [快速开始](#快速开始)
- [详细步骤](#详细步骤)
- [API 使用示例](#api-使用示例)
- [常见问题](#常见问题)

---

## 快速开始

### 1. 准备测试配置文件

创建 `config-mock.toml`:

```toml
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/tmp/roboplc-middleware-test.log"
daily_rotation = false

# 配置连接到 Mock Modbus 服务器
[[devices]]
id = "mock-device"
type = "plc"
address = "127.0.0.1"
port = 5555
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

# 信号组 1: 电机控制
[[devices.signal_groups]]
name = "motor_control"
description = "电机控制寄存器"
register_address = "h100"
register_count = 5

[[devices.signal_groups.fields]]
name = "motor_speed"
data_type = "U16"
offset = 0

[[devices.signal_groups.fields]]
name = "motor_status"
data_type = "U16"
offset = 1

[[devices.signal_groups.fields]]
name = "motor_direction"
data_type = "Bool"
offset = 2

[[devices.signal_groups.fields]]
name = "error_code"
data_type = "U16"
offset = 3

[[devices.signal_groups.fields]]
name = "fault_flag"
data_type = "Bool"
offset = 4

# 信号组 2: 传感器数据
[[devices.signal_groups]]
name = "temperature_sensor"
description = "温度传感器数据"
register_address = "h200"
register_count = 10

[[devices.signal_groups.fields]]
name = "temperature_1"
data_type = "F32"
offset = 0

[[devices.signal_groups.fields]]
name = "temperature_2"
data_type = "F32"
offset = 2

[[devices.signal_groups.fields]]
name = "humidity"
data_type = "U16"
offset = 4

[[devices.signal_groups.fields]]
name = "sensor_status"
data_type = "U16"
offset = 5

[[devices.signal_groups.fields]]
name = "alarm_code"
data_type = "U16"
offset = 6
```

### 2. 启动 Mock Modbus 服务器

项目已经包含了一个完整的 Rust 实现的 Mock Modbus 服务器（位于 `tests/mock_modbus.rs`）。

**方式一: 运行示例程序（推荐）**

使用 `demo/register_rpc_demo.rs` 中的示例，它会自动启动 Mock Modbus 服务器并演示完整的测试流程：

```bash
# 运行示例程序（会自动启动 Mock Modbus 服务器）
cargo run --bin register_rpc_demo
```

这个示例会：
1. 自动启动 Mock Modbus 服务器
2. 设置测试寄存器值
3. 演示 JSON-RPC 请求格式
4. 清理资源

**方式二: 通过测试启动**

如果你想运行完整的集成测试，可以：

```bash
# 运行集成测试（会启动 Mock Modbus 服务器）
cargo test --test integration_tests

# 运行 E2E 测试
cargo test --test e2e_tests

# 运行异步 RPC 测试
cargo test --test async_rpc_tests
```

**方式三: 创建自定义 Mock 服务器**

如果你需要创建自己的 Mock Modbus 服务器用于测试，可以参考 `demo/register_rpc_demo.rs` 的实现：

```rust
use std::thread;
use std::time::Duration;

// 引入测试用的 Mock Modbus 服务器
#[path = "../tests/mock_modbus.rs"]
mod mock_modbus;
use mock_modbus::{MockModbusConfig, MockModbusServer};

fn main() {
    // 启动 Mock Modbus 服务器
    let config = MockModbusConfig {
        port: 5555,  // 指定端口（0 = 自动分配）
        unit_id: 1,
        response_delay_ms: 0,  // 响应延迟（毫秒）
        accept_connections: true,
        drop_after_requests: None,  // 在 N 个请求后断开连接
    };
    
    let mock_server = MockModbusServer::start(config)
        .expect("Failed to start mock server");
    
    println!("Mock Modbus 服务器运行在端口: {}", mock_server.port());
    
    // 设置寄存器初始值
    mock_server.set_holding_register(100, 42);
    mock_server.set_holding_register(101, 100);
    mock_server.set_input_register(200, 25);
    mock_server.set_coil(300, true);
    
    // 服务器会在后台运行，直到调用 stop()
    thread::sleep(Duration::from_secs(60));
    
    // 停止服务器
    mock_server.stop();
}
```

编译并运行：
```bash
cargo run --bin mock_server
```

### 3. 启动中间件

在另一个终端:

```bash
# 编译项目
cargo build --release

# 启动中间件(开发模式,跳过实时调度)
ROBOPLC_SIMULATED=1 ./target/release/roboplc-middleware
```

### 4. 测试连接

```bash
# 检查设备状态
curl http://localhost:8081/api/devices

# 检查健康状态
curl http://localhost:8081/api/health
```

---

## 详细步骤

### 步骤 1: 配置 Mock Modbus 服务器

Mock Modbus 服务器模拟真实 Modbus TCP 设备的行为:

- **端口**: 默认 5555 (可在配置中修改)
- **Unit ID**: 默认 1
- **寄存器映射**:
  - `h100-h104`: 电机控制信号组
  - `h200-h209`: 温度传感器信号组

### 步骤 2: 启动中间件

中间件会自动连接到配置的设备:

```bash
# 检查日志确认连接成功
tail -f /tmp/roboplc-middleware-test.log
```

预期输出:
```
INFO ModbusWorker[mock-device]: Connected to 127.0.0.1:5555
INFO ModbusWorker[mock-device]: Starting heartbeat
```

### 步骤 3: 验证连接状态

```bash
# 查看所有设备
curl http://localhost:8081/api/devices | jq
```

预期输出:
```json
{
  "devices": [
    {
      "id": "mock-device",
      "connected": true,
      "last_communication_ms": 1234,
      "error_count": 0
    }
  ]
}
```

---

## API 使用示例

### JSON-RPC API (端口 8080)

#### 1. Ping 测试

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "ping",
    "p": {}
  }'
```

响应:
```json
{
  "result": {
    "success": true
  }
}
```

#### 2. 获取设备列表

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "get_device_list",
    "p": {}
  }'
```

响应:
```json
{
  "result": {
    "devices": ["mock-device"]
  }
}
```

#### 3. 获取设备状态

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "get_status",
    "p": {"device_id": "mock-device"}
  }'
```

响应:
```json
{
  "result": {
    "connected": true,
    "last_communication_ms": 1234,
    "error_count": 0
  }
}
```

#### 4. 读取信号组 (ReadSignalGroup)

读取电机控制信号组:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "read_signal_group",
    "p": {"device_id": "mock-device", "group_name": "motor_control"}
  }'
```

响应:
```json
{
  "result": {
    "data": {
      "motor_speed": 1500,
      "motor_status": 1,
      "motor_direction": false,
      "error_code": 0,
      "fault_flag": false
    }
  }
}
```

读取温度传感器信号组:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "read_signal_group",
    "p": {"device_id": "mock-device", "group_name": "temperature_sensor"}
  }'
```

响应:
```json
{
  "result": {
    "data": {
      "temperature_1": 25.0,
      "temperature_2": 50.0,
      "humidity": 65,
      "sensor_status": 1,
      "alarm_code": 0
    }
  }
}
```

#### 5. 写入信号组 (WriteSignalGroup)

写入电机控制参数:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "write_signal_group",
    "p": {
      "device_id": "mock-device",
      "group_name": "motor_control",
      "data": {
        "motor_speed": 2000,
        "motor_status": 1,
        "motor_direction": true,
        "error_code": 0,
        "fault_flag": false
      }
    }
  }'
```

响应:
```json
{
  "result": {
    "data": {
      "group_name": "motor_control",
      "result": {
        "writes": 5,
        "latency_us": 1500
      }
    }
  }
}
```

验证写入结果:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "read_signal_group",
    "p": {"device_id": "mock-device", "group_name": "motor_control"}
  }'
```

### HTTP RESTful API (端口 8081)

#### 1. 获取设备列表

```bash
curl http://localhost:8081/api/devices | jq
```

#### 2. 获取设备状态

```bash
curl http://localhost:8081/api/devices/mock-device/status | jq
```

#### 3. 健康检查

```bash
curl http://localhost:8081/api/health | jq
```

#### 4. 查询配置

```bash
curl http://localhost:8081/api/config | jq
```

---

## Python 客户端示例

### JSON-RPC 客户端

```python
#!/usr/bin/env python3
import requests
import json

class RpcClient:
    def __init__(self, url="http://localhost:8080/jsonrpc"):
        self.url = url
        self.id_counter = 0
    
    def call(self, method, params=None):
        """调用 JSON-RPC 方法"""
        self.id_counter += 1
        payload = {
            "m": method,
            "p": params or {}
        }
        
        response = requests.post(self.url, json=payload)
        result = response.json()
        
        if "error" in result:
            raise Exception(f"RPC Error: {result['error']}")
        
        return result.get("result")
    
    def ping(self):
        """Ping 测试"""
        return self.call("ping")
    
    def get_device_list(self):
        """获取设备列表"""
        return self.call("get_device_list")
    
    def get_status(self, device_id):
        """获取设备状态"""
        return self.call("get_status", {"device_id": device_id})
    
    def read_signal_group(self, device_id, group_name):
        """读取信号组"""
        return self.call("read_signal_group", 
                        {"device_id": device_id, "group_name": group_name})
    
    def write_signal_group(self, device_id, group_name, data):
        """写入信号组"""
        return self.call("write_signal_group",
                        {"device_id": device_id, "group_name": group_name, "data": data})

def main():
    # 创建客户端
    client = RpcClient()
    
    # Ping 测试
    print("=== Ping ===")
    result = client.ping()
    print(f"Result: {result}")
    
    # 获取设备列表
    print("\n=== Device List ===")
    devices = client.get_device_list()
    print(f"Devices: {devices['devices']}")
    
    device_id = "mock-device"
    
    # 获取设备状态
    print(f"\n=== Status of {device_id} ===")
    status = client.get_status(device_id)
    print(f"Status: {status}")
    
    # 读取电机控制信号组
    print(f"\n=== Read motor_control ===")
    motor_data = client.read_signal_group(device_id, "motor_control")
    print(f"Data: {json.dumps(motor_data['data'], indent=2)}")
    
    # 读取温度传感器信号组
    print(f"\n=== Read temperature_sensor ===")
    temp_data = client.read_signal_group(device_id, "temperature_sensor")
    print(f"Data: {json.dumps(temp_data['data'], indent=2)}")
    
    # 写入电机控制参数
    print(f"\n=== Write motor_control ===")
    write_data = {
        "motor_speed": 3000,
        "motor_status": 1,
        "motor_direction": True,
        "error_code": 0,
        "fault_flag": False
    }
    result = client.write_signal_group(device_id, "motor_control", write_data)
    print(f"Write result: {result}")
    
    # 验证写入
    print(f"\n=== Verify write ===")
    motor_data = client.read_signal_group(device_id, "motor_control")
    print(f"Data: {json.dumps(motor_data['data'], indent=2)}")

if __name__ == '__main__':
    main()
```

运行:
```bash
python3 rpc_client.py
```

---

## 上位机集成示例

### C# 客户端

```csharp
using System;
using System.Net.Http;
using System.Text;
using System.Threading.Tasks;
using Newtonsoft.Json;

public class RpcClient
{
    private readonly HttpClient _client = new HttpClient();
    private readonly string _url;
    private int _idCounter = 0;

    public RpcClient(string url = "http://localhost:8080/jsonrpc")
    {
        _url = url;
    }

    public async Task<T> CallAsync<T>(string method, object? parameters = null)
    {
        var payload = new
        {
            m = method,
            p = parameters ?? new {}
        };
        
        var content = new StringContent(
            JsonConvert.SerializeObject(payload),
            Encoding.UTF8,
            "application/json"
        );
        
        var response = await _client.PostAsync(_url, content);
        var responseBody = await response.Content.ReadAsStringAsync();
        dynamic result = JsonConvert.DeserializeObject(responseBody);
        
        if (result.error != null)
        {
            throw new Exception($"RPC Error: {result.error}");
        }
        
        return result.result.ToObject<T>();
    }

    public async Task<bool> PingAsync()
    {
        var result = await CallAsync<dynamic>("ping");
        return result.success == true;
    }

    public async Task<string[]> GetDeviceListAsync()
    {
        var result = await CallAsync<dynamic>("get_device_list");
        return result.devices.ToObject<string[]>();
    }

    public async Task<dynamic> ReadSignalGroupAsync(string deviceId, string groupName)
    {
        var result = await CallAsync<dynamic>("read_signal_group", 
            new { device_id = deviceId, group_name = groupName });
        return result.data;
    }

    public async Task WriteSignalGroupAsync(string deviceId, string groupName, object data)
    {
        await CallAsync<dynamic>("write_signal_group",
            new { device_id = deviceId, group_name = groupName, data = data });
    }
}

// 使用示例
class Program
{
    static async Task Main(string[] args)
    {
        var client = new RpcClient();
        
        // Ping
        Console.WriteLine("Ping: " + await client.PingAsync());
        
        // Read motor control
        var motorData = await client.ReadSignalGroupAsync("mock-device", "motor_control");
        Console.WriteLine("Motor Speed: " + motorData.motor_speed);
        
        // Write motor control
        await client.WriteSignalGroupAsync("mock-device", "motor_control", new
        {
            motor_speed = 3000,
            motor_status = 1,
            motor_direction = true,
            error_code = 0,
            fault_flag = false
        });
        
        Console.WriteLine("Write completed");
    }
}
```

---

## 常见问题

### Q1: Mock Modbus 服务器无法启动

**问题**: 端口已被占用

**解决方法**:
```bash
# 检查端口占用
lsof -i :5555

# 或使用其他端口
# 在 MockModbusConfig 中修改 port 参数（或设置为 0 自动分配）
```

### Q2: 中间件无法连接到 Mock Modbus

**问题**: `Connection refused` 或连接超时

**解决方法**:
1. 确认 Mock Modbus 服务器正在运行
2. 检查配置文件中的地址和端口
3. 检查防火墙设置
4. 查看中间件日志:

```bash
tail -f /tmp/roboplc-middleware-test.log
```

### Q3: JSON-RPC 请求超时

**问题**: 请求 30 秒后超时

**解决方法**:
1. 检查设备是否连接: `curl http://localhost:8081/api/devices`
2. 检查 Mock Modbus 服务器日志
3. 检查消息路由是否正常

### Q4: 读取的值与预期不符

**问题**: F32 值显示不正确

**解决方法**:
1. 检查 `byte_order` 配置
2. 检查 `addressing_mode` 配置 (zero_based vs one_based)
3. 验证 Mock Modbus 服务器的数据格式
4. 检查信号组字段配置

### Q5: 如何模拟设备故障

**方法 1**: 停止 Mock Modbus 服务器

```bash
# 如果使用示例程序，按 Ctrl+C 停止
# 如果使用自定义服务器，停止对应的进程
# 观察中间件重连日志
```

**方法 2**: 修改 Mock Modbus 配置添加延迟

在创建自定义 Mock 服务器时，可以设置响应延迟：

```rust
let config = MockModbusConfig {
    port: 5555,
    unit_id: 1,
    response_delay_ms: 5000,  // 添加 5 秒延迟
    accept_connections: true,
    drop_after_requests: None,
};
```

**方法 3**: 模拟连接断开

设置在 N 个请求后断开连接：

```rust
let config = MockModbusConfig {
    port: 5555,
    unit_id: 1,
    response_delay_ms: 0,
    accept_connections: true,
    drop_after_requests: Some(5),  // 在 5 个请求后断开
};
```

### Q6: 如何测试并发请求

**Python 示例**:

```python
import asyncio
from rpc_client import RpcClient

async def concurrent_reads(client, count=10):
    tasks = []
    for i in range(count):
        task = asyncio.create_task(
            client.read_signal_group("mock-device", "motor_control")
        )
        tasks.append(task)
    
    results = await asyncio.gather(*tasks)
    print(f"Completed {len(results)} requests")

async def main():
    client = RpcClient()
    await concurrent_reads(client, 10)

asyncio.run(main())
```

---

## 进阶测试

### 1. 测试信号组编码/解码

修改 Mock Modbus 服务器的寄存器值,验证中间件正确解析:

```rust
// 在自定义 Mock 服务器中
mock_server.set_holding_register(100, 4660);  // 0x1234
```

通过 JSON-RPC 读取:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "read_signal_group",
    "p": {"device_id": "mock-device", "group_name": "motor_control"}
  }'
```

验证 `motor_speed` 是否为 4660。

### 2. 测试 F32 数据类型

写入一个 F32 值:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "write_signal_group",
    "p": {
      "device_id": "mock-device",
      "group_name": "temperature_sensor",
      "data": {
        "temperature_1": 37.5
      }
    }
  }'
```

验证写入结果:

```bash
curl -X POST http://localhost:8080/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "m": "read_signal_group",
    "p": {"device_id": "mock-device", "group_name": "temperature_sensor"}
  }'
```

检查返回的 `temperature_1` 是否为 37.5。

### 3. 测试连接重连

1. 启动中间件
2. 等待连接成功
3. 停止 Mock Modbus 服务器（Ctrl+C）
4. 观察中间件重连日志
5. 重启 Mock Modbus 服务器
6. 验证自动重连成功

---

## 总结

通过本指南,你应该能够:

1. ✅ 启动 Mock Modbus 服务器
2. ✅ 配置并启动中间件
3. ✅ 通过 JSON-RPC API 读取和写入寄存器
4. ✅ 通过 HTTP API 监控设备状态
5. ✅ 使用 Python/C# 等语言集成上位机
6. ✅ 测试并发请求和故障恢复

如有问题,请查看日志或参考其他文档:
- [架构概览](../architecture.md)
- [HTTP API](../http-api.md)
- [配置管理](../configuration.md)
- [测试指南](./测试指南.md)
