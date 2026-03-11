# Worker 模块

> RoboPLC 的 Worker 模块采用分布式架构，每个 Worker 负责特定的任务类型。

## 🏗️ Worker 类型

| Worker | 端口 | 职责 |
|--------|------|------|
| **RpcWorker** | 8080 | JSON-RPC 2.0 服务器（异步架构），处理客户端请求 |
| **HttpWorker** | 8081 | HTTP REST API 服务器，提供设备管理接口 |
| **DeviceManager** | - | 设备注册和超时清理（不再路由 DeviceControl） |
| **ModbusWorker** | - | Modbus TCP 客户端（每设备一个），执行寄存器操作 |
| **HeartbeatWorker** | - | 独立心跳检测，跟踪设备延迟 |
| **LatencyMonitor** | - | 3-sigma 延迟异常检测 |
| **ConfigLoader** | - | 配置热重载（文件监控） |

## 🔄 Worker 通信流程

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│   JSON-RPC      │────▶│  Device Manager  │     │   Modbus    │
│   Server        │     │  (Timeout        │     │   Workers   │
│   (port 8080)   │     │   Cleanup)       │     │  (per device)│
└─────────────────┘     └──────────────────┘     └─────────────┘
        │                                               ▲
        │                                               │
        │              ┌──────────────────┐             │
        └─────────────▶│  HTTP API       │◀────────────┘
                       │  (port 8081)    │       (直接响应)
                       └──────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ ConfigLoader │     │ Heartbeat    │     │  Latency     │
│ (Hot Reload) │     │  Worker      │     │  Monitor     │
└──────────────┘     └──────────────┘     └──────────────┘
```

**架构演进说明**：DeviceManager 不再路由 DeviceControl/Response 消息，ModbusWorker 直接使用 respond_to 通道响应给 RpcWorker。

## 📋 详细说明

### RpcWorker

**职责：**
- 监听 JSON-RPC 端点（默认端口 8080）
- 使用异步架构处理并发请求
- 解析 JSON-RPC 请求并转换为内部 Message 类型
- 通过 Hub 发送设备控制消息
- 处理响应并通过 oneshot 通道发回客户端

**架构特点（异步模式）：**
- 在 blocking worker 中 spawn tokio runtime
- 使用 `tokio::net::TcpListener` 进行异步 TCP 接收
- 使用 `tokio::select!` 进行并发处理
- 使用 `tokio::sync::mpsc` 进行设备控制请求传递
- 使用 `tokio::sync::oneshot` 进行响应处理

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "rpc_server", blocking = true)]
pub struct RpcWorker {
    config: Config,
}
```

**JSON-RPC 方法：**

| 方法 | 参数 | 说明 |
|------|------|------|
| `ping` | `{}` | 健康检查 |
| `get_version` | `{}` | 获取中间件版本 |
| `get_device_list` | `{}` | 获取设备列表 |
| `get_status` | `{"device_id": "plc-1"}` | 获取设备状态 |
| `read_signal_group` | `{"device_id": "plc-1", "group_name": "sensors"}` | 读取信号组 |
| `write_signal_group` | `{"device_id": "plc-1", "group_name": "actuators", "data": {...}}` | 写入信号组 |

**请求格式：**
```json
{"m":"read_signal_group", "p":{"device_id": "plc-1", "group_name": "sensors"}}
```

**关键结构：**

```rust
// 设备控制请求
pub struct DeviceControlRequest {
    pub device_id: String,
    pub operation: Operation,
    pub params: JsonValue,
    pub correlation_id: u64,
    pub respond_to: ResponseSender,  // oneshot::Sender
}

// 待处理请求跟踪
struct PendingRequest {
    correlation_id: u64,
    created_at: Instant,
    respond_to: ResponseSender,
}

// RPC Handler
struct RpcHandler {
    device_ids: Vec<String>,
    device_control_tx: mpsc::Sender<DeviceControlRequest>,
    hub: Hub<Message>,
}
```

**关键点：**
- **双通道机制**：mpsc 用于内部传递，oneshot 用于接收响应
- **超时清理**：定期清理超时的 pending_requests
- **correlation_id**：全局原子计数器生成唯一 ID
- **错误传播**：超时时发送 TimeoutCleanup 消息

### DeviceManager

**职责：**
- 作为设备注册中心
- 接收 TimeoutCleanup 消息并清理超时请求
- 维护 worker_map（设备 ID → Worker 名称映射）
- **不再路由 DeviceControl 和 DeviceResponse 消息**

**架构演进说明：**

旧架构中 DeviceManager 会：
1. 接收 DeviceControl 消息
2. 查找目标 ModbusWorker
3. 转发 DeviceControl 消息
4. 接收 DeviceResponse 消息
5. 通过 correlation_id 路由回 RpcWorker

新架构中：
- DeviceManager **不再订阅 DeviceControl 和 DeviceResponse**
- ModbusWorker **直接使用 respond_to 通道响应给 RpcWorker**
- DeviceManager 只处理 **TimeoutCleanup** 消息

**关键结构：**

```rust
pub struct DeviceManager {
    config: Config,
    worker_map: HashMap<String, String>,  // 设备 ID → Worker 名称
    pending_requests: HashMap<u64, Sender<DeviceResponseData>>,  // 待处理请求
}
```

**消息订阅：**
```rust
// 只订阅 TimeoutCleanup 消息
event_matches!(Message::TimeoutCleanup { .. })
```

**关键点：**
- **设备注册**：启动时注册所有设备到共享状态
- **超时清理**：清理 pending_requests 中的超时请求
- **职责简化**：不再参与消息路由

### ModbusWorker

**职责：**
- 管理 Modbus TCP 连接
- 执行 Modbus 操作（读写所有寄存器类型）
- 使用直接响应机制回应 RpcWorker
- 上报延迟数据

**模块结构：**
```
modbus/
├── mod.rs       # 模块导出
├── client.rs    # ModbusClient - TCP 客户端
├── worker.rs    # ModbusWorker - RoboPLC worker 实现
├── operations.rs # 寄存器操作和地址解析
├── parsing.rs   # 信号组编码/解码
└── types.rs     # 共享类型（Backoff, ConnectionState等）
```

**支持的寄存器类型：**

| 前缀 | 类型 | Modbus 代码 |
|------|------|-------------|
| `c` | Coil | 0x |
| `d` | Discrete Input | 1x |
| `i` | Input Register | 3x |
| `h` | Holding Register | 4x |

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    device: Device,
    client: Option<ModbusClient>,
    connection_state: ConnectionState,
    last_communication: Option<SystemTime>,
    backoff: Backoff,
    timeout_handler: TimeoutHandler,
}
```

**直接响应机制：**

```rust
// 处理 DeviceControl 消息
if let Message::DeviceControl { device_id, operation, params, correlation_id, respond_to } = msg {
    // 执行操作
    let result = execute_operation(...);
    
    // 直接通过 respond_to 发送响应
    if let Some(sender) = respond_to {
        let _ = sender.send((success, data, error));
    }
}
```

**关键结构：**

```rust
// 连接状态
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

// 指数退避
struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

// 超时处理器
struct TimeoutHandler {
    base_timeout: Duration,
    max_timeout: Duration,
    current_timeout: Duration,
}
```

**连接配置：**
- `BASE_TIMEOUT`：1 秒
- `MAX_TIMEOUT`：30 秒
- `BACKOFF_BASE_MS`：100 毫秒
- `BACKOFF_MAX_MS`：30 秒

**关键点：**
- **每设备一个 Worker**：独立管理连接和操作
- **指数退避**：连接失败时避免重连风暴
- **直接响应**：使用 respond_to 通道直接响应 RpcWorker
- **信号组操作**：支持批量读写连续寄存器
- **连接探测**：每次操作前进行轻量级连接探测

---

### HeartbeatWorker

**职责：**
- 独立检测所有设备是否在线
- 通过发送 GetStatus 请求复用 ModbusWorker 连接
- 广播 DeviceHeartbeat 消息（包含真实延迟）
- 记录延迟到 latency_samples
- 更新设备状态到共享变量

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "heartbeat", blocking = true)]
pub struct HeartbeatWorker {
    config: Config,
    current_device_index: usize,
    heartbeat_interval_sec: u32,
    heartbeat_timeout_sec: u32,
}
```

**工作流程：**
```
循环检查每个设备
   │
   ├── 发送 GetStatus 请求
   │
   ├── 等待响应（带超时，默认 5 秒）
   │
   ├── 记录延迟
   │
   ├── 更新 device_states
   │
   └── 广播 DeviceHeartbeat
```

**配置参数：**
- `heartbeat_interval_sec`：心跳间隔（秒），取所有设备的最小值
- `heartbeat_timeout_sec`：响应超时（默认 5 秒）

**关键点：**
- **轮询检测**：平均分配检查时间，避免同时发送大量请求
- **状态变化事件**：连接状态变化时记录 DeviceEvent
- **延迟采样**：成功响应时记录 LatencySample

---

### LatencyMonitor

**职责：**
- 接收设备心跳消息
- 维护延迟统计窗口
- 实现 3-sigma 异常检测
- 发送延迟异常事件

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "latency_monitor")]
pub struct LatencyMonitor {
    latency_stats: HashMap<String, LatencyStats>,
}
```

**算法参数：**
- `LATENCY_WINDOW`：100 个样本
- `SIGMA_THRESHOLD`：3.0
- `MIN_ANOMALY_SAMPLES`：10

**3-sigma 检测原理：**
```
异常阈值 = 平均值 + 3 × 标准差

约 99.7% 的正态分布数据落在此范围内
超过此值的数据点被判定为异常
```

**关键结构：**
```rust
struct LatencyStats {
    samples: VecDeque<u64>,  // 滑动窗口
    mean: f64,
    variance: f64,
}

impl LatencyStats {
    fn add_sample(&mut self, latency_us: u64);
    fn is_anomaly(&self, latency_us: u64) -> bool;
}
```

---

### HttpWorker

**职责：**
- 使用 actix-web 框架提供 HTTP API 端点
- 查询系统状态
- 提供设备管理接口

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "http_server", blocking = true)]
pub struct HttpWorker {
    config: Config,
}
```

**API 端点：**

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/devices` | GET | 获取所有设备列表 |
| `/api/devices/{id}/status` | GET | 获取单个设备状态 |
| `/api/health` | GET | 系统健康检查 |
| `/api/config` | GET | 当前配置 |
| `/api/config/reload` | POST | 配置重载（由文件监控触发） |

**健康状态：**
- `healthy`：所有设备连接
- `degraded`：部分设备断线
- `unhealthy`：所有设备断线或无设备

**关键点：**
- **无状态设计**：不维护内部状态，从 Variables 读取
- **共享状态**：通过 AppState 访问 device_states

---

### ConfigLoader

**职责：**
- 监视配置文件变化（使用 notify crate）
- 配置变更时发送 ConfigUpdate 消息
- 避免不必要的重载（内容对比）

**Worker 配置：**
```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "config_loader", blocking = true)]
pub struct ConfigLoader {
    config_path: String,
    current_config: Config,
}
```

**工作流程：**
```
监控 config.toml
   │
   ├── 文件修改事件
   │
   ├── 读取新内容
   │
   ├── 对比内容（避免重复）
   │
   └── 发送 ConfigUpdate 消息
```

**内容对比：**
- 使用 serde_json 将配置序列化为 JSON
- 对比新旧配置的 JSON 内容
- 只有内容变化时才发送 ConfigUpdate

## 🎯 优势

### 解耦合

Worker 之间通过消息总线通信，不直接依赖。

### 可扩展

新增消息类型不需要修改其他 Worker。

### 容错性

一个 Worker 失败不影响其他 Worker。

### 可测试

每个组件都有独立的测试。

### 实时性

使用 `parking_lot_rt` 实现实时安全的同步原语。

## 📝 相关文档

- [架构概览](../architecture.md)
- [消息传递机制](../messaging/消息传递机制.md)
- [配置管理](../configuration.md)
- [HTTP API](../http-api.md)
- [测试指南](../testing/测试指南.md)
