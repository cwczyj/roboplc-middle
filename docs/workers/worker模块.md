# Worker 模块

> RoboPLC 的 Worker 模块采用分布式架构，每个 Worker 负责特定的任务类型。

## 🏗️ Worker 类型

| Worker | 职责 | 说明 |
|--------|------|----------|
| **RpcWorker** | JSON-RPC 服务 | 处理来自客户端的 JSON-RPC 请求，通过 Hub 与其他 Worker 通信 |
| **DeviceManager** | 设备管理 | 接收设备控制消息，路由到对应的 ModbusWorker，管理待处理请求 |
| **ModbusWorker** | 设备控制 | 管理 Modbus TCP 连接，执行 Modbus 操作，发送心跳 |
| **HttpWorker** | HTTP 服务 | 提供 REST API 接口用于设备管理和监控 |
| **ConfigUpdater** | 配置管理 | 监听配置变化，动态更新系统配置和设备列表 |

## 🔄 Worker 通信流程

```
客户端
   │
   ├── RPC Worker (JSON-RPC)
   │   │
   └── HttpWorker (REST API)
       │
       └──> Hub
       │           │
       │           ├── DeviceManager (路由)
       │           ├── ModbusWorker (xN)
       │           ├── HttpWorker
       │           └── ConfigUpdater
       │
       └──> Hub
```

## 📋 详细说明

### RpcWorker

**职责：**
- 监听 JSON-RPC 端点（默认端口 8543）
- 解析 JSON-RPC 请求
- 转换为内部 Message 类型
- 通过 Hub 发送设备控制消息
- 处理响应并通过 response_tx 发回客户端

**关键结构：**

#### RpcHandler
处理单个 JSON-RPC 请求和响应的回调结构。

#### DeviceControlRequest
代表设备控制请求，包含：
- `device_id`：目标设备 ID
- `operation`：操作类型
- `params`：参数（JSON 值）
- `correlation_id`：请求 ID（用于响应匹配）

#### DeviceResponseData
通过通道发送回请求者的响应数据：
- `success`：操作是否成功
- `data`：响应数据（JSON 值）
- `error`：错误信息

**流程：**
```
客户端 JSON-RPC 请求
  │
  ▼
  RpcHandler 接收请求
  │
  ├── 生成 correlation_id
  │  │
  ├── 创建 DeviceControl 消息
  │  │  │
  └── 通过 response_tx 发送到主循环
      │
      ▼
      主循环：通过 response_rx 接收响应
      │
      │  ├── 匹配 correlation_id
      │  │  ├── 查找 pending_requests 中的通道
      │  │  │  ├── 发送响应数据
      │  │  │  └── 清理 pending_requests
```

**关键点：**
- **通道机制**：使用 mpsc::channel 在 RpcHandler 和主循环间传递消息
- **异步处理**：主循环非阻塞等待响应
- **超时处理**：使用 recv_timeout() 设置响应超时时间

### DeviceManager

**职责：**
- 作为 Hub 消息路由器
- 维护 worker_map（设备 ID → Worker 名称映射）
- 接收 DeviceControl 和 DeviceResponse 消息
- 管理 pending_requests（待处理请求）
- 接收 ConfigUpdate 并更新配置

**关键结构：**

#### worker_map
```rust
pub struct DeviceManager {
    config: Config,
    worker_map: HashMap<String, String>,  // 设备 ID → Worker 名称
    pending_requests: HashMap<u64, Sender<DeviceResponseData>>,  // 待处理请求
}
```

#### 消息处理流程

**DeviceControl 处理：**
```
接收 DeviceControl
  │
  ├── 验证 device_id 是否在 worker_map 中
  │   │
  ├── 找到目标 Worker 名称
  │   │  │
  ├── 通过 Hub 转发 DeviceControl
  │   │
  └── 等待 ModbusWorker 的 DeviceResponse
```

**DeviceResponse 处理：**
```
接收 DeviceResponse
  │
  ├── 从 pending_requests 中查找对应的 response_tx
  │   │
  ├── 通过 correlation_id 匹配原始请求
  │   │  │
  ├── 发送响应数据
  │   │  │
  └── 清理 pending_requests
  │
  │
  ├── 如果未找到：记录警告
  │   └── 如果发送失败：记录错误
```

**关键点：**
- **消息路由**：使用 worker_map 查找目标 Worker，不是硬编码设备名称
- **请求跟踪**：pending_requests 跟踪所有未完成的请求
- **响应匹配**：correlation_id 确保响应与请求正确对应

### ModbusWorker

**职责：**
- 管理 Modbus TCP 连接
- 执行 Modbus 操作（读/写寄存器）
- 维护操作队列（并发控制）
- 发送心跳保持连接活跃
- 监控设备延迟和状态

**关键结构：**

#### ConnectionState
连接状态枚举：
```rust
pub enum ConnectionState {
    Disconnected,  // 断开
    Connecting,    // 连接中
    Connected,     // 已连接
}
```

#### Backoff
指数退避策略实现：
```rust
struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

fn next_delay(&mut self) -> Duration {
    let jitter = (self.next_delay_ms / 10) * (self.attempts as u64 % 3);
    let delay = self.next_delay_ms + jitter;
    Duration::from_millis(delay)
}
```

**配置：**
- `BASE_TIMEOUT`：1 秒
- `MAX_TIMEOUT`：30 秒
- `BACKOFF_BASE_MS`：100 毫秒
- `BACKOFF_MAX_MS`：30 秒

#### OperationQueue
操作队列，支持并发控制和请求跟踪：
```rust
struct OperationQueue<T> {
    pending: VecDeque<T>,
    in_flight: usize,
    max_in_flight: usize,
}

fn can_start(&self) -> bool {
    self.in_flight < self.max_in_flight
}
```

**操作流程：**
```
接收 DeviceControl
  │
  ├── 验证是发给自己的设备
  │  │
  ├── 转换为 ModbusOp
  │  │  │
  ├── 压入操作队列
  │  │  │
  ├── 执行操作（如连接则先连接）
  │  │  │
  └── 发送 DeviceResponse
```

**关键点：**
- **指数退避**：连接失败时使用 Backoff 策略避免重连风暴
- **并发控制**：使用 OperationQueue 限制并发操作数
- **操作顺序**：FIFO 队列保证顺序
- **状态维护**：通过 DeviceEvent 更新设备状态

#### ModbusOp
操作类型枚举：
```rust
pub enum ModbusOp {
    ReadHolding { address: u16, count: u16 },
    WriteSingle { address: u16, value: u16 },
    WriteMultiple { address: u16, values: Vec<u16> },
    // 批量操作类型
}
```

#### 操作执行
```rust
fn execute_operation(&mut self, op: &ModbusOp) -> OperationResult {
    let client = match &self.connection {
        Some(c) => c.clone(),
        None => return error,
    };

    match op {
        ModbusOp::ReadHolding { address, count } => {
            // 读保持寄存器
            self.read_holding(client, address, count)
        }
        ModbusOp::WriteSingle { address, value } => {
            // 写单个寄存器
            self.write_single(client, address, value)
        }
        // ... 其他操作类型
    }
}
```

**关键点：**
- **连接管理**：ensure_connected() 确保执行操作前已连接
- **重试机制**：操作失败时自动重试（使用 Backoff）
- **延迟监控**：记录操作延迟并上报
- **错误处理**：失败时返回详细的错误信息

### HttpWorker

**职责：**
- 使用 actix-web 框架提供 HTTP API 端点
- 查询系统状态
- 提供设备管理接口
- 更新配置（POST /api/config/reload）

**实现方式：**
- 使用 `AppState` 共享设备状态
- GET 端点：读取 device_states
- POST 端点：发送 DeviceControl 到 Hub

**关键点：**
- **无状态设计**：HTTP Worker 不维护任何内部状态
- **消息传递**：通过 hub_sender 可选字段发送消息
- **响应式**：返回统一的 JSON 响应格式

### ConfigUpdater

**职责：**
- 监听 ConfigUpdate 消息（由 ConfigLoader 发送）
- 解析配置 JSON
- 更新 Variables 中的配置
- 更新 worker_map（如果设备列表变化）
- 必要时重启相关 Worker

## 🎯 优势

### 解耦合
Worker 之间通过消息总线通信，不直接依赖

### 可扩展
新增消息类型不需要修改其他 Worker

### 容错性
一个 Worker 失败不影响其他 Worker

### 可测试
每个组件都有独立的测试

## 📝 相关文档

- [架构概览](../architecture/架构概览.md)
- [消息传递机制](../messaging/消息传递机制.md)
- [配置管理](../configuration/配置管理.md)
- [HTTP API](../http-api/http-api.md)
