# Heartbeat 拆分重构计划

## TL;DR

> **Quick Summary**: 将 ModbusWorker 中的心跳逻辑拆分到独立的 HeartbeatWorker，通过 Hub 发送 GetStatus 请求复用连接，实现真正的心跳检测和延迟测量。
> 
> **Deliverables**:
> - 修改后的 ModbusWorker（移除心跳逻辑）
> - 新的 HeartbeatWorker（独立心跳检测）
> - 修改后的 LatencySample（device_id: String）
> - 修改后的 LatencyMonitor（适配 String 类型）
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: NO - sequential (依赖关系)
> **Critical Path**: Task 1 → Task 2/3 → Task 4 → Task 5/6

---

## Context

### Original Request

用户发现 ModbusWorker 中包含心跳消息发送，存在两个问题：
1. 当长时间无消息时，心跳信号不会发送
2. 心跳是对 ModbusWorker 的监控，而不是对设备的监控

用户希望将 Modbus 的 heartbeat 拆分开来，ModbusWorker 只进行设备的 device control 信号的处理。

### Interview Summary

**Key Discussions**:
- 心跳检测方式：方案 B（复用 ModbusClient 连接，发送 GetStatus 请求）
- device_id 类型：将 LatencySample.device_id 从 u32 改为 String
- 架构选择：通过 Hub 消息机制复用 ModbusWorker 的连接

**Research Findings**:
- 当前心跳逻辑在 worker.rs 520-539 行
- ModbusWorker 使用阻塞 `for msg in hub_client` 模式
- LatencyMonitor 中有 `parse::<u32>()` 需要移除
- GetStatus 操作只返回状态，不读取寄存器

### Metis Review

**Identified Gaps** (addressed):
- HeartbeatWorker 启动顺序问题：需优雅处理 ModbusWorker 未准备好
- 连接超时处理：HeartbeatWorker 应设置合理超时并记录警告
- 设备离线检测：GetStatus 超时即认为离线

---

## Work Objectives

### Core Objective

将心跳逻辑从 ModbusWorker 拆分到独立 HeartbeatWorker，实现：
1. 心跳定时发送，不受消息流量影响
2. 真实设备延迟测量
3. 类型一致性（device_id 使用 String）

### Concrete Deliverables

- `src/lib.rs`: LatencySample.device_id 改为 String
- `src/workers/modbus/worker.rs`: 删除心跳逻辑
- `src/workers/latency_monitor.rs`: 适配 String 类型
- `src/workers/heartbeat_worker.rs`: 新建心跳检测 worker
- `src/workers/mod.rs`: 注册 HeartbeatWorker
- `src/main.rs`: 启动 HeartbeatWorker

### Definition of Done

- [ ] `cargo build` 编译通过
- [ ] `cargo test` 所有测试通过
- [ ] 心跳能独立于消息流量定期发送
- [ ] LatencySample.device_id 为 String 类型
- [ ] LatencyMonitor 正常工作

### Must Have

- 心跳逻辑完全从 ModbusWorker 移除
- HeartbeatWorker 能通过 Hub 发送 GetStatus 并获得响应
- LatencySample.device_id 类型统一为 String

### Must NOT Have (Guardrails)

- 不改变 DeviceHeartbeat 消息格式（外部消费者依赖）
- 不改变 Variables 结构体字段
- 不增加新的配置选项
- 不改变 Modbus 协议处理逻辑
- 不破坏现有测试

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: Tests-after
- **Framework**: cargo test

### Agent-Executed QA Scenarios

**Scenario 1: 编译验证**
```
Tool: Bash
Steps:
  1. cargo build --release
  2. Assert: exit code 0
  3. Assert: no errors
Expected Result: 编译成功
Evidence: Build output
```

**Scenario 2: 单元测试**
```
Tool: Bash
Steps:
  1. cargo test
  2. Assert: all tests pass
Expected Result: 所有测试通过
Evidence: Test output
```

**Scenario 3: 心跳独立性验证**
```
Tool: Bash
Preconditions: 运行程序，无外部请求
Steps:
  1. 启动程序
  2. 等待 60 秒不发送任何请求
  3. 检查日志中的 DeviceHeartbeat 消息
Expected Result: 即使无请求，心跳仍然定期发送
Evidence: 日志输出
```

---

## Execution Strategy

### Dependency Matrix

| Task | Depends On | Blocks | Can Parallelize With |
|------|------------|--------|---------------------|
| 1 | None | 2, 3 | None |
| 2 | 1 | 4 | 3 |
| 3 | 1 | 4 | 2 |
| 4 | 2, 3 | 5, 6 | None |
| 5 | 4 | None | 6 |
| 6 | 4 | None | 5 |

### Critical Path

Task 1 → Task 2/3 → Task 4 → Task 5/6

---

## TODOs

### Task 1: 修改 LatencySample 类型

**What to do**:
- 将 `LatencySample.device_id` 从 `u32` 改为 `String`
- 移除 `Copy` trait（String 不是 Copy）

**File**: `src/lib.rs`

**Changes**:
```rust
// 修改前 (第 229-241 行)
#[derive(Clone, Debug, Copy)]
pub struct LatencySample {
    pub device_id: u32,
    pub latency_us: u64,
    pub timestamp_ms: u64,
}

// 修改后
#[derive(Clone, Debug)]  // 移除 Copy
pub struct LatencySample {
    pub device_id: String,  // 改为 String
    pub latency_us: u64,
    pub timestamp_ms: u64,
}
```

**Must NOT do**:
- 不改变其他字段类型
- 不添加新的字段

**Recommended Agent Profile**:
- **Category**: `quick`
- **Skills**: None needed
- **Reason**: 简单的类型修改

**Parallelization**:
- **Can Run In Parallel**: NO
- **Blocks**: Task 2, Task 3

**References**:
- `src/lib.rs:229-241` - LatencySample 定义
- `src/workers/latency_monitor.rs:537` - LatencySample 使用处

**Acceptance Criteria**:
- [ ] LatencySample.device_id 类型为 String
- [ ] 编译通过（可能有其他地方需要修复）

**Commit**: YES
- Message: `refactor(types): change LatencySample.device_id to String`
- Files: `src/lib.rs`

---

### Task 2: 修改 ModbusWorker - 删除心跳逻辑

**What to do**:
- 删除 `last_heartbeat` 字段
- 删除 `last_heartbeat` 初始化
- 删除心跳检查和发送逻辑
- 更新 `record_communication` 方法传递 device_id

**File**: `src/workers/modbus/worker.rs`

**Changes**:

1. **删除字段** (第 27 行):
```rust
// 删除这行
last_heartbeat: SystemTime,
```

2. **删除初始化** (第 44 行):
```rust
// 删除这行
last_heartbeat: SystemTime::UNIX_EPOCH,
```

3. **删除心跳逻辑** (第 520-539 行):
```rust
// 删除整个代码块
// Handle heartbeat
let now = SystemTime::now();
if now
    .duration_since(self.last_heartbeat)
    .unwrap_or(Duration::ZERO)
    >= Duration::from_secs(self.device.heartbeat_interval_sec as u64)
{
    // ... 全部删除
}
```

4. **更新 record_communication_with** (第 107-123 行):
```rust
// 修改前
fn record_communication_with<F>(&mut self, latency_us: u64, mut emit: F)
where
    F: FnMut(LatencySample),
{
    let now = SystemTime::now();
    self.last_communication = Some(now);

    let sample = LatencySample {
        device_id: 0,  // ← 修改这里
        latency_us,
        timestamp_ms: now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    emit(sample);
}

// 修改后
fn record_communication_with<F>(&mut self, device_id: &str, latency_us: u64, mut emit: F)
where
    F: FnMut(LatencySample),
{
    let now = SystemTime::now();
    self.last_communication = Some(now);

    let sample = LatencySample {
        device_id: device_id.to_string(),  // 使用传入的 device_id
        latency_us,
        timestamp_ms: now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    emit(sample);
}
```

5. **更新 record_communication** (第 125-129 行):
```rust
// 修改前
fn record_communication(&mut self, context: &Context<Message, Variables>, latency_us: u64) {
    self.record_communication_with(latency_us, |sample: LatencySample| {
        let _ = context.variables().latency_samples.force_push(sample);
    });
}

// 修改后
fn record_communication(&mut self, context: &Context<Message, Variables>, latency_us: u64) {
    self.record_communication_with(&self.device.id, latency_us, |sample: LatencySample| {
        let _ = context.variables().latency_samples.force_push(sample);
    });
}
```

**Must NOT do**:
- 不改变消息处理逻辑
- 不改变连接管理逻辑
- 不改变 GetStatus 的返回格式

**Recommended Agent Profile**:
- **Category**: `quick`
- **Skills**: None needed
- **Reason**: 删除代码和小修改

**Parallelization**:
- **Can Run In Parallel**: NO (依赖 Task 1)
- **Blocked By**: Task 1
- **Blocks**: Task 4

**References**:
- `src/workers/modbus/worker.rs:27` - last_heartbeat 字段
- `src/workers/modbus/worker.rs:44` - last_heartbeat 初始化
- `src/workers/modbus/worker.rs:520-539` - 心跳逻辑
- `src/workers/modbus/worker.rs:107-129` - record_communication 方法

**Acceptance Criteria**:
- [ ] `last_heartbeat` 字段已删除
- [ ] 心跳发送逻辑已删除
- [ ] `record_communication` 正确传递 device_id
- [ ] 编译通过

**Commit**: YES
- Message: `refactor(modbus): remove heartbeat logic from ModbusWorker`
- Files: `src/workers/modbus/worker.rs`

---

### Task 3: 适配 LatencyMonitor

**What to do**:
- 移除 `parse::<u32>()` 转换
- 直接使用 String 类型的 device_id

**File**: `src/workers/latency_monitor.rs`

**Changes**:

修改 `run()` 方法中的消息处理 (约第 521-548 行):
```rust
// 修改前
if let Message::DeviceHeartbeat {
    device_id,
    timestamp_ms,
    latency_us,
} = msg
{
    let device_id_num = device_id.parse::<u32>().unwrap_or(0);  // ← 删除这行

    let sample = LatencySample {
        device_id: device_id_num,  // ← 改为 device_id.clone()
        latency_us,
        timestamp_ms,
    };

    context.variables().latency_samples.force_push(sample);

    if let Some(event) =
        self.process_latency_sample(&device_id, latency_us, timestamp_ms)
    {
        context.variables().device_events.force_push(event);
    }
}

// 修改后
if let Message::DeviceHeartbeat {
    device_id,
    timestamp_ms,
    latency_us,
} = msg
{
    let sample = LatencySample {
        device_id: device_id.clone(),  // 直接使用 String
        latency_us,
        timestamp_ms,
    };

    context.variables().latency_samples.force_push(sample);

    if let Some(event) =
        self.process_latency_sample(&device_id, latency_us, timestamp_ms)
    {
        context.variables().device_events.force_push(event);
    }
}
```

**Must NOT do**:
- 不改变 3-sigma 算法逻辑
- 不改变 LatencyStats 结构

**Recommended Agent Profile**:
- **Category**: `quick`
- **Skills**: None needed
- **Reason**: 简单的类型适配

**Parallelization**:
- **Can Run In Parallel**: YES (with Task 2)
- **Blocked By**: Task 1
- **Blocks**: Task 4

**References**:
- `src/workers/latency_monitor.rs:521-548` - 消息处理

**Acceptance Criteria**:
- [ ] 不再有 `parse::<u32>()` 转换
- [ ] 编译通过

**Commit**: YES (groups with Task 2)
- Message: `refactor(latency): adapt LatencyMonitor for String device_id`
- Files: `src/workers/latency_monitor.rs`

---

### Task 4: 新建 HeartbeatWorker

**What to do**:
- 创建新的 HeartbeatWorker
- 实现定期发送 GetStatus 请求
- 测量真实延迟
- 广播 DeviceHeartbeat
- 记录 LatencySample
- 更新设备状态

**File**: `src/workers/heartbeat_worker.rs` (新建)

**完整代码**:

```rust
//! HeartbeatWorker - 独立心跳检测 Worker
//!
//! 职责：
//! - 定期检查所有设备是否在线
//! - 通过发送 GetStatus 请求复用 ModbusWorker 的连接
//! - 广播 DeviceHeartbeat 消息（包含真实延迟）
//! - 记录延迟到 latency_samples
//! - 更新设备状态到共享变量

use crate::config::Config;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::event_matches;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 心跳检测 Worker
///
/// 通过发送 GetStatus 请求来检测设备是否在线，
/// 复用 ModbusWorker 已建立的连接。
#[derive(WorkerOpts)]
#[worker_opts(name = "heartbeat_worker", blocking = true)]
pub struct HeartbeatWorker {
    config: Config,
    /// 下一个心跳检查的设备索引（轮询）
    current_device_index: usize,
    /// 全局心跳间隔（秒）- 取所有设备的最小值
    heartbeat_interval_sec: u32,
    /// 心跳超时（秒）- 等待响应的最大时间
    heartbeat_timeout_sec: u32,
}

impl HeartbeatWorker {
    /// 创建新的 HeartbeatWorker
    pub fn new(config: Config) -> Self {
        // 计算全局心跳间隔（取所有设备的最小值）
        let heartbeat_interval_sec = config
            .devices
            .iter()
            .map(|d| d.heartbeat_interval_sec)
            .min()
            .unwrap_or(30);

        Self {
            config,
            current_device_index: 0,
            heartbeat_interval_sec,
            heartbeat_timeout_sec: 5, // 默认 5 秒超时
        }
    }

    /// 发送心跳请求并等待响应
    ///
    /// 返回：(是否在线, 延迟微秒)
    fn ping_device(
        &self,
        device_id: &str,
        context: &Context<Message, Variables>,
    ) -> (bool, u64) {
        let start = SystemTime::now();
        let correlation_id = Self::generate_correlation_id();

        // 创建响应通道
        let (tx, rx) = mpsc::channel();

        // 发送 GetStatus 请求
        let send_result = context.hub().send(Message::DeviceControl {
            device_id: device_id.to_string(),
            operation: crate::messages::Operation::GetStatus,
            params: serde_json::json!({}),
            correlation_id,
            respond_to: Some(tx),
        });

        if send_result.is_err() {
            tracing::warn!(device_id = %device_id, "Failed to send heartbeat request");
            return (false, 0);
        }

        // 等待响应（带超时）
        let timeout = Duration::from_secs(self.heartbeat_timeout_sec as u64);
        match rx.recv_timeout(timeout) {
            Ok((success, _data, _error)) => {
                let latency_us = start
                    .elapsed()
                    .unwrap_or(Duration::ZERO)
                    .as_micros() as u64;

                (success, latency_us)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    device_id = %device_id,
                    timeout_sec = self.heartbeat_timeout_sec,
                    "Heartbeat request timed out"
                );
                (false, 0)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!(device_id = %device_id, "Response channel disconnected");
                (false, 0)
            }
        }
    }

    /// 生成唯一的 correlation_id
    fn generate_correlation_id() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// 更新设备状态到共享变量
    fn update_device_status(
        &self,
        device_id: &str,
        connected: bool,
        context: &Context<Message, Variables>,
    ) {
        let mut states = context.variables().device_states.write();
        if let Some(status) = states.get_mut(device_id) {
            let was_connected = status.connected;
            status.connected = connected;
            status.last_communication = std::time::Instant::now();

            // 如果状态发生变化，记录事件
            if was_connected != connected {
                let event_type = if connected {
                    DeviceEventType::Connected
                } else {
                    DeviceEventType::Disconnected
                };

                let event = DeviceEvent {
                    device_id: device_id.to_string(),
                    event_type,
                    timestamp_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    details: format!(
                        "Device {} via heartbeat check",
                        if connected { "connected" } else { "disconnected" }
                    ),
                };

                // 释放锁后再推送事件
                drop(states);
                context.variables().device_events.force_push(event);
            }
        }
    }

    /// 广播心跳消息并记录延迟
    fn broadcast_heartbeat(
        &self,
        device_id: &str,
        connected: bool,
        latency_us: u64,
        context: &Context<Message, Variables>,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // 广播 DeviceHeartbeat 消息
        let _ = context.hub().send(Message::DeviceHeartbeat {
            device_id: device_id.to_string(),
            timestamp_ms,
            latency_us,
        });

        // 记录 LatencySample
        if connected && latency_us > 0 {
            let sample = LatencySample {
                device_id: device_id.to_string(),
                latency_us,
                timestamp_ms,
            };
            context.variables().latency_samples.force_push(sample);
        }

        tracing::trace!(
            device_id = %device_id,
            connected = connected,
            latency_us = latency_us,
            "Heartbeat completed"
        );
    }
}

impl Worker<Message, Variables> for HeartbeatWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let device_count = self.config.devices.len();

        if device_count == 0 {
            tracing::warn!("No devices configured, HeartbeatWorker will idle");
            while context.is_online() {
                std::thread::sleep(Duration::from_secs(10));
            }
            return Ok(());
        }

        // 计算每个设备的检查间隔
        // 平均分配检查时间，避免同时发送大量请求
        let per_device_interval = Duration::from_secs(
            self.heartbeat_interval_sec as u64 / device_count as u64
        );
        let min_interval = Duration::from_millis(100); // 最小间隔 100ms
        let check_interval = per_device_interval.max(min_interval);

        tracing::info!(
            devices_count = device_count,
            heartbeat_interval_sec = self.heartbeat_interval_sec,
            check_interval_ms = check_interval.as_millis(),
            "HeartbeatWorker started"
        );

        while context.is_online() {
            // 获取当前要检查的设备
            let device = &self.config.devices[self.current_device_index];

            tracing::debug!(
                device_id = %device.id,
                device_index = self.current_device_index,
                "Checking device heartbeat"
            );

            // 发送心跳请求
            let (connected, latency_us) = self.ping_device(&device.id, context);

            // 更新设备状态
            self.update_device_status(&device.id, connected, context);

            // 广播心跳消息
            self.broadcast_heartbeat(&device.id, connected, latency_us, context);

            // 移动到下一个设备
            self.current_device_index = (self.current_device_index + 1) % device_count;

            // 等待下一个检查周期
            std::thread::sleep(check_interval);
        }

        tracing::info!("HeartbeatWorker stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        use crate::config::{Device, DeviceType, ServerConfig};
        use std::collections::HashMap;

        Config {
            server: ServerConfig {
                rpc_port: 8080,
                http_port: 8081,
            },
            logging: crate::config::LoggingConfig {
                level: "info".to_string(),
                file: None,
                daily_rotation: false,
            },
            devices: vec![Device {
                id: "test-device".to_string(),
                device_type: DeviceType::Plc,
                address: "127.0.0.1".to_string(),
                port: 502,
                unit_id: 1,
                addressing_mode: Default::default(),
                byte_order: Default::default(),
                tcp_nodelay: true,
                max_concurrent_ops: 3,
                heartbeat_interval_sec: 30,
                signal_groups: vec![],
            }],
        }
    }

    #[test]
    fn heartbeat_worker_calculates_interval() {
        let config = test_config();
        let worker = HeartbeatWorker::new(config);

        assert_eq!(worker.heartbeat_interval_sec, 30);
    }

    #[test]
    fn correlation_id_increments() {
        let id1 = HeartbeatWorker::generate_correlation_id();
        let id2 = HeartbeatWorker::generate_correlation_id();

        assert!(id2 > id1);
    }
}
```

**Must NOT do**:
- 不直接创建 ModbusClient 连接
- 不改变 DeviceHeartbeat 消息格式
- 不添加新的配置字段

**Recommended Agent Profile**:
- **Category**: `unspecified-low`
- **Skills**: None needed
- **Reason**: 新建文件，逻辑清晰

**Parallelization**:
- **Can Run In Parallel**: NO
- **Blocked By**: Task 2, Task 3
- **Blocks**: Task 5, Task 6

**References**:
- `src/workers/modbus/worker.rs:365-384` - GetStatus 处理逻辑
- `src/workers/latency_monitor.rs` - LatencySample 使用参考
- `src/messages.rs:101-113` - DeviceHeartbeat 消息定义

**Acceptance Criteria**:
- [ ] HeartbeatWorker 正确创建
- [ ] 能发送 GetStatus 请求
- [ ] 能测量延迟
- [ ] 能广播 DeviceHeartbeat
- [ ] 能记录 LatencySample
- [ ] 编译通过

**Commit**: YES
- Message: `feat(workers): add HeartbeatWorker for device health monitoring`
- Files: `src/workers/heartbeat_worker.rs`

---

### Task 5: 注册 HeartbeatWorker

**What to do**:
- 在 mod.rs 中添加 heartbeat_worker 模块
- 导出 HeartbeatWorker

**File**: `src/workers/mod.rs`

**Changes**:
```rust
// 添加模块声明
pub mod heartbeat_worker;

// 添加导出
pub use heartbeat_worker::HeartbeatWorker;
```

**Recommended Agent Profile**:
- **Category**: `quick`
- **Skills**: None needed

**Parallelization**:
- **Can Run In Parallel**: YES (with Task 6)
- **Blocked By**: Task 4

**References**:
- `src/workers/mod.rs` - 现有模块结构

**Acceptance Criteria**:
- [ ] heartbeat_worker 模块已注册
- [ ] HeartbeatWorker 已导出
- [ ] 编译通过

**Commit**: YES (groups with Task 6)
- Message: `feat(workers): register HeartbeatWorker`
- Files: `src/workers/mod.rs`

---

### Task 6: 启动 HeartbeatWorker

**What to do**:
- 在 main.rs 中导入 HeartbeatWorker
- 在 ModbusWorker 之后启动 HeartbeatWorker

**File**: `src/main.rs`

**Changes**:

1. 添加导入：
```rust
use roboplc_middleware::workers::heartbeat_worker::HeartbeatWorker;
```

2. 在 ModbusWorker 之后启动：
```rust
// 6. 为每个设备创建一个 ModbusWorker
for device in &config.devices {
    controller.spawn_worker(ModbusWorker::new(device.clone()))?;
}

// 7. HeartbeatWorker - 心跳检测
// 注意：必须在 ModbusWorker 之后启动
controller.spawn_worker(HeartbeatWorker::new(config.clone()))?;
```

**Recommended Agent Profile**:
- **Category**: `quick`
- **Skills**: None needed

**Parallelization**:
- **Can Run In Parallel**: YES (with Task 5)
- **Blocked By**: Task 4

**References**:
- `src/main.rs:102-119` - 现有 worker 启动代码

**Acceptance Criteria**:
- [ ] HeartbeatWorker 已导入
- [ ] HeartbeatWorker 在 ModbusWorker 之后启动
- [ ] 编译通过

**Commit**: YES (groups with Task 5)
- Message: `feat(main): start HeartbeatWorker in main`
- Files: `src/main.rs`

---

## Commit Strategy

| After Task | Message | Files |
|------------|---------|-------|
| 1 | `refactor(types): change LatencySample.device_id to String` | `src/lib.rs` |
| 2 | `refactor(modbus): remove heartbeat logic from ModbusWorker` | `src/workers/modbus/worker.rs` |
| 3 | `refactor(latency): adapt LatencyMonitor for String device_id` | `src/workers/latency_monitor.rs` |
| 4 | `feat(workers): add HeartbeatWorker for device health monitoring` | `src/workers/heartbeat_worker.rs` |
| 5+6 | `feat(workers): register and start HeartbeatWorker` | `src/workers/mod.rs`, `src/main.rs` |

---

## Success Criteria

### Verification Commands

```bash
# 编译验证
cargo build --release
# Expected: 成功编译，无错误

# 单元测试
cargo test
# Expected: 所有测试通过

# 运行验证
cargo run
# Expected: HeartbeatWorker 启动日志出现
# Expected: DeviceHeartbeat 消息定期发送
```

### Final Checklist

- [ ] LatencySample.device_id 为 String 类型
- [ ] ModbusWorker 心跳逻辑已移除
- [ ] HeartbeatWorker 正常工作
- [ ] LatencyMonitor 适配完成
- [ ] 所有测试通过
- [ ] 编译无警告