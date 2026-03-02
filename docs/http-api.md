# HTTP API 模块

> HTTP API 模块提供 RESTful 接口用于设备管理和监控。

## 📡 端点概览

### GET 端点
| 端点 | 方法 | 说明 |
|-------|------|----------|
| `GET /api/devices` | 获取所有设备列表 | 返回设备 ID、状态等信息 |
| `GET /api/devices/{id}` | 获取单个设备详情 | 通过设备 ID 查询 |
| `GET /api/health` | 健康检查 | 返回系统健康状态 |
| `GET /api/config` | 查询配置 | 返回当前配置信息 |

### POST 端点
| 端点 | 方法 | 说明 |
|-------|------|----------|
| `POST /api/devices/{id}/register` | 设置寄存器 | 设置 Modbus 设备的寄存器地址和值 |
| `POST /api/devices/{id}/batch` | 批量操作 | 批量读写多个寄存器 |
| `POST /api/devices/{id}/move` | 机械臂控制 | 移动机械臂到指定位置 |

## 🔧 工作流程

### 1. 设备注册请求流程
```
Client              DeviceManager         ModbusWorker
    │                      │
    ├── send DeviceControl  │  │
    │                      │  ◄──►
    │                      │  │
    │                      ▼         │
    │                      │  │  ◄──►
    ├─► receive DeviceControl  │◄───┤
    │                      │  │
    │                      │         │
    │  route to ModbusWorker │
    ├─► execute Modbus operation│
    │  │  ◄───┤
    │  │         │  │
    │  │         │
    │  │         │
    │  │         ▼         │
    │  │  │         │
    │  │         │         │
    └─► send DeviceResponse   │
    │                      │
    ◄────────────────────────────────────────────────────►
    └─────────────────────────────────────────────────────►
```

### 2. 批量操作流程
```
Client              DeviceManager         ModbusWorker
    │                      │
    ├── send batch request   │  │
    │                      │  ◄──►
    │                      │  │
    │                      ▼         │
    │                      │  │  ◄──►
    ├─► route to ModbusWorker│
    │                      │
    │                      │  │  │  │
    │  │         │  │
    │  │         │  │
    │  │  │  │  │
    │  │  │  │  │
    │  │         │  │
    └─► execute operations    │  │
    │  │  ◄───┤
    │  │  │  │  │
    │  │         │  │
    │  │         │
    │  │         ▼         │
    │  │         │  │  │
    │  │         │  │
    │  │  │  │  │
    └─► return responses      │
    │                      │
    ◄────────────────────────────────────────────────────►
    └─────────────────────────────────────────────────────►
```

### 3. 消息传递机制

HTTP API → Hub → DeviceManager → ModbusWorker 消息流：
1. **correlation_id**：每个请求通过 next_correlation_id() 生成唯一 ID
2. **发送**：通过 context.hub().send() 发送 DeviceControl 消息
3. **路由**：DeviceManager 使用 worker_map 查找目标 Worker
4. **响应**：ModbusWorker 发送 DeviceResponse，DeviceManager 路由回请求者

## 📊 API 响应格式

### 成功响应
```json
{
  "status": "success",
  "data": { ... }
}
```

### 错误响应
```json
{
  "status": "error",
  "error": "错误描述"
}
```

## 📝 并发控制

### 请求排队
- OperationQueue 限制最大并发操作数量（max_in_flight）
- 队列满时拒绝新请求

### 顺序保证
- 每个操作完成后立即执行下一个
- 防止并发冲突

## 🔒 超时处理

### 连接超时
- 基础超时：1 秒
- 最大超时：30 秒
- 超时后自动重连

### 心跳机制
- 间隔：30 秒
- 超时重置连接状态

## 🧪 测试指南

### 运行系统
```bash
# 开发模式
ROBOPLC_SIMULATED=1 cargo run

# 生产模式
cargo run
```

### 测试
```bash
# 运行特定测试
cargo test test_name

# 运行所有测试
cargo test
```

## 📚 相关文档

- [消息传递机制](../messaging/消息传递机制.md) - 详细的消息流设计
- [配置管理](../configuration/配置管理.md) - 配置热更新机制
