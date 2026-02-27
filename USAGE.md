# roboplc-middleware 使用指南

## 目录

- [快速开始](#快速开始)
- [配置说明](#配置说明)
- [API 接口](#api-接口)
- [运行模式](#运行模式)
- [监控和日志](#监控和日志)
- [故障排查](#故障排查)

## 快速开始

### 1. 环境准备

确保已安装 Rust 工具链：

```bash
rustc --version
cargo --version
```

### 2. 克隆和构建

```bash
# 克隆仓库
git clone <repository-url>
cd roboplc-middleware

# 构建项目
cargo build --release
```

### 3. 创建配置文件

复制示例配置文件并修改：

```bash
cp config.sample.toml config.toml
```

编辑 `config.toml`：

```toml
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[[devices]]
id = "plc-1"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
heartbeat_interval_sec = 5
max_concurrent_ops = 3

[[devices.register_mappings]]
signal_name = "temperature"
address = "h100"
data_type = "U16"
```

### 4. 运行

**开发模式（无需 root 权限）：**

```bash
ROBOPLC_SIMULATED=1 cargo run
```

**生产模式（需要 root 权限，使用实时调度）：**

```bash
sudo cargo run --release
```

## 配置说明

### 服务器配置

| 字段 | 类型 | 说明 | 默认值 |
|------|------|------|---------|
| `server.rpc_port` | u16 | JSON-RPC 服务端口 | 8080 |
| `server.http_port` | u16 | HTTP API 端口 | 8081 |

### 日志配置

| 字段 | 类型 | 说明 | 默认值 |
|------|------|------|---------|
| `logging.level` | String | 日志级别：trace/debug/info/warn/error | info |
| `logging.file` | String | 日志文件路径 | - |
| `logging.daily_rotation` | bool | 是否按天轮转日志 | true |

### 设备配置

#### 基本字段

| 字段 | 类型 | 说明 | 默认值 |
|------|------|------|---------|
| `id` | String | 设备唯一标识符（必需） | - |
| `type` | String | 设备类型：plc/robot_arm | plc |
| `address` | String | Modbus TCP 地址（IP 或主机名） | - |
| `port` | u16 | Modbus TCP 端口 | 502 |
| `unit_id` | u8 | Modbus 单元 ID（从站地址） | 1 |
| `addressing_mode` | String | 地址模式：zero_based/one_based | zero_based |
| `byte_order` | String | 字节序：big_endian/little_endian等 | big_endian |
| `tcp_nodelay` | bool | 禁用 Nagle 算法 | true |
| `max_concurrent_ops` | u8 | 最大并发操作数 | 3 |
| `heartbeat_interval_sec` | u32 | 心跳间隔（秒） | 30 |

#### 寄存器映射

寄存器映射将 Modbus 地址映射为有意义的信号名称。

**地址格式：**

| 前缀 | 寄存器类型 | 说明 |
|------|-----------|------|
| `c` | Coil (0x) | 线圈，读写布尔值 |
| `d` | Discrete Input (1x) | 离散输入，只读布尔值 |
| `i` | Input Register (3x) | 输入寄存器，只读数值 |
| `h` | Holding Register (4x) | 保持寄存器，读写数值 |

**示例：**

```toml
# 读取保持寄存器 100
[[devices.register_mappings]]
signal_name = "temperature"
address = "h100"
data_type = "U16"

# 写入线圈 5
[[devices.register_mappings]]
signal_name = "pump_status"
address = "c5"
data_type = "Bool"
access = "rw"
```

#### 数据类型

| 类型 | 说明 | 字节数 |
|------|------|--------|
| `U16` | 无符号 16 位整数 | 2 |
| `U32` | 无符号 32 位整数 | 4 |
| `I16` | 有符号 16 位整数 | 2 |
| `I32` | 有符号 32 位整数 | 4 |
| `F32` | 32 位浮点数 (IEEE 754) | 4 |
| `Bool` | 布尔值 | 1 |

#### 访问模式

| 模式 | 说明 |
|------|------|
| `rw` | 可读写（默认） |
| `read` | 只读 |
| `write` | 只写 |

### 配置验证

中间件会在启动时验证配置：

- ✅ 设备 ID 必须唯一
- ✅ 寄存器地址格式必须正确（前缀 + 数字）
- ✅ 地址必须在有效范围内（0-65535）
- ❌ 验证失败会阻止启动，检查错误日志

## API 接口

### JSON-RPC 2.0 API (端口 8080)

JSON-RPC 提供了标准化的设备控制接口。

#### 请求格式

```json
{
  "jsonrpc": "2.0",
  "method": "methodName",
  "params": { ... },
  "id": 1
}
```

#### 响应格式

```json
{
  "jsonrpc": "2.0",
  "result": { ... },
  "id": 1
}
```

#### 可用方法

##### 1. Ping

检查服务是否在线。

```bash
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "ping",
    "params": {},
    "id": 1
  }'
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": { "success": true },
  "id": 1
}
```

##### 2. GetVersion

获取中间件版本信息。

```json
{
  "jsonrpc": "2.0",
  "method": "get_version",
  "params": {},
  "id": 1
}
```

##### 3. GetDeviceList

获取所有配置的设备列表。

```json
{
  "jsonrpc": "2.0",
  "method": "get_device_list",
  "params": {},
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "devices": ["plc-1", "robot-arm-2"]
  },
  "id": 1
}
```

##### 4. GetStatus

获取设备状态。

```json
{
  "jsonrpc": "2.0",
  "method": "get_status",
  "params": {
    "device_id": "plc-1"
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "connected": true,
    "last_communication_ms": 1709025600000,
    "error_count": 0,
    "reconnect_count": 2
  },
  "id": 1
}
```

##### 5. SetRegister

写入单个寄存器。

```json
{
  "jsonrpc": "2.0",
  "method": "set_register",
  "params": {
    "device_id": "plc-1",
    "address": "h100",
    "value": 1234
  },
  "id": 1
}
```

##### 6. GetRegister

读取单个寄存器。

```json
{
  "jsonrpc": "2.0",
  "method": "get_register",
  "params": {
    "device_id": "plc-1",
    "address": "h100"
  },
  "id": 1
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "address": "h100",
    "value": 1234
  },
  "id": 1
}
```

##### 7. ReadBatch

批量读取多个寄存器。

```json
{
  "jsonrpc": "2.0",
  "method": "read_batch",
  "params": {
    "device_id": "plc-1",
    "addresses": ["h100", "h101", "h102"]
  },
  "id": 1
}
```

##### 8. WriteBatch

批量写入多个寄存器。

```json
{
  "jsonrpc": "2.0",
  "method": "write_batch",
  "params": {
    "device_id": "plc-1",
    "values": [
      {"address": "h100", "value": 100},
      {"address": "h101", "value": 200}
    ]
  },
  "id": 1
}
```

##### 9. MoveTo

控制机械臂移动到指定位置。

```json
{
  "jsonrpc": "2.0",
  "method": "move_to",
  "params": {
    "device_id": "robot-arm-1",
    "position": "x:100,y:200,z:50"
  },
  "id": 1
}
```

### HTTP API (端口 8081)

HTTP API 提供管理和监控接口。

#### 健康检查

```bash
curl http://localhost:8081/api/health
```

响应：

```json
{
  "status": "healthy",
  "uptime_secs": 3600,
  "devices_count": 2
}
```

#### 设备列表

```bash
curl http://localhost:8081/api/devices
```

响应：

```json
{
  "devices": [
    {
      "id": "plc-1",
      "type": "plc",
      "connected": true,
      "last_communication_ms": 1709025600000,
      "error_count": 0
    }
  ]
}
```

#### 设备详情

```bash
curl http://localhost:8081/api/devices/plc-1/status
```

#### 配置查询

```bash
curl http://localhost:8081/api/config
```

#### 重新加载配置

```bash
curl -X POST http://localhost:8081/api/config/reload
```

响应：

```json
{
  "status": "success",
  "message": "Configuration reloaded"
}
```

## 运行模式

### 开发模式

开发模式跳过实时调度要求，方便开发和测试。

```bash
ROBOPLC_SIMULATED=1 cargo run
```

**特点：**
- ✅ 无需 root 权限
- ✅ 快速启动和调试
- ⚠️ 不保证实时性
- ⚠️ 仅用于测试

### 生产模式

生产模式使用实时调度（FIFO），提供确定性性能。

```bash
sudo cargo run --release
```

**特点：**
- ✅ 实时调度保证
- ✅ 优化的性能
- ✅ 适合生产环境
- ⚠️ 需要 root 权限

### 后台运行

使用 systemd 或其他进程管理器：

**systemd 示例：**

创建 `/etc/systemd/system/roboplc-middleware.service`：

```ini
[Unit]
Description=RoboPLC Middleware
After=network.target

[Service]
Type=simple
User=roboplc
WorkingDirectory=/opt/roboplc-middleware
Environment="ROBOPLC_SIMULATED=0"
ExecStart=/opt/roboplc-middleware/target/release/roboplc-middleware
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

启用服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable roboplc-middleware
sudo systemctl start roboplc-middleware
```

## 监控和日志

### 日志位置

默认日志文件：`/var/log/roboplc-middleware.log`

日志级别在 `config.toml` 中配置：

```toml
[logging]
level = "info"  # trace, debug, info, warn, error
file = "/var/log/roboplc-middleware.log"
daily_rotation = true
```

### 查看日志

```bash
# 实时查看日志
tail -f /var/log/roboplc-middleware.log

# 搜索错误
grep ERROR /var/log/roboplc-middleware.log

# 查看特定设备日志
grep "device_id=plc-1" /var/log/roboplc-middleware.log
```

### 延迟监控

中间件监控每次设备通信的延迟，使用 3-sigma 算法检测异常：

- **正常**: 延迟在 mean ± 3σ 范围内
- **异常**: 延迟超过 mean + 3σ，标记为异常

通过 HTTP API 查询延迟数据：

```bash
curl http://localhost:8081/api/devices/plc-1/latency
```

### 设备事件

设备连接状态变化会触发事件：

- `Connected`: 设备成功连接
- `Disconnected`: 设备连接断开
- `Reconnecting`: 设备正在重连
- `Error`: 设备发生错误
- `HeartbeatMissed`: 心跳超时

查看事件流：

```bash
curl http://localhost:8081/api/events
```

## 故障排查

### 常见问题

#### 1. 无法连接设备

**症状：** 日志显示连接失败

**解决方案：**

1. 检查设备地址和端口：
   ```bash
   ping 192.168.1.100
   nc -zv 192.168.1.100 502
   ```

2. 检查防火墙规则

3. 确认设备已开机并监听 Modbus 端口

#### 2. 设备频繁断开

**症状：** 日志显示反复连接/断开

**可能原因：**
- 网络不稳定
- 设备负载过高
- 心跳间隔过短

**解决方案：**

1. 调整心跳间隔：
   ```toml
   [[devices]]
   heartbeat_interval_sec = 60  # 增加到 60 秒
   ```

2. 检查网络连接稳定性

3. 调整超时时间（代码中修改常量）

#### 3. 延迟过高

**症状：** API 返回高延迟值

**解决方案：**

1. 检查网络延迟：
   ```bash
   ping 192.168.1.100
   ```

2. 检查设备负载和性能

3. 调整 `max_concurrent_ops` 减少并发压力

#### 4. 配置文件错误

**症状：** 启动失败，显示配置错误

**解决方案：**

1. 验证 TOML 语法：
   ```bash
   cargo install cargo-tomlf
   tomlf fmt config.toml
   ```

2. 检查必填字段

3. 验证设备 ID 唯一性

4. 检查寄存器地址格式

### 调试模式

启用调试日志：

```toml
[logging]
level = "debug"
```

或环境变量：

```bash
RUST_LOG=debug cargo run
```

### 性能调优

1. **减少并发操作数**：
   ```toml
   max_concurrent_ops = 1  # 降低并发
   ```

2. **调整超时时间**（代码修改）：
   ```rust
   const BASE_TIMEOUT: Duration = Duration::from_secs(2);  // 增加基础超时
   ```

3. **启用 TCP_NODELAY**（默认已启用）：
   ```toml
   tcp_nodelay = true
   ```

## 测试

运行测试套件：

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test modbus_worker::tests

# 运行测试并显示输出
cargo test -- --nocapture
```

## 贡献

欢迎提交问题和拉取请求！

## 许可证

Apache-2.0
