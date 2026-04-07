# JSON-RPC over TCP 接口示例

本文档展示通过 TCP 连接 roboplc-middleware 时发送和接收的 JSON 字符串格式。

**连接信息：**
- 协议：JSON-RPC 2.0 over TCP
- 默认地址：`127.0.0.1:8080`

**重要说明：** 本中间件使用缩写字段名以节省带宽：
- `m` = method（方法名）
- `p` = params（参数对象）
- `i` = id（请求 ID）

---

## 1. Ping - 健康检查

### 发送
```json
{ "jsonrpc": "2.0", "m": "ping", "p": {}, "i": 1 }
```

### 接收
```json
{ "jsonrpc": "2.0", "i": 1, "result": { "success": true } }
```

---

## 2. GetVersion - 获取版本

### 发送
```json
{ "jsonrpc": "2.0", "m": "get_version", "p": {}, "i": 2 }
```

### 接收
```json
{ "jsonrpc": "2.0", "i": 2, "result": { "version": "0.1.0" } }
```

---

## 3. GetDeviceList - 获取设备列表

### 发送
```json
{ "jsonrpc": "2.0", "m": "get_device_list", "p": {}, "i": 3 }
```

### 接收
```json
{ "jsonrpc": "2.0", "i": 3, "result": { "devices": ["plc1", "robot_arm_1"] } }
```

---

## 4. GetStatus - 获取设备状态

### 发送
```json
{ "jsonrpc": "2.0", "m": "get_status", "p": { "device_id": "plc1" }, "i": 4 }
```

### 接收（设备在线）
```json
{ "jsonrpc": "2.0", "i": 4, "result": { "connected": true, "last_communication_ms": 1234, "error_count": 0 } }
```

### 接收（设备离线）
```json
{ "jsonrpc": "2.0", "i": 4, "result": { "connected": false, "last_communication_ms": 0, "error_count": 3 } }
```

---

## 5. ReadSignalGroup - 读取信号组

### 发送（示例：读取机械臂实时位置）
```json
{"jsonrpc":"2.0","m":"read_signal_group","p":{"device_id":"Test-Dobot","group_name":"real_time_position_and_euler"},"i":1}
```

### 接收（成功）
```json
{"jsonrpc":"2.0","i":1,"result":{"success":true,"data":{"x_value":353.0445,"y_value":174.1174,"z_value":335.9194,"rx":-177.8,"ry":-5.13,"rz":-45.8}}}
```

### 发送（示例：读取电机控制）
```json
{ "jsonrpc": "2.0", "m": "read_signal_group", "p": { "device_id": "plc1", "group_name": "motor_control" }, "i": 5 }
```

### 接收（成功）
```json
{ "jsonrpc": "2.0", "i": 5, "result": { "success": true, "data": { "motor_speed": 1500, "motor_status": 1, "motor_direction": true, "error_code": 0, "fault_flag": false } } }
```

### 接收（失败 - 设备未连接）
```json
{ "jsonrpc": "2.0", "i": 5, "result": { "error": "Device plc1 is not connected" } }
```

---

## 6. WriteSignalGroup - 写入信号组

### 发送（示例：机械臂位置控制）
```json
{ "jsonrpc": "2.0", "m": "write_signal_group", "p": { "device_id": "Test-Dobot", "group_name": "position_and_euler", "data": { "x_value": 353.0445, "y_value": 174.1174, "z_value": 335.9194, "rx": -177.8, "ry": -5.13, "rz": -45.8 } }, "i": 1 }
```

### 发送（示例：电机速度控制）
```json
{ "jsonrpc": "2.0", "m": "write_signal_group", "p": { "device_id": "plc1", "group_name": "motor_control", "data": { "motor_speed": 2000 } }, "i": 6 }
```

### 接收（成功）
```json
{ "jsonrpc": "2.0", "i": 6, "result": { "success": true } }
```

### 接收（失败 - 字段不存在）
```json
{ "jsonrpc": "2.0", "i": 6, "result": { "error": "Field 'motor_speed' not found in signal group 'motor_control'" } }
```

---

## 7. 错误响应格式

### 设备不存在
```json
{ "jsonrpc": "2.0", "i": 7, "error": { "code": -32001, "message": "Device 'unknown_device' not found" } }
```

### 信号组不存在
```json
{ "jsonrpc": "2.0", "i": 8, "error": { "code": -32002, "message": "Signal group 'invalid_group' not found in device 'plc1'" } }
```

### 无效 JSON 解析错误
```json
{ "jsonrpc": "2.0", "i": null, "error": { "code": -32700, "message": "Parse error" } }
```

### 无效方法
```json
{ "jsonrpc": "2.0", "i": 9, "error": { "code": -32601, "message": "Method not found" } }
```

### 请求超时
```json
{ "jsonrpc": "2.0", "i": 10, "result": { "error": "Request timed out" } }
```

---

## JSON 字段说明

### 请求字段
| 字段 | 说明 | 示例值 |
|------|------|--------|
| `jsonrpc` | 固定为 "2.0" | `"2.0"` |
| `m` | 方法名 | `"ping"`, `"get_version"`, `"get_device_list"`, `"get_status"`, `"read_signal_group"`, `"write_signal_group"` |
| `p` | 方法参数对象 | 见下方参数说明 |
| `i` | 请求 ID（整数） | `1`, `2`, `3`... |

### 各方法参数 (`p`) 说明
| 方法 | 参数格式 |
|------|----------|
| `ping` | `{}` |
| `get_version` | `{}` |
| `get_device_list` | `{}` |
| `get_status` | `{ "device_id": "plc1" }` |
| `read_signal_group` | `{ "device_id": "plc1", "group_name": "motor_control" }` |
| `write_signal_group` | `{ "device_id": "plc1", "group_name": "motor_control", "data": { "motor_speed": 2000 } }` |

### 响应字段
| 字段 | 说明 |
|------|------|
| `jsonrpc` | 固定为 "2.0" |
| `i` | 响应 ID，对应请求的 `i` |
| `result` | 成功时的结果对象 |
| `error` | 失败时的错误对象（包含 `code` 和 `message`） |

### 错误码
| 错误码 | 说明 |
|--------|------|
| -32700 | 解析错误（无效 JSON） |
| -32601 | 方法不存在 |
| -32001 | 设备不存在 |
| -32002 | 信号组不存在 |
| -32003 | 字段不存在 |
| -32004 | 设备未连接 |
| -32005 | 请求超时 |

---

## 快速参考卡片

```
连接：127.0.0.1:8080 (TCP)

请求格式:
{ "jsonrpc": "2.0", "m": "<方法>", "p": { <参数> }, "i": <ID> }

方法列表:
  ping                → { }
  get_version         → { }
  get_device_list     → { }
  get_status          → { "device_id": "xxx" }
  read_signal_group   → { "device_id": "xxx", "group_name": "xxx" }
  write_signal_group  → { "device_id": "xxx", "group_name": "xxx", "data": { <字段>: <值> } }

响应格式 (成功):
{ "jsonrpc": "2.0", "i": <ID>, "result": { "success": true, "data": { ... } } }

响应格式 (失败):
{ "jsonrpc": "2.0", "i": <ID>, "result": { "error": "错误信息" } }
或
{ "jsonrpc": "2.0", "i": <ID>, "error": { "code": -32xxx, "message": "..." } }