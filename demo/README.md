# Demo - Mock Server 和 JSON-RPC 客户端

本目录包含用于测试和演示 roboplc-middleware 的完整示例代码。

## 📁 文件结构

```
demo/
├── config-mock.toml      # Mock 配置文件
├── mock_server.rs        # Mock Modbus TCP 服务器
├── jsonrpc_client.rs     # JSON-RPC 客户端示例
└── README.md             # 本文档
```

## 🚀 快速开始

**重要提示**: 必须按照以下顺序启动服务，每个服务在单独的终端窗口中运行。

### 步骤 1: 设置配置文件

```bash
# 复制 mock 配置文件到项目根目录
cp demo/config-mock.toml config.toml
```

### 步骤 2: 启动 Mock Modbus Server (终端 1)

在第一个终端窗口中运行：

```bash
# 使用默认端口 5555
cargo run --bin mock_server

# 或使用自定义端口
ROBOPLC_MOCK_PORT=5555 cargo run --bin mock_server
```

你会看到：
```
🚀 启动 Mock Modbus Server...
✅ Mock Modbus Server 启动在端口：5555
   地址：127.0.0.1:5555
   Demo 数据已初始化:
   - 电机速度：1500 RPM, 状态：运行中，方向：正转
   - 温度 1: 25.5°C, 温度 2: 30.2°C, 湿度：65%

按 Ctrl+C 停止服务器
```

**保持此终端窗口运行，不要关闭**

### 步骤 3: 启动 roboplc-middleware (终端 2)

在第二个终端窗口中运行：

```bash
# 确保 config.toml 已复制 (步骤 1)
# 启动中间件（使用模拟模式跳过实时调度）
ROBOPLC_SIMULATED=1 cargo run --bin roboplc-middleware
```

等待看到类似输出：
```
RPC Server started on 0.0.0.0:8080
...
```

**保持此终端窗口运行，不要关闭**

### 步骤 4: 使用 JSON-RPC 客户端与中间件通信 (终端 3)

在第三个终端窗口中运行：

```bash
# 读取电机控制信号组
cargo run --bin jsonrpc_client -- read motor_control

# 读取温度传感器信号组
cargo run --bin jsonrpc_client -- read temperature_sensor

# 写入电机速度
cargo run --bin jsonrpc_client -- write motor_control motor_speed 2000

# 列出所有设备
cargo run --bin jsonrpc_client -- list

# 获取系统状态
cargo run --bin jsonrpc_client -- status

# 交互模式
cargo run --bin jsonrpc_client -- interactive
```

## 📖 架构图

```
┌─────────────────┐      JSON-RPC      ┌──────────────────┐     Modbus TCP     ┌─────────────────┐
│  JSON-RPC       │◄──────────────────►│  roboplc-        │◄──────────────────►│  Mock Modbus    │
│  Client         │    TCP:8080        │  middleware      │   TCP:5555         │  Server         │
│  (上位机)        │   (原始 TCP)        │  (通信中间件)     │                   │  (模拟 PLC)       │
└─────────────────┘                    └──────────────────┘                    └─────────────────┘
                                              │
                                              ▼ HTTP REST API
                                        ┌──────────────┐
                                        │   curl/      │
                                        │   browser    │
                                        │   HTTP:8081  │
                                        └──────────────┘
```

### 端口说明

| 端口 | 协议 | 用途 | 访问方式 | 示例 |
|------|------|------|----------|------|
| **8080** | JSON-RPC (原始 TCP) | 设备控制接口（读/写寄存器） | `nc`, `socat` | `echo '{...}' \| nc 127.0.0.1 8080` |
| **8081** | HTTP REST API | 管理接口（查询设备状态） | `curl`, 浏览器 | `curl http://127.0.0.1:8081/api/devices` |

**重要提示**: 8080 端口是原始 TCP 协议，**不是 HTTP**。请勿使用 curl 访问 8080，请使用 `nc` 或 `soccat` 等 TCP 客户端工具。

## 🔧 Mock Server 功能

### 支持的 Modbus 功能码

| 功能码 | 名称 | 描述 |
|--------|------|------|
| 0x03 | Read Holding Registers | 读取保持寄存器 |
| 0x04 | Read Input Registers | 读取输入寄存器 |
| 0x06 | Write Single Register | 写入单个寄存器 |
| 0x10 | Write Multiple Registers | 写入多个寄存器 |
| 0x01 | Read Coils | 读取线圈 |
| 0x05 | Write Single Coil | 写入单个线圈 |

### 初始化数据

Mock Server 启动时会自动初始化以下数据：

**电机控制寄存器 (h100-h104)**
| 地址 | 字段名 | 数据类型 | 初始值 | 描述 |
|------|--------|----------|--------|------|
| h100 | motor_speed | U16 | 1500 | 电机速度 (RPM) |
| h101 | motor_status | U16 | 1 | 电机状态 (0=停止，1=运行) |
| h102 | motor_direction | Bool | 1 | 电机方向 (0=反转，1=正转) |
| h103 | error_code | U16 | 0 | 错误代码 |
| h104 | fault_flag | Bool | 0 | 故障标志 |

**温度传感器寄存器 (h200-h209)**
| 地址 | 字段名 | 数据类型 | 初始值 | 描述 |
|------|--------|----------|--------|------|
| h200-h201 | temperature_1 | F32 | 25.5 | 温度 1 (°C) |
| h202-h203 | temperature_2 | F32 | 30.2 | 温度 2 (°C) |
| h204 | humidity | U16 | 65 | 湿度 (%) |
| h205 | sensor_status | U16 | 1 | 传感器状态 |
| h206 | alarm_code | U16 | 0 | 报警代码 |

## 📡 JSON-RPC 客户端

### 命令行用法

```bash
# 显示帮助
cargo run --bin jsonrpc_client

# 读取信号组
cargo run --bin jsonrpc_client -- read <signal_group>

# 写入信号字段
cargo run --bin jsonrpc_client -- write <signal_group> <field> <value>
```

### 使用 nc 直接测试（推荐）

```bash
# Ping 测试
echo '{"jsonrpc":"2.0","method":"ping","params":{},"id":1}' | nc 127.0.0.1 8080

# 获取版本
echo '{"jsonrpc":"2.0","method":"get_version","params":{},"id":2}' | nc 127.0.0.1 8080

# 读取信号组
echo '{"jsonrpc":"2.0","method":"read_signal_group","params":{"device_id":"mock-device","group_name":"motor_control"},"id":3}' | nc 127.0.0.1 8080
```

### JSON-RPC 请求格式

**注意**: roboplc-rpc 使用标准 JSON-RPC 2.0 格式：
- `method` - 方法名
- `params` - 参数
- `id` - 请求标识
- `result` - 响应结果
- `error` - 错误信息

中间件支持以下 JSON-RPC 方法：

#### 1. ping - 连接测试

```json
{"jsonrpc":"2.0","method":"ping","params":{},"id":1}
```

响应:
```json
{"id":1,"result":{"success":true}}
```

#### 2. get_version - 获取版本

```json
{"jsonrpc":"2.0","method":"get_version","params":{},"id":2}
```

响应:
```json
{"id":2,"result":{"version":"0.1.0"}}
```

#### 3. read_signal_group - 读取信号组

```json
{
  "jsonrpc": "2.0",
  "m": "read_signal_group",
  "p": {
    "device_id": "mock-device",
    "group_name": "motor_control"
  },
  "id": 3
}
```

**注意**: 参数名必须是 `device_id` 和 `group_name`（不是 `signal_group`）

#### 4. write_signal_group - 写入信号组

```json
{
  "jsonrpc": "2.0",
  "m": "write_signal_group",
  "p": {
    "device_id": "mock-device",
    "group_name": "motor_control",
    "data": {
      "motor_speed": 2000
    }
  },
  "id": 4
}
```

#### 5. get_status - 获取设备状态

```json
{
  "jsonrpc": "2.0",
  "m": "get_status",
  "p": {
    "device_id": "mock-device"
  },
  "id": 5
}
```

### 使用 curl 测试 HTTP API (8081)

HTTP API 使用 8081 端口，支持 curl 访问：

```bash
# 获取设备列表
curl http://127.0.0.1:8081/api/devices

# 获取指定设备状态
curl http://127.0.0.1:8081/api/devices/mock-device/status

# 健康检查
curl http://127.0.0.1:8081/api/health

# 获取配置
curl http://127.0.0.1:8081/api/config

# 触发配置重载
curl -X POST http://127.0.0.1:8081/api/config/reload
```

**注意**: HTTP API (8081) 仅支持管理操作，**不支持**直接读写 Modbus 寄存器。如需读写寄存器，请使用 8080 端口的 JSON-RPC 接口。

## 🔍 调试和日志

### 查看详细日志

```bash
# 设置日志级别
RUST_LOG=debug cargo run
```

### 查看 Mock Server 日志

Mock Server 会在控制台显示：
- 📡 新连接
- 📝 写入寄存器操作

### 查看中间件日志

中间件日志会输出到：
- 控制台
- 日志文件（配置文件中的 `logging.file` 指定）

## ⚠️ 常见问题

### 使用 curl 访问 8080 无输出

**问题**: 使用 `curl http://127.0.0.1:8080` 没有响应。

**原因**: 8080 端口是**原始 TCP 协议**（非 HTTP），curl 无法直接使用。

**解决**: 
- 设备控制（读/写寄存器）→ 使用 `nc` 访问 8080:
  ```bash
  echo '{"jsonrpc":"2.0","method":"readsignalgroup","params":{"device_id":"mock-device","group_name":"motor_control"},"id":1}' | nc 127.0.0.1 8080
  ```
- 管理查询（设备列表、状态）→ 使用 `curl` 访问 8081:
  ```bash
  curl http://127.0.0.1:8081/api/devices
  ```

### 端口已被占用

如果看到 "Address already in use" 错误：

```bash
# 查找占用端口的进程
lsof -i :5555
lsof -i :8080

# 杀死进程
kill -9 <PID>
```

### 连接被拒绝

确保服务启动顺序：
1. 先启动 Mock Server
2. 再启动 Middleware
3. 最后使用 Client 测试

### 配置文件错误

确保 `config.toml` 在正确的目录：
```bash
# 复制配置文件
cp demo/config-mock.toml config.toml

# 验证配置
cat config.toml
```

## 🧪 完整测试流程

1. **准备环境**
   ```bash
   # 复制配置文件
   cp demo/config-mock.toml config.toml
   ```

2. **终端 1: 启动 Mock Server**
   ```bash
   cargo run --bin mock_server
   ```

3. **终端 2: 启动 Middleware**
   ```bash
   ROBOPLC_SIMULATED=1 cargo run
   ```

4. **终端 3: 测试通信**
   ```bash
   # 读取数据
   cargo run --bin jsonrpc_client -- read motor_control
   
   # 写入数据
   cargo run --bin jsonrpc_client -- write motor_control motor_speed 2500
   
   # 再次读取验证
   cargo run --bin jsonrpc_client -- read motor_control
   ```

## 📚 扩展开发

### 添加新的信号组

编辑 `config-mock.toml`：

```toml
[[devices.signal_groups]]
name = "new_group"
description = "新信号组"
register_address = "h300"
register_count = 5

[[devices.signal_groups.fields]]
name = "field1"
data_type = "U16"
offset = 0
```

### 添加新的 JSON-RPC 方法

编辑 `src/workers/rpc_worker.rs`，添加新的方法处理逻辑。

### 自定义 Mock Server 行为

编辑 `demo/mock_server.rs`，可以：
- 添加自定义寄存器初始化
- 模拟异常情况
- 添加响应延迟