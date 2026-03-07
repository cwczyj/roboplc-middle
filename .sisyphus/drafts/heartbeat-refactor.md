# Draft: Heartbeat 拆分方案

## 背景

当前 ModbusWorker 存在两个问题：
1. 长时间无消息时心跳不发送（阻塞在 `for msg in hub_client`）
2. 心跳是对 ModbusWorker 的监控，而非对设备的监控（`latency_us: 0` 硬编码）

## 解决方案

将心跳逻辑从 ModbusWorker 拆分到独立的 HeartbeatWorker：
- ModbusWorker 只处理设备控制（Read/Write SignalGroup, GetStatus）
- HeartbeatWorker 通过发送 GetStatus 请求复用连接，检测设备在线状态

## 确认的设计决策

1. **架构选择**: 方案 A - 改造 ModbusWorker + 新建 HeartbeatWorker
2. **心跳检测方式**: 复用 ModbusClient 连接，发送 GetStatus 请求
3. **device_id 类型**: 将 LatencySample.device_id 从 u32 改为 String，与 Device.id 保持一致

---

## 修改文件清单

| # | 文件 | 操作 | 改动说明 |
|---|------|------|---------|
| 1 | `src/lib.rs` | 修改 | LatencySample.device_id: u32 → String，移除 Copy trait |
| 2 | `src/workers/modbus/worker.rs` | 修改 | 删除心跳字段和逻辑，更新 record_communication |
| 3 | `src/workers/latency_monitor.rs` | 修改 | 适配 String 类型 device_id |
| 4 | `src/workers/heartbeat_worker.rs` | 新建 | 独立心跳检测 worker |
| 5 | `src/workers/mod.rs` | 修改 | 注册 HeartbeatWorker |
| 6 | `src/main.rs` | 修改 | 启动 HeartbeatWorker |

---

## 详细修改内容

### 任务 1: 修改 LatencySample 类型 (src/lib.rs)

**变更**:
- `device_id: u32` → `device_id: String`
- 移除 `Copy` trait（String 不是 Copy）
- 保留 `Clone, Debug`

### 任务 2: 修改 ModbusWorker (src/workers/modbus/worker.rs)

**删除**:
1. `last_heartbeat` 字段
2. `last_heartbeat` 初始化
3. 心跳检查和发送逻辑（520-539行）

**修改**:
- `record_communication_with`: 添加 device_id 参数
- `record_communication`: 传递 device_id

### 任务 3: 适配 LatencyMonitor (src/workers/latency_monitor.rs)

**修改**:
- `run()` 中移除 `parse::<u32>()` 转换
- 直接使用 String 类型的 device_id

### 任务 4: 新建 HeartbeatWorker (src/workers/heartbeat_worker.rs)

**功能**:
- 定期轮询所有设备
- 发送 GetStatus 请求复用 ModbusWorker 连接
- 测量真实延迟
- 广播 DeviceHeartbeat
- 记录 LatencySample
- 更新设备状态

### 任务 5: 注册 HeartbeatWorker (src/workers/mod.rs)

**添加**:
- `pub mod heartbeat_worker;`
- `pub use heartbeat_worker::HeartbeatWorker;`

### 任务 6: 启动 HeartbeatWorker (src/main.rs)

**添加**:
- 导入 HeartbeatWorker
- 在 ModbusWorker 之后启动

---

## 架构图

```
┌─────────────────┐
│  HeartbeatWorker │
│  (独立定时器)     │
└────────┬────────┘
         │ DeviceControl(GetStatus)
         ▼
┌─────────────────┐     ┌─────────────────┐
│  DeviceManager  │────▶│  ModbusWorker   │
│  (Hub Router)   │     │  (复用连接)      │
└─────────────────┘     └────────┬────────┘
                                 │ DeviceResponse
                                 ▼
                        ┌─────────────────┐
                        │ HeartbeatWorker │
                        │ 测量延迟        │
                        │ 广播心跳        │
                        └────────┬────────┘
                                 │ DeviceHeartbeat (广播)
                                 ▼
                        ┌─────────────────┐
                        │ LatencyMonitor  │
                        │ (3-sigma 异常检测) │
                        └─────────────────┘
```

---

## 风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| LatencySample 类型改动影响范围 | 中 | 编译器会提示所有使用处 |
| LatencyMonitor 适配问题 | 低 | 只需移除类型转换 |
| ModbusWorker 改动影响稳定性 | 低 | 只删除代码，不改变核心逻辑 |
| HeartbeatWorker 新代码有 bug | 低 | 逻辑简单，使用已有机制 |

---

## 测试计划

1. `cargo build` - 验证编译通过
2. `cargo test` - 运行单元测试
3. `cargo run` - 手动测试心跳功能
4. 验证设备离线检测
5. 验证 LatencyMonitor 异常检测