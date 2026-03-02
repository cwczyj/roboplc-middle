# 配置指南

本文档详细说明 roboplc-middleware 的配置文件格式和各配置项。

## 配置文件位置

配置文件默认名为 `config.toml`，放置在程序运行目录下。

## 完整配置示例

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
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices.register_mappings]]
signal_name = "temperature"
address = "h100"
data_type = "U16"
access = "rw"
description = "温度传感器"

[[devices.register_mappings]]
signal_name = "pressure"
address = "h101"
data_type = "F32"
access = "r"
description = "压力传感器"

[[devices]]
id = "robot-arm-1"
type = "robot_arm"
address = "192.168.1.101"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "little_endian"
tcp_nodelay = true
max_concurrent_ops = 5
heartbeat_interval_sec = 10
```

## 配置项说明

### [server] 服务器配置

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `rpc_port` | u16 | 是 | - | JSON-RPC 服务器监听端口 |
| `http_port` | u16 | 是 | - | HTTP 管理接口监听端口 |

### [logging] 日志配置

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `level` | String | 是 | - | 日志级别: trace/debug/info/warn/error |
| `file` | String | 是 | - | 日志文件路径 |
| `daily_rotation` | bool | 是 | - | 是否按天轮转日志文件 |

### [[devices]] 设备配置

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `id` | String | 是 | - | 设备唯一标识符（全局唯一） |
| `type` | String | 否 | "plc" | 设备类型: plc / robot_arm |
| `address` | String | 是 | - | Modbus TCP 地址（IP 或主机名） |
| `port` | u16 | 是 | - | Modbus TCP 端口（通常为 502） |
| `unit_id` | u8 | 是 | - | Modbus 单元 ID（从站 ID） |
| `addressing_mode` | String | 否 | "zero_based" | 地址模式: zero_based / one_based |
| `byte_order` | String | 否 | "big_endian" | 字节序: big_endian / little_endian / little_endian_byte_swap / mid_big |
| `tcp_nodelay` | bool | 否 | true | 是否启用 TCP_NODELAY |
| `max_concurrent_ops` | usize | 否 | 3 | 最大并发操作数 |
| `heartbeat_interval_sec` | u64 | 否 | 30 | 心跳检测间隔（秒） |

### [[devices.register_mappings]] 寄存器映射

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `signal_name` | String | 是 | 信号名称（用于 JSON-RPC 调用） |
| `address` | String | 是 | Modbus 地址（格式见下文） |
| `data_type` | String | 是 | 数据类型: U16/I16/U32/I32/F32/F64 |
| `access` | String | 否 | 访问权限: r / w / rw（默认 rw） |
| `description` | String | 否 | 描述信息 |

## 地址格式

Modbus 地址使用前缀表示寄存器类型：

| 前缀 | 寄存器类型 | Modbus 地址范围 |
|------|-----------|-----------------|
| `c` | 线圈 (Coil) | 0x (00001-09999) |
| `d` | 离散输入 (Discrete Input) | 1x (10001-19999) |
| `i` | 输入寄存器 (Input Register) | 3x (30001-39999) |
| `h` | 保持寄存器 (Holding Register) | 4x (40001-49999) |

**示例:**
- `h100` = 保持寄存器地址 100
- `i50` = 输入寄存器地址 50
- `c10` = 线圈地址 10

## 地址模式说明

### zero_based（零基地址）
Modbus 协议实际地址与配置地址相同。
- 配置 `h100` → Modbus 地址 100

### one_based（一基地址）
配置地址需要减 1 才是实际 Modbus 地址。
- 配置 `h100` → Modbus 地址 99

**注意:** 不同设备厂商可能使用不同的地址模式，请参考设备文档。

## 字节序说明

| 值 | 说明 |
|---|------|
| `big_endian` | 大端序 (ABCD) |
| `little_endian` | 小端序 (DCBA) |
| `little_endian_byte_swap` | 小端字节交换 (BADC) |
| `mid_big` | 中大端 (CDAB) |

**常见设备字节序:**
- 西门子 PLC: big_endian
- 欧姆龙 PLC: little_endian
- 三菱 PLC: little_endian_byte_swap