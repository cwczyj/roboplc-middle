# RoboPLC 架构文档

> 本文档说明 RoboPLC 中间件系统的整体架构、各模块的功能和设计决策。

## 📋 文档导航

- [架构概览](#架构概览) - 整体架构设计说明
- [HTTP API 模块](#http-api-模块) - REST API 端点设计
- [消息传递机制](#消息传递机制) - Hub 消息流设计
- [Worker 模块](#worker-模块) - Worker 架构说明
- [配置管理](#配置管理) - 配置加载和热更新
- [测试指南](#测试指南) - 如何运行和测试系统

## 🏗️ 架构设计决策

### 消息驱动架构

整个系统采用**消息驱动架构**，各 Worker 通过 RoboPLC Hub 交换消息：
- **解耦合**：Worker 之间不直接依赖，通过消息总线通信
- **可扩展**：新增消息类型或 Worker 不需要修改其他 Worker
- **容错性**：消息发送失败不会导致系统崩溃

### Worker 分层

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│   JSON-RPC      │────▶│  Device Manager  │────▶│   Modbus    │
│   Server        │     │  (Hub Router)    │     │   Workers   │
│   (port 8080)   │     └──────────────────┘     │  (per device)│
└─────────────────┘              │               └─────────────┘
        │                        │                       │
        │                        ▼                       │
        │              ┌──────────────────┐             │
        └─────────────▶│  HTTP API       │◀────────────┘
                       │  (port 8081)    │
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

### Worker 类型

| Worker | 端口 | 职责 |
|--------|------|------|
| **RpcWorker** | 8080 | JSON-RPC 2.0 服务器（异步架构） |
| **HttpWorker** | 8081 | HTTP REST API 管理接口 |
| **DeviceManager** | - | 设备注册和超时清理，不再路由 DeviceControl 消息 |
| **ModbusWorker** | - | Modbus TCP 客户端（每设备一个） |
| **HeartbeatWorker** | - | 独立心跳检测，延迟跟踪 |
| **LatencyMonitor** | - | 3-sigma 延迟异常检测 |
| **ConfigLoader** | - | 配置热重载（文件监控） |

## 核心设计模式

### 1. 请求-响应模式

每个设备控制请求都包含唯一的 `correlation_id`，用于匹配响应：
- **请求端**：生成 correlation_id 并发送 DeviceControl
- **响应端**：ModbusWorker 直接使用 respond_to 通道发送响应给 RpcWorker
- **超时处理**：未收到响应时返回错误，触发 TimeoutCleanup 清理

**架构演进说明**：DeviceManager 不再订阅 DeviceControl 消息，ModbusWorker 直接使用 respond_to 通道进行响应，避免了消息循环。

### 2. 消息类型

消息在 Hub 中按类型投递：
1. **DeviceHeartbeat** - 心跳消息（广播，always 投递）
2. **ConfigUpdate** - 配置更新通知（广播，always 投递）
3. **DeviceControl** - 设备控制相关（单播，由 ModbusWorker 订阅）
4. **DeviceResponse** - 设备响应（不再经过 DeviceManager）
5. **TimeoutCleanup** - 超时清理消息
6. **SystemStatus** - 系统状态查询

### 3. 直接响应机制

使用 `respond_to: Option<Sender<DeviceResponseData>>` 实现直接响应：
- RpcWorker 创建 oneshot 通道
- DeviceControl 携带 respond_to 发送端
- ModbusWorker 完成操作后直接通过 respond_to 发送响应
- 避免了 DeviceManager 路由开销和潜在的消息循环

### 4. 连接管理

- **自动重连**：连接断开时使用指数退避策略
- **心跳机制**：HeartbeatWorker 独立检测设备在线状态
- **超时检测**：连接超时触发重连
- **连接探测**：每次操作前进行轻量级连接探测

### 5. 延迟监控

- **采样窗口**：每个设备维护 100 个延迟样本
- **3-sigma 检测**：超过平均值 + 3倍标准差判定为异常
- **最小样本**：至少需要 10 个样本才开始异常检测

## 消息流架构（Wave 2 异步重构）

### RpcWorker 异步架构

RpcWorker 采用新的异步架构：

1. **在 blocking worker 中 spawn tokio runtime**
2. **使用 `tokio::net::TcpListener`** 进行异步 TCP 接收
3. **使用 `tokio::select!`** 进行并发处理
4. **使用 `tokio::sync::mpsc`** 在 RpcHandler 和主循环间传递请求
5. **使用 `tokio::sync::oneshot`** 进行响应处理

```
RPC Worker Thread
    │
    ├── tokio runtime (spawn in blocking worker)
    │   │
    │   ├── TcpListener::bind() - 异步监听
    │   │
    │   ├── tokio::select! {
    │   │       accept connection -> spawn connection handler
    │   │       recv device_control_rx -> forward to Hub
    │   │       recv shutdown_rx -> break
    │   │       interval cleanup -> cleanup timed-out requests
    │   │   }
    │   │
    │   └── Connection handler:
    │       ├── read request with timeout
    │       ├── spawn_blocking(RpcServer::handle_request_payload)
    │       └── write response with timeout
    │
    └── Main loop:
        └── wait for shutdown signal (context.is_online())
```

### 请求处理流程

```
1. 客户端发送 JSON-RPC 请求
   │
2. RpcHandler::handle_call() 接收请求
   │
3. 生成 correlation_id（原子递增）
   │
4. 创建 oneshot 通道 (response_tx, response_rx)
   │
5. 构建 DeviceControlRequest { ..., respond_to: response_tx }
   │
6. 通过 mpsc::blocking_send() 发送到 device_control_tx
   │
7. Bridge 任务 recv 到请求
   │
8. 创建 Message::DeviceControl 并发送到 Hub
   │
9. response_rx.blocking_recv() 等待响应（带 30 秒超时）
   │
10. 收到响应或超时后返回 JSON-RPC 响应
```

## 项目结构

```
src/
├── lib.rs              # Main library exports, shared state (Variables)
├── main.rs             # Entry point
├── config.rs           # Configuration parsing and validation
├── messages.rs         # Message enums for worker communication
├── data_conversion.rs  # Data type conversion utilities
└── workers/            # Worker implementations
    ├── mod.rs          # Worker module exports
    ├── manager.rs      # Device manager (device registration, timeout cleanup)
    ├── rpc_worker.rs   # JSON-RPC 2.0 server (async architecture)
    ├── http_worker.rs  # HTTP API server
    ├── heartbeat_worker.rs   # Heartbeat detection
    ├── latency_monitor.rs    # Latency anomaly detection
    ├── config_loader.rs      # Hot config reload
    ├── config_updater.rs     # Config update handler
    └── modbus/         # Modbus implementation
        ├── mod.rs
        ├── worker.rs   # ModbusWorker implementation
        ├── client.rs   # Modbus TCP client
        ├── operations.rs # Register operations
        ├── parsing.rs  # Signal group encoding/decoding
        └── types.rs    # Shared types (Backoff, ConnectionState, etc.)

tests/
├── integration_tests.rs        # Worker integration tests
├── e2e_tests.rs               # End-to-end tests
├── async_rpc_tests.rs         # Async RPC tests
├── functional_config_tests.rs # Config functional tests
├── functional_http_tests.rs   # HTTP API functional tests
├── functional_worker_tests.rs # Worker functional tests
└── mock_modbus.rs             # Mock Modbus TCP server

demo/
├── mock_server.rs             # Demo mock server
├── jsonrpc_client.rs          # JSON-RPC client demo
└── register_rpc_demo.rs       # Register RPC demo
```

## 关键依赖

- `roboplc`: Real-time PLC framework (workers, Hub, comm)
- `roboplc-rpc`: JSON-RPC server framework
- `serde`/`serde_json`: Serialization
- `tokio`: Async runtime
- `actix-web`: HTTP server
- `thiserror`: Error handling
- `tracing`: Structured logging
- `notify`: File watching for config reload
- `parking_lot_rt`: Real-time safe synchronization primitives

## 文档索引

- [消息传递机制](messaging/消息传递机制.md) - 详细消息流程
- [配置管理](configuration/配置管理.md) - 配置热更新机制
- [Worker 模块](workers/worker模块.md) - Worker 详细说明
- [HTTP API](http-api.md) - HTTP API 参考
- [测试指南](testing/测试指南.md) - 测试文档

[返回顶部](#-roplc-架构文档)
