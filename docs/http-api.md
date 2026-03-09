# HTTP API 模块

> HTTP API 模块提供 RESTful 接口用于设备管理和监控。

## 📡 端点概览

### GET 端点

| 端点 | 说明 |
|------|------|
| `GET /api/devices` | 获取所有设备列表，返回设备 ID、连接状态、错误计数等 |
| `GET /api/devices/{id}/status` | 获取单个设备详情，包括连接状态和通信时间 |
| `GET /api/health` | 系统健康检查，返回健康状态和设备连接统计 |
| `GET /api/config` | 查询当前配置信息 |

### POST 端点

| 端点 | 说明 |
|------|------|
| `POST /api/config/reload` | 配置重载（实际由文件监控触发，此端点仅返回成功） |

## 📊 API 响应格式

### 设备列表响应

**请求：** `GET /api/devices`

**响应：**
```json
{
  "devices": [
    {
      "id": "plc-1",
      "connected": true,
      "last_communication_ms": 1234,
      "error_count": 0
    }
  ]
}
```

### 单个设备状态

**请求：** `GET /api/devices/{id}/status`

**成功响应（200）：**
```json
{
  "id": "plc-1",
  "connected": true,
  "last_communication_ms": 1234,
  "error_count": 0,
  "reconnect_count": 2
}
```

**设备不存在（404）：**
```json
{
  "error": "Device not found"
}
```

### 健康检查响应

**请求：** `GET /api/health`

**响应：**
```json
{
  "status": "healthy",
  "devices": {
    "total": 3,
    "connected": 3,
    "disconnected": 0
  }
}
```

**健康状态说明：**

| 状态 | 条件 |
|------|------|
| `healthy` | 所有设备都已连接 |
| `degraded` | 部分设备断开连接 |
| `unhealthy` | 所有设备断开连接或没有配置设备 |

### 配置查询响应

**请求：** `GET /api/config`

**响应：**
```json
{
  "config": {
    "server": {
      "rpc_port": 8080,
      "http_port": 8081
    },
    "logging": {
      "level": "info",
      "file": "/var/log/roboplc-middleware.log",
      "daily_rotation": true
    },
    "devices": [...]
  }
}
```

### 配置重载响应

**请求：** `POST /api/config/reload`

**响应：**
```json
{
  "reload": "ok"
}
```

> **注意：** 实际的配置重载由 ConfigLoader 的文件监控机制触发。修改 `config.toml` 文件后会自动重新加载配置。

## 🔧 架构说明

### 无状态设计

HttpWorker 不维护任何内部状态，所有数据从共享状态（Variables）读取：
- `device_states`: 设备连接状态和统计信息
- `config`: 当前配置

### 技术实现

- 使用 **actix-web** 框架
- 在 blocking worker 中spawn tokio runtime
- 通过 `AppState` 共享设备状态
- 使用 `parking_lot_rt::RwLock` 实现并发安全读取

### 共享状态结构

```rust
pub struct AppState {
    pub device_states: Arc<RwLock<HashMap<String, DeviceStatus>>>,
    pub config: Arc<Config>,
}
```

## 📝 相关文档

- [Worker 模块](workers/worker模块.md) - HttpWorker 详细说明
- [架构概览](architecture.md) - 整体架构设计
- [配置管理](configuration.md) - 配置热更新机制
