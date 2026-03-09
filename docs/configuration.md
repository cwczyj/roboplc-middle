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

# 信号组配置：定义一个信号组及其字段映射
# [[devices.signal_groups]] 定义信号组的属性
# [[devices.signal_groups.fields]] 定义该信号组内的字段（属于上一个 [[devices.signal_groups]]）
[[devices.signal_groups]]
name = "temperature_sensor"
description = "温度传感器数据"
register_address = "h100"
register_count = 5
# fields 字段在下面定义（可省略，但强烈建议配置）

# 以下 [[devices.signal_groups.fields]] 定义属于上面的 temperature_sensor 信号组
# fields 是 [[devices.signal_groups]] 的子配置项，定义该组内的具体字段
[[devices.signal_groups.fields]]
name = "temperature"
data_type = "F32"
offset = 0

[[devices.signal_groups.fields]]
name = "humidity"
data_type = "U16"
offset = 2

[[devices.signal_groups.fields]]
name = "pressure"
data_type = "I16"
offset = 3

[[devices.signal_groups.fields]]
name = "sensor_status"
data_type = "Bool"
offset = 4
# 上面的 4 个 [[devices.signal_groups.fields]] 都属于 temperature_sensor 信号组

# 下面的 [[devices.signal_groups]] 开始定义一个新的信号组
# 注意：新的 [[devices.signal_groups]] 会重置 fields 的关联关系
# 因此下面的 [[devices.signal_groups.fields]] 属于 status 信号组，而不是 temperature_sensor

[[devices.signal_groups]]
name = "status"
description = "设备状态"
register_address = "h200"
register_count = 5

# 设备状态信号组的字段映射
# offset 表示相对于 signal_group.register_address 的偏移量
[[devices.signal_groups.fields]]
name = "running"
data_type = "Bool"
offset = 0

[[devices.signal_groups.fields]]
name = "error_code"
data_type = "U16"
offset = 1

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
| `max_concurrent_ops` | u8 | 否 | 3 | 最大并发操作数 |
| `heartbeat_interval_sec` | u32 | 否 | 30 | 心跳检测间隔（秒） |

### [[devices.signal_groups]] 信号组配置

信号组（Signal Group）用于定义一批在连续寄存器范围内的相关信号，支持批量读写操作。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | String | 是 | 信号组名称（用于 API 调用） |
| `description` | String | 否 | 信号组描述 |
| `register_address` | String | 是 | Modbus 起始地址（带前缀，如 "h100"） |
| `register_count` | u16 | 是 | 寄存器数量 |
| `fields` | Array | **建议** | 字段映射列表（若不提供则为空列表） |

**注意:** `fields` 字段虽然可以省略（解析为空列表），但为了正确解释寄存器数据的含义，**强烈建议**明确配置字段映射。如果不配置 `fields`，API 将返回原始寄存器值而无法进行数据类型转换和字段命名。

#### TOML 数组层级关系说明

在 TOML 配置中，`[[devices.signal_groups]]` 和 `[[devices.signal_groups.fields]]` 的关系如下：

```toml
# 第 1 个信号组开始
[[devices.signal_groups]]
name = "group1"
register_address = "h100"
register_count = 5

# 以下 fields 属于 group1（直到遇到下一个 [[devices.signal_groups]]）
[[devices.signal_groups.fields]]
name = "field1"
data_type = "F32"
offset = 0

[[devices.signal_groups.fields]]
name = "field2"
data_type = "U16"
offset = 2
# group1 的 fields 结束

# 第 2 个信号组开始（重置 fields 关联）
[[devices.signal_groups]]
name = "group2"
register_address = "h200"
register_count = 3

# 以下 fields 属于 group2
[[devices.signal_groups.fields]]
name = "field3"
data_type = "I16"
offset = 0
# group2 的 fields 结束
```

**关键规则：**
- 每个 `[[devices.signal_groups]]` 开始一个新的信号组
- 后续的 `[[devices.signal_groups.fields]]` 自动归属到**上一个** `[[devices.signal_groups]]`
- 遇到新的 `[[devices.signal_groups]]` 时，自动结束上一个信号组的 fields 定义
- `fields` 可以省略（不配置任何 `[[devices.signal_groups.fields]]`）

### [[devices.signal_groups.fields]] 字段映射

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | String | 是 | 字段名称 |
| `data_type` | String | 是 | 数据类型: U16/I16/U32/I32/F32/Bool |
| `offset` | u16 | 是 | 寄存器偏移量（以寄存器为单位） |

**字段映射示例：**

假设有以下配置：
```toml
register_address = "h100"
register_count = 5

[[devices.signal_groups.fields]]
name = "temperature"
data_type = "F32"
offset = 0

[[devices.signal_groups.fields]]
name = "humidity"
data_type = "U16"
offset = 2
```

**寄存器布局：**
```
Modbus 地址    偏移量    字段名        数据类型
h100           0         temperature   F32 (占 2 个寄存器)
h101           1         (temperature 继续存储)
h102           2         humidity      U16 (占 1 个寄存器)
h103           3         (未使用)
h104           4         (未使用)
```

**验证规则：**
- `temperature` (F32): offset=0, 占用 2 个寄存器 → 访问范围: offset 0-1 ✓
- `humidity` (U16): offset=2, 占用 1 个寄存器 → 访问范围: offset 2 ✓
- 两者均在 `register_count=5` 范围内 ✓

## 地址格式

Modbus 地址使用前缀表示寄存器类型：

| 前缀 | 寄存器类型 | Modbus 代码 |
|------|-----------|-------------|
| `c` | 线圈 (Coil) | 0x |
| `d` | 离散输入 (Discrete Input) | 1x |
| `i` | 输入寄存器 (Input Register) | 3x |
| `h` | 保持寄存器 (Holding Register) | 4x |

**示例:**
- `h100` = 保持寄存器地址 100
- `i50` = 输入寄存器地址 50
- `c10` = 线圈地址 10
- `d5` = 离散输入地址 5

## 数据类型

| 类型 | 说明 | 占用寄存器数 |
|------|------|-------------|
| `U16` | 无符号 16 位整数 | 1 |
| `I16` | 有符号 16 位整数 | 1 |
| `U32` | 无符号 32 位整数 | 2 |
| `I32` | 有符号 32 位整数 | 2 |
| `F32` | 32 位浮点数 (IEEE 754) | 2 |
| `Bool` | 布尔值 | 1 |

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

## 信号组验证规则

配置加载时会验证信号组的以下规则：

1. **字段名称唯一**：同一信号组内字段名称不能重复
2. **偏移量有效**：字段偏移量 + 数据类型所需寄存器数 ≤ register_count
3. **地址格式正确**：register_address 必须使用有效的前缀和数字

## 配置热重载

ConfigLoader Worker 会监控配置文件变化：
- 修改 `config.toml` 后自动重新加载
- 使用内容对比避免不必要的重载
- 发送 `ConfigUpdate` 消息通知其他 Worker
- 三菱 PLC: little_endian_byte_swap