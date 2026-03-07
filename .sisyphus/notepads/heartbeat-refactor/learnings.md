# Heartbeat Refactor - Notepad

## Learnings

### 2026-03-07 Task 1 Completed
- LatencySample.device_id 从 u32 改为 String
- 需要移除 Copy trait（String 不是 Copy）
- 编译器会提示所有使用处需要更新

### 2026-03-07 Task 2 Completed
- ModbusWorker 心跳逻辑删除
- last_heartbeat 字段已删除
- 心跳发送代码块已删除

### 2026-03-07 Task 3 Completed
- LatencyMonitor 已适配 String 类型
- 移除了 parse::<u32>() 转换
- 使用 device_id.clone() 替代

### 2026-03-07 Task 4-6 Completed
- HeartbeatWorker 新建完成
- 通过 Hub 发送 GetStatus 请求复用 ModbusWorker 连接
- 测量真实延迟
- 广播 DeviceHeartbeat 消息

### 2026-03-07 验证通过
- cargo build --release 成功
- cargo test 全部通过（135 个测试）

## Decisions

1. HeartbeatWorker 通过 Hub 消息机制复用 ModbusWorker 连接
2. LatencySample.device_id 使用 String 类型与 Device.id 保持一致

## Issues

无