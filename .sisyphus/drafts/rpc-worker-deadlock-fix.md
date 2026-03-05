# RpcWorker 死锁问题修复计划 Draft

## 问题总结

### 核心问题
RpcWorker 是单线程阻塞式 worker，导致严重的性能和正确性问题。

### 问题详细分析

#### 问题 1: 单线程阻塞导致吞吐量极低
- **现象**: `send_device_control()` 内部调用 `recv_timeout(30s)` 阻塞
- **影响**: 每个请求阻塞整个 RPC 服务器 30 秒
- **结果**: 无法并发处理多个请求，吞吐量极低

#### 问题 2: 阻塞期间主循环无法执行
- **代码位置**: `rpc_worker.rs` 第433行 `response_rx.recv_timeout(30s)`
- **影响**: 第628行的 `device_control_rx.try_recv()` 永远无法执行
- **结果**: `device_control_rx` 中的消息无法转发到 Hub

#### 问题 3: Modbus 操作无法执行
- **链路**: `device_control_rx.try_recv()` (第628行) → `hub.send()` (第640行) → DeviceManager → ModbusWorker
- **问题**: 阻塞期间链路断开
- **结果**: Modbus 操作完全无法执行

#### 问题 4: 响应通道在超时后关闭
- **代码位置**: `rpc_worker.rs` 第404行 `let (response_tx, response_rx) = channel()`
- **生命周期**: `response_tx` 只在 `send_device_control()` 函数内
- **问题**: 超时返回后，`response_tx` 被丢弃，通道关闭
- **结果**: 即使后续有 `DeviceResponse`，也无法发送给客户端

#### 问题 5: 客户端和系统状态不一致
- **客户端**: T36 收到超时错误 "Request timed out"
- **系统**: T38-T42 Modbus 操作可能成功执行
- **结果**: 客户端认为失败，但设备状态可能已改变

### 时序图

```
T0:   客户端发送 RPC 请求
T1:   listener.accept() 返回
T2:   handle_request_payload() 执行
T3:   device_control_tx.send(request) ✅ 进入队列
T4:   response_rx.recv_timeout(30s) ❌ 阻塞

T5-T34:  主线程阻塞
          ❌ device_control_rx.try_recv() 无法执行
          ❌ 消息无法转发到 Hub
          ❌ DeviceManager 收不到请求
          ❌ ModbusWorker 无法执行

T35:  recv_timeout() 超时
T36:  发送 TimeoutCleanup
      return Ok(Error)
      【response_tx 被丢弃！】
T37:  客户端收到超时错误 ✅

T38:  回到主循环
      device_control_rx.try_recv() ✅ 执行
      hub.send(Message::DeviceControl) ✅ 转发

T40:  ModbusWorker 执行操作 ✅

T42:  DeviceResponse 发送 ✅

T43:  DeviceManager 尝试发送到 response_tx
      ❌ 失败 (通道已关闭)
```

## 修复方案

### 方案 1: 完全异步重构 (推荐)

#### 核心思路
将 RpcWorker 从阻塞式改为异步式，使用 tokio 运行时并发处理每个连接。

#### 优点
- ✅ 完全解决阻塞问题
- ✅ 支持高并发
- ✅ 响应速度快
- ✅ 符合现代 Rust 异步编程模式

#### 缺点
- ⚠️ 需要重构大量代码
- ⚠️ 需要修改 Worker 接口

### 方案 2: 多线程处理每个连接

#### 核心思路
保持阻塞式 worker，但为每个 TCP 连接 spawn 一个独立线程。

#### 优点
- ✅ 修改量较小
- ✅ 保持原有架构

#### 缺点
- ❌ 线程数量不可控
- ❌ 资源消耗高
- ❌ 不符合 RoboPLC 框架模式

### 方案 3: 去除超时，改为轮询

#### 核心思路
在主循环中定期检查 `device_control_rx`，使用 `select!` 同时监听多个事件。

#### 优点
- ✅ 修改量最小
- ✅ 保持阻塞式 worker

#### 缺点
- ❌ 仍然单线程处理
- ❌ 无法真正并发
- ❌ 复杂度增加

### 最终选择: 方案 1 (完全异步重构)

#### 理由
1. 项目已经依赖 tokio (Cargo.toml 第20行)
2. HttpWorker 已经使用 tokio 异步运行时
3. RoboPLC 框架支持异步 worker
4. 这是长期最佳的解决方案

## 修复步骤

### Phase 1: 基础设施重构
1. 修改 RpcWorker 为异步 worker
2. 移除 `blocking = true` 属性
3. 使用 tokio 异步运行时

### Phase 2: 消息处理重构
1. 使用 `tokio::sync::mpsc` 替代 `std::sync::mpsc`
2. 使用 `tokio::sync::oneshot` 替代阻塞等待
3. 移除 `recv_timeout()` 阻塞调用

### Phase 3: 主循环重构
1. 使用 `tokio::select!` 同时监听多个事件
2. 并发处理多个 TCP 连接
3. 异步处理每个 RPC 请求

### Phase 4: 响应处理重构
1. 使用 `oneshot::Sender` 和 `Receiver`
2. 正确处理超时和清理
3. 确保 response_tx 生命周期正确

### Phase 5: 测试和验证
1. 单元测试
2. 集成测试
3. 性能测试

## 技术细节

### 使用 Tokio 异步运行时

```rust
// 修改前
#[worker_opts(name = "rpc_server", blocking = true)]
pub struct RpcWorker {
    config: Config,
}

// 修改后
#[worker_opts(name = "rpc_server")]
pub struct RpcWorker {
    config: Config,
    // 添加 tokio 运行时
    rt: tokio::runtime::Runtime,
}
```

### 使用 Tokio 通道

```rust
// 修改前
let (device_control_tx, device_control_rx) = channel::<DeviceControlRequest>();

// 修改后
let (device_control_tx, device_control_rx) = tokio::sync::mpsc::channel(100);
```

### 使用 Oneshot 通道

```rust
// 修改前
let (response_tx, response_rx) = channel();
match response_rx.recv_timeout(30s) {
    Ok((success, data, error)) => { ... }
    Err(RecvTimeoutError::Timeout) => { ... }
}

// 修改后
let (response_tx, response_rx) = tokio::sync::oneshot::channel();
match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
    Ok(Ok((success, data, error))) => { ... }
    Ok(Err(_)) => { ... }
    Err(_) => { // timeout }
}
```

### 使用 Select 宏

```rust
// 修改前
while context.is_online() {
    match listener.accept() {
        Ok((stream, source)) => { /* 同步处理 */ }
        Err(WouldBlock) => {
            while let Ok(request) = device_control_rx.try_recv() {
                /* 处理 */
            }
        }
    }
}

// 修改后
let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

loop {
    tokio::select! {
        // 处理新连接
        accept_result = listener.accept() => {
            match accept_result {
                Ok((stream, source)) => {
                    // spawn 异步任务处理连接
                    tokio::spawn(handle_connection(stream, source, handler.clone()));
                }
                Err(e) => { /* 错误处理 */ }
            }
        }

        // 处理内部消息
        Some(request) = device_control_rx.recv() => {
            /* 转发到 Hub */
        }
    }
}
```

## 依赖检查

- ✅ tokio (已存在)
- ✅ roboplc-rpc (已存在)
- ⚠️ 需要检查 roboplc-rpc 是否支持异步

## 测试策略

### 单元测试
1. 测试通道发送和接收
2. 测试超时处理
3. 测试清理逻辑

### 集成测试
1. 测试完整的请求-响应流程
2. 测试并发请求处理
3. 测试超时场景

### 性能测试
1. 测试并发吞吐量
2. 测试响应时间
3. 测试资源消耗

## 风险和缓解

### 风险 1: RoboPLC 框架兼容性
- **风险**: RoboPLC 可能不支持异步 worker
- **缓解**: 需要查阅 RoboPLC 文档和示例

### 风险 2: 代码复杂度增加
- **风险**: 异步代码更复杂，调试困难
- **缓解**: 充分的测试和日志

### 风险 3: 向后兼容性
- **风险**: 接口变化可能影响其他模块
- **缓解**: 保持公共 API 不变

## 成功标准

1. ✅ 不再有阻塞点
2. ✅ 支持并发处理多个请求
3. ✅ 响应时间 < 1s
4. ✅ 正确处理超时和清理
5. ✅ 客户端和系统状态一致
6. ✅ 所有测试通过
