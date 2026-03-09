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
| **DeviceManager** | - | 消息路由器，协调 Worker 间通信 |
| **ModbusWorker** | - | Modbus TCP 客户端（每设备一个） |
| **HeartbeatWorker** | - | 独立心跳检测，延迟跟踪 |
| **LatencyMonitor** | - | 3-sigma 延迟异常检测 |
| **ConfigLoader** | - | 配置热重载（文件监控） |

## 核心设计模式

### 1. 请求-响应模式
每个设备控制请求都包含唯一的 `correlation_id`，用于匹配响应：
- **请求端**：生成 correlation_id 并发送 DeviceControl
- **响应端**：接收 DeviceResponse 并根据 correlation_id 路由回请求者
- **超时处理**：未收到响应时返回错误，触发 TimeoutCleanup 清理

### 2. 消息类型优先级
消息在 Hub 中按优先级投递：
1. **SystemStatus** - 最高优先级，系统控制消息
2. **DeviceControl/DeviceResponse** - 设备控制相关（单播）
3. **DeviceHeartbeat** - 心跳消息（广播）
4. **ConfigUpdate** - 配置更新通知（广播）

### 3. 操作队列 (OperationQueue)
- **并发控制**：限制同时进行的 Modbus 操作数量
- **背压控制**：队列满时拒绝新操作
- **重试策略**：操作失败自动重试

### 4. 连接管理
- **自动重连**：连接断开时使用指数退避策略
- **心跳机制**：HeartbeatWorker 独立检测设备在线状态
- **超时检测**：连接超时触发重连

### 5. 延迟监控
- **采样窗口**：每个设备维护 100 个延迟样本
- **3-sigma 检测**：超过平均值 + 3倍标准差判定为异常
- **最小样本**：至少需要 10 个样本才开始异常检测

## 文档索引

- [消息传递机制](messaging/消息传递机制.md)
- [配置管理](configuration/配置管理.md)
- [Worker 模块](workers/worker模块.md)
- [HTTP API](http-api.md)

[返回顶部](#-roplc-架构文档)
