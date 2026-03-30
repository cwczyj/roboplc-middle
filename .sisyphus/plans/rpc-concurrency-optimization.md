# RPC并发性能优化计划

## TL;DR

> **Quick Summary**: 优化中间件以支持多TCP客户端以~50ms频率高并发访问，解决数据丢失问题。分两阶段：快速优化（增加容量、错误处理）和架构优化（实现并发队列、连接池）。
> 
> **Deliverables**: 
> - 第二阶段：max_blocking_threads=128, mpsc容量=5000, Hub错误处理, 合并spawn_blocking
> - 第三阶段：OperationQueue实现, Modbus连接池, 异步架构
> 
> **Estimated Effort**: Medium (第二阶段) + Large (第三阶段)
> **Parallel Execution**: YES - 第二阶段4个独立任务, 第三阶段3个独立任务
> **Critical Path**: T1 → T2 → 验收测试 → T5 → T6 → T7 → 最终验收

---

## Context

### Original Request
多个TCP客户端通过JSON-RPC以~50ms频率访问时出现数据丢失/获取不到结果。

### Interview Summary
**Key Discussions**:
- 根因分析：每个请求占用2个blocking线程，32线程只支持16并发
- ModbusWorker串行处理：OperationQueue设计未实现
- Hub.send()忽略返回值：消息可能静默丢失
- 第一阶段已完成：通道容量、线程池、超时、实时调度

**Research Findings**:
- `OperationQueue` 类型存在于 `types.rs:117-161` 但仅用于测试
- `max_concurrent_ops` 配置在 `config.rs` 定义但未生效
- Hub默认通道容量256-1024，使用policy_channel

### Metis Review
**Identified Gaps** (addressed):
- 缺少客户端数量目标：假设20客户端，400 req/s
- 缺少可测试验收标准：每任务添加QA场景
- 边缘情况未处理：通道满时的行为、Hub错误处理
- spawn_blocking合并方案：需要重构请求处理流程

---

## Work Objectives

### Core Objective
优化中间件以支持20个TCP客户端以50ms频率（400 req/s）并发访问，确保无数据丢失。

### Concrete Deliverables

**第二阶段（快速优化）**:
- `src/workers/rpc/worker.rs`: max_blocking_threads 32→128
- `src/workers/rpc/worker.rs`: mpsc通道容量 1000→5000
- `src/workers/rpc/handler.rs`: Hub.send()返回值处理
- `src/workers/rpc/connection.rs` + `request.rs`: 合并spawn_blocking调用

**第三阶段（架构优化）**:
- `src/workers/modbus/types.rs`: 实现OperationQueue
- `src/workers/modbus/handler.rs`: 集成并发队列
- `src/workers/modbus/client.rs`: 连接池机制

### Definition of Done
- [x] 第二阶段所有修改通过编译和测试
- [x] 负载测试：20客户端×50ms频率，持续5分钟，无请求丢失
- [x] 第三阶段所有修改通过编译和测试
- [x] 并发测试：单设备10并发请求，响应时间<100ms

### Must Have
- 保持JSON-RPC API向后兼容
- 保持config.toml schema向后兼容
- 保持RoboPLC worker调度机制
- 所有现有测试必须通过

### Must NOT Have (Guardrails)
- 不修改JSON-RPC方法签名和响应格式
- 不修改设备配置schema
- 不移除现有日志
- 不增加响应超时超过10s
- 不移除max_concurrent_ops配置字段（保持向后兼容）

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (cargo test)
- **Automated tests**: YES (TDD for new features, tests-after for config changes)
- **Framework**: cargo test / custom load test
- **If TDD**: 每个TODO包含测试用例作为验收标准

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Backend/API**: Use Bash (curl) — Send requests, assert status + response fields
- **Library/Module**: Use Bash (cargo test) — Run tests, verify pass

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (第二阶段 - 立即开始，独立任务):
├── Task 1: 增加 max_blocking_threads 到 128 [quick]
├── Task 2: 增加 mpsc 通道容量到 5000 [quick]
├── Task 3: 添加 Hub.send() 返回值处理 [quick]
└── Task 4: 合并 spawn_blocking 调用 [unspecified-high]

Wave 2 (第二阶段验收):
└── Task 5: 第二阶段负载测试验证 [unspecified-high]

Wave 3 (第三阶段 - 独立任务):
├── Task 6: 实现 OperationQueue 并发队列 [deep]
├── Task 7: 集成 OperationQueue 到 ModbusWorker [deep]
└── Task 8: 实现 Modbus 连接池 [unspecified-high]

Wave FINAL (最终验收):
├── Task F1: 计划合规审计 (oracle)
├── Task F2: 代码质量审查 (unspecified-high)
├── Task F3: 负载测试验证 (unspecified-high)
└── Task F4: 范围保真度检查 (deep)

Critical Path: T1-T4 并行 → T5 → T6-T8 并行 → F1-F4 → 用户确认
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Wave 1), 3 (Wave 3), 4 (Final)
```

### Dependency Matrix

- **1-4**: — — 5, 1
- **5**: 1, 2, 3, 4 — F3, 2
- **6**: — 7, 3
- **7**: 6 — 8, 4
- **8**: — F3, 4

### Agent Dispatch Summary

- **Wave 1**: **4** — T1-T3 → `quick`, T4 → `unspecified-high`
- **Wave 2**: **1** — T5 → `unspecified-high`
- **Wave 3**: **3** — T6-T7 → `deep`, T8 → `unspecified-high`
- **FINAL**: **4** — F1 → `oracle`, F2/F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

### 第二阶段：快速优化

- [x] 1. 增加 max_blocking_threads 到 128

  **What to do**:
  - 修改 `src/workers/rpc/worker.rs:87`
  - 将 `max_blocking_threads(32)` 改为 `max_blocking_threads(128)`
  - 这将支持最多 64 个并发请求（每个请求占用 2 个 blocking 线程）

  **Must NOT do**:
  - 不修改其他 tokio 运行时参数
  - 不改变 worker_threads 数量

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单行配置修改，风险低
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Task 5 (验收测试)
  - **Blocked By**: None

  **References**:
  - `src/workers/rpc/worker.rs:84-90` - Tokio runtime configuration

  **Acceptance Criteria**:
  - [ ] 文件修改完成
  - [ ] `cargo build` 通过
  - [ ] `cargo test` 全部通过

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build 2>&1
      2. Verify exit code 0
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-01-build.log

  Scenario: 测试验证
    Tool: Bash
    Steps:
      1. cargo test 2>&1
      2. Verify all tests pass
    Expected Result: "test result: ok. X passed; 0 failed"
    Evidence: .sisyphus/evidence/task-01-tests.log
  ```

  **Commit**: YES
  - Message: `perf(rpc): increase max_blocking_threads from 32 to 128`
  - Files: `src/workers/rpc/worker.rs`

- [x] 2. 增加 mpsc 通道容量到 5000

  **What to do**:
  - 修改 `src/workers/rpc/worker.rs:67`
  - 将 `mpsc::channel::<DeviceControlRequest>(1000)` 改为 `(5000)`
  - 计算依据：400 req/s × 5s timeout × 2.5 安全系数 = 5000

  **Must NOT do**:
  - 不修改其他通道参数
  - 不改变通道类型

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单行配置修改，风险低
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Task 5 (验收测试)
  - **Blocked By**: None

  **References**:
  - `src/workers/rpc/worker.rs:67` - mpsc channel creation

  **Acceptance Criteria**:
  - [ ] 文件修改完成
  - [ ] `cargo build` 通过
  - [ ] `cargo test` 全部通过

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build 2>&1
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-02-build.log
  ```

  **Commit**: YES
  - Message: `perf(rpc): increase mpsc channel capacity from 1000 to 5000`
  - Files: `src/workers/rpc/worker.rs`

- [x] 3. 添加 Hub.send() 返回值处理

  **What to do**:
  - 修改 `src/workers/rpc/handler.rs:164`
  - 将 `self.hub.send(...)` 改为处理返回值
  - 至少记录错误日志，避免静默丢失

  **Must NOT do**:
  - 不修改消息格式
  - 不添加重试逻辑（保持简单）
  - 不改变超时行为

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单错误处理，风险低
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Task 5 (验收测试)
  - **Blocked By**: None

  **References**:
  - `src/workers/rpc/handler.rs:163-165` - Hub.send() call
  - `src/workers/heartbeat_worker.rs:151` - 示例：忽略返回值 `let _ =`

  **Acceptance Criteria**:
  - [ ] 所有 `hub.send()` 调用都处理返回值
  - [ ] 发送失败时记录 warn 级别日志
  - [ ] `cargo build` 通过
  - [ ] `cargo test` 全部通过

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build 2>&1
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-03-build.log

  Scenario: 代码检查
    Tool: Bash
    Steps:
      1. grep -n "hub().send\|hub.send" src/workers/rpc/*.rs
      2. Verify no lines have "let _ =" before send
    Expected Result: All hub.send() calls handle return value
    Evidence: .sisyphus/evidence/task-03-grep.log
  ```

  **Commit**: YES
  - Message: `fix(rpc): handle Hub.send() return value to detect message loss`
  - Files: `src/workers/rpc/handler.rs`

- [x] 4. 合并 spawn_blocking 调用

  **What to do**:
  - 分析当前两个 spawn_blocking 调用点：
    1. `connection.rs:50` - handle_request_payload
    2. `request.rs:52` - recv_timeout
  - 设计合并方案：将请求处理和响应等待合并到单个 spawn_blocking
  - 实现重构，减少每请求占用线程数从 2 降到 1

  **Must NOT do**:
  - 不改变请求处理逻辑
  - 不增加超时时间
  - 不改变响应格式

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要重构多个文件，中等复杂度
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 5 (验收测试)
  - **Blocked By**: None

  **References**:
  - `src/workers/rpc/connection.rs:48-55` - spawn_blocking for request handling
  - `src/workers/rpc/request.rs:52-76` - spawn_blocking for response waiting
  - `src/workers/rpc/handler.rs:143-169` - blocking_send + blocking_recv

  **Acceptance Criteria**:
  - [ ] 每个请求只占用 1 个 blocking 线程
  - [ ] `cargo build` 通过
  - [ ] `cargo test` 全部通过
  - [ ] 现有功能测试不变

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build 2>&1
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-04-build.log

  Scenario: 功能测试
    Tool: Bash
    Steps:
      1. cargo test 2>&1
      2. Verify all tests pass
    Expected Result: "test result: ok"
    Evidence: .sisyphus/evidence/task-04-tests.log

  Scenario: 线程占用验证
    Tool: Bash
    Steps:
      1. grep -c "spawn_blocking" src/workers/rpc/*.rs
      2. Verify count reduced from 2 to 1
    Expected Result: spawn_blocking count = 1
    Evidence: .sisyphus/evidence/task-04-spawn-blocking.log
  ```

  **Commit**: YES
  - Message: `refactor(rpc): merge spawn_blocking calls to reduce thread usage`
  - Files: `src/workers/rpc/connection.rs`, `src/workers/rpc/request.rs`

- [x] 5. 第二阶段负载测试验证

  **What to do**:
  - 构建发布版本
  - 启动中间件
  - 运行负载测试：20 客户端，50ms 间隔，持续 5 分钟
  - 验证无请求失败，P95 延迟 < 100ms

  **Must NOT do**:
  - 不修改代码
  - 不跳过测试步骤

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要运行和验证系统
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (after Wave 1)
  - **Blocks**: Phase 3 tasks
  - **Blocked By**: Tasks 1, 2, 3, 4

  **References**:
  - `demo/jsonrpc_client.rs` - 测试客户端
  - `tests/async_rpc_tests.rs` - 性能测试示例

  **Acceptance Criteria**:
  - [ ] 发布版本构建成功
  - [ ] 负载测试完成
  - [ ] 无请求失败
  - [ ] P95 延迟 < 100ms

  **QA Scenarios**:
  ```
  Scenario: 构建验证
    Tool: Bash
    Steps:
      1. cargo build --release 2>&1
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-05-release-build.log

  Scenario: 负载测试
    Tool: Bash
    Preconditions: 中间件已启动
    Steps:
      1. cargo build --release
      2. 启动中间件（后台）
      3. 运行测试客户端 5 分钟
      4. 收集结果
    Expected Result: 0 failures, P95 < 100ms
    Evidence: .sisyphus/evidence/task-05-load-test.log
  ```

  **Commit**: NO

### 第三阶段：架构优化

- [x] 6. 实现 OperationQueue 并发队列

  **What to do**:
  - 在 `src/workers/modbus/types.rs` 中完善 `OperationQueue<T>` 实现
  - 添加公开 API：`new(max_in_flight: usize)`, `push()`, `pop_if_ready()`, `complete()`
  - 实现并发控制逻辑：限制同时执行的操作数
  - 添加单元测试

  **Must NOT do**:
  - 不修改现有的 ModbusWorker 消息处理逻辑（本任务只实现类型）
  - 不添加外部依赖
  - 不改变现有的 TimeoutHandler、Backoff 等类型

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要设计并发控制逻辑，涉及状态管理
  - **Skills**: [`test-driven-development`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 8)
  - **Blocks**: Task 7
  - **Blocked By**: Task 5

  **References**:
  - `src/workers/modbus/types.rs:117-161` - OperationQueue 现有定义
  - `src/workers/modbus/types.rs:232-289` - 现有测试代码
  - `src/config.rs:Device.max_concurrent_ops` - 配置参数

  **Acceptance Criteria**:
  - [ ] OperationQueue 有公开的构造函数和方法
  - [ ] `new(max_in_flight)` 创建指定容量的队列
  - [ ] `push()` 添加操作到队列
  - [ ] `pop_if_ready()` 在有容量时返回操作
  - [ ] `complete()` 释放容量
  - [ ] 单元测试覆盖所有方法
  - [ ] `cargo test operation_queue` 通过

  **QA Scenarios**:
  ```
  Scenario: 单元测试
    Tool: Bash
    Steps:
      1. cargo test operation_queue 2>&1
    Expected Result: All tests pass
    Evidence: .sisyphus/evidence/task-06-tests.log

  Scenario: 容量限制测试
    Tool: Bash
    Steps:
      1. cargo test operation_queue_limits 2>&1
    Expected Result: Test verifies max_in_flight is enforced
    Evidence: .sisyphus/evidence/task-06-capacity-test.log
  ```

  **Commit**: YES
  - Message: `feat(modbus): implement OperationQueue for concurrent operation control`
  - Files: `src/workers/modbus/types.rs`

- [x] 7. 集成 OperationQueue 到 ModbusWorker

  **What to do**:
  - 修改 `src/workers/modbus/handler.rs`
  - 在 `DeviceControlHandler` 中添加 `OperationQueue` 字段
  - 修改 `handle_device_control` 使用队列控制并发
  - 使用 `max_concurrent_ops` 配置参数
  - 当队列满时返回适当的错误响应

  **Must NOT do**:
  - 不改变 DeviceControl 消息格式
  - 不改变响应格式
  - 不移除现有的错误处理
  - 不修改 ModbusClient 的连接逻辑

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要修改核心处理逻辑，集成新组件
  - **Skills**: [`test-driven-development`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 6, 8)
  - **Blocks**: Task 8
  - **Blocked By**: Task 6

  **References**:
  - `src/workers/modbus/handler.rs:199-248` - handle_device_control 方法
  - `src/workers/modbus/worker.rs:50-144` - 消息处理循环
  - `src/config.rs:Device.max_concurrent_ops` - 配置参数

  **Acceptance Criteria**:
  - [ ] ModbusWorker 使用 OperationQueue 控制并发
  - [ ] 并发数受 max_concurrent_ops 配置限制
  - [ ] 队列满时返回错误响应（而非阻塞）
  - [ ] 现有测试全部通过
  - [ ] 新增并发控制测试通过

  **QA Scenarios**:
  ```
  Scenario: 集成测试
    Tool: Bash
    Steps:
      1. cargo test 2>&1
    Expected Result: All tests pass
    Evidence: .sisyphus/evidence/task-07-integration-tests.log

  Scenario: 并发限制测试
    Tool: Bash
    Steps:
      1. 设置 max_concurrent_ops = 2
      2. 发送 5 个并发请求
      3. 验证最多 2 个同时执行
    Expected Result: Exactly 2 concurrent, others queued or rejected
    Evidence: .sisyphus/evidence/task-07-concurrency-test.log
  ```

  **Commit**: YES
  - Message: `feat(modbus): integrate OperationQueue into ModbusWorker for concurrent operations`
  - Files: `src/workers/modbus/handler.rs`, `src/workers/modbus/worker.rs`

- [x] 8. 实现 Modbus 连接池

  **What to do**:
  - 在 `src/workers/modbus/client.rs` 中实现连接池
  - 设计 `ModbusConnectionPool` 类型
  - 支持从池中获取/归还连接
  - 实现连接健康检查和自动重连
  - 支持配置池大小

  **Must NOT do**:
  - 不改变 ModbusClient 的公开 API（保持向后兼容）
  - 不修改 Modbus TCP 协议实现
  - 不增加新的依赖

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要设计连接池机制，中等复杂度
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 6, 7)
  - **Blocks**: None (可选优化)
  - **Blocked By**: Task 5

  **References**:
  - `src/workers/modbus/client.rs:77-97` - ModbusClient 实现
  - `src/workers/modbus/client.rs:148-182` - execute_operation 方法
  - `src/config.rs:Device` - 设备配置

  **Acceptance Criteria**:
  - [ ] ModbusConnectionPool 类型实现
  - [ ] 支持配置池大小
  - [ ] 连接健康检查机制
  - [ ] 自动重连功能
  - [ ] 单元测试通过
  - [ ] 与 ModbusWorker 集成（可选）

  **QA Scenarios**:
  ```
  Scenario: 连接池测试
    Tool: Bash
    Steps:
      1. cargo test connection_pool 2>&1
    Expected Result: All tests pass
    Evidence: .sisyphus/evidence/task-08-pool-tests.log

  Scenario: 并发连接测试
    Tool: Bash
    Steps:
      1. 创建连接池 (size=3)
      2. 发送 5 个并发请求
      3. 验证使用池化连接
    Expected Result: Requests use pooled connections efficiently
    Evidence: .sisyphus/evidence/task-08-concurrent-test.log
  ```

  **Commit**: YES
  - Message: `feat(modbus): add connection pool for parallel device access`
  - Files: `src/workers/modbus/client.rs`

---

## Final Verification Wave (MANDATORY)

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files exist.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy` + `cargo test`. Review all changed files for: `as any`/`@ts-ignore`, empty catches, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [x] F3. **Load Test Verification** — `unspecified-high`
  Start middleware. Run load test with 20 clients at 50ms interval for 5 minutes. Verify no request failures. Save evidence to `.sisyphus/evidence/final-load-test/`.
  Output: `Requests [N total/N success] | Latency P95 [Xms] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **Phase 2**: `perf(rpc): increase concurrency capacity - blocking threads 32→128, channel 1000→5000` — workers/rpc/worker.rs
- **Phase 2**: `fix(rpc): handle Hub.send() errors` — workers/rpc/handler.rs
- **Phase 2**: `refactor(rpc): merge spawn_blocking calls to reduce thread usage` — workers/rpc/connection.rs, request.rs
- **Phase 3**: `feat(modbus): implement OperationQueue for concurrent operations` — workers/modbus/types.rs, handler.rs
- **Phase 3**: `feat(modbus): add connection pool for parallel device access` — workers/modbus/client.rs

---

## Success Criteria

### Verification Commands
```bash
# Phase 2 verification
cargo test 2>&1 | grep -E "(passed|failed)"
cargo clippy 2>&1 | grep -E "(warning|error)"

# Phase 3 verification
cargo test operation_queue 2>&1
cargo test connection_pool 2>&1

# Load test
cargo build --release
./target/release/roboplc-middleware &
# Run load test client
./target/release/jsonrpc_client --concurrent 20 --interval 50 --duration 300
```

### Final Checklist
- [x] All "Must Have" present
- [x] All "Must NOT Have" absent
- [x] All tests pass
- [x] Load test passes (20 clients × 50ms × 5min, no failures)
- [x] P95 latency < 100ms under load