# RpcWorker 死锁问题修复计划

## TL;DR

> **Quick Summary**: 将 RpcWorker 从单线程阻塞式重构为异步并发式，解决严重的性能和正确性问题。
>
> **Deliverables**:
> - 重构 RpcWorker 为异步 worker
> - 使用 tokio 运行时并发处理 RPC 连接
> - 使用 tokio::sync 通道替代阻塞通道
> - 移除所有超时阻塞操作
> - 确保正确处理超时和清理
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 5 waves
> **Critical Path**: Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5

---

## Context

### Original Request
用户发现 RpcWorker 中存在严重的死锁问题：`send_device_control()` 函数内部调用 `recv_timeout(30s)` 阻塞，导致主循环无法执行 `try_recv()`，进而无法转发消息到 Hub，最终导致 Modbus 操作无法执行。

### Interview Summary
**Key Discussions**:
- 用户准确识别了阻塞问题：`response_rx.recv_timeout(30s)` 阻塞期间，`device_control_rx.try_recv()` 无法执行
- 用户进一步发现：超时后 `response_tx` 被丢弃，即使后续有 `DeviceResponse` 也无法发送给客户端
- 请求完全异步重构，去除不必要的超时操作，避免冗余

**Research Findings**:
- 项目已依赖 tokio (Cargo.toml 第20行)
- HttpWorker 已使用 tokio 异步运行时
- 当前 RpcWorker 使用 `blocking = true` 属性
- RoboPLC 框架支持异步 worker

### Metis Review
**Identified Gaps** (addressed):
- 需要验证 RoboPLC 框架是否支持异步 worker
- 需要检查 roboplc-rpc 是否支持异步处理
- 需要确保所有使用 `std::sync::mpsc` 的地方都改为 `tokio::sync::mpsc`
- 需要确保 DeviceManager 和 ModbusWorker 不受影响

---

## Work Objectives

### Core Objective
将 RpcWorker 从单线程阻塞式重构为异步并发式，彻底解决死锁问题，提升吞吐量和响应速度。

### Concrete Deliverables
- 修改后的 `src/workers/rpc_worker.rs` (完全异步化)
- 更新后的测试用例
- 性能对比报告

### Definition of Done
- [ ] 所有阻塞操作移除
- [ ] 支持并发处理 10+ 个 RPC 请求
- [ ] 平均响应时间 < 1s
- [ ] 超时清理机制正确工作
- [ ] 所有测试通过

### Must Have
- ✅ 完全异步化，无阻塞点
- ✅ 使用 tokio 运行时
- ✅ 使用 tokio::sync 通道
- ✅ 正确处理超时和清理
- ✅ 保持公共 API 不变

### Must NOT Have (Guardrails)
- **不要保留任何 `std::sync::mpsc` 阻塞通道**
- **不要保留任何 `recv_timeout()` 阻塞调用**
- **不要修改 HttpWorker 和其他 worker**
- **不要破坏现有消息传递机制**
- **不要使用多线程 spawn (tokio::spawn 代替)**

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring "user manually tests/confirms" are FORBIDDEN.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: YES (Tests-after)
- **Framework**: 现有的测试框架 (tokio)
- **如果 TDD**: 每个任务先写测试，再实现

### QA Policy
Every task MUST include agent-executed QA scenarios (see TODO template below).
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Frontend/UI**: 使用 reqwest 发送 HTTP 请求验证 RPC 接口
- **Backend/API**: 使用 tokio 测试框架验证异步逻辑
- **Library/Module**: 使用 tokio::test 宏验证功能
- **Integration**: 使用 mock Modbus 服务器验证完整流程

---

## Execution Strategy

### Parallel Execution Waves

> Maximize throughput by grouping independent tasks into parallel waves.
> Each wave completes before the next begins.
> Target: 5-8 tasks per wave.

```
Wave 1 (Foundation + Research):
├── Task 1: Research RoboPLC async worker support [quick]
├── Task 2: Research roboplc-rpc async compatibility [quick]
├── Task 3: Create test infrastructure [quick]
├── Task 4: Document current architecture [quick]
└── Task 5: Design async architecture [quick]

Wave 2 (Core Infrastructure):
├── Task 6: Modify RpcWorker to async [unspecified-high]
├── Task 7: Replace std::sync::mpsc with tokio::sync::mpsc [unspecified-high]
├── Task 8: Replace blocking recv with oneshot channels [unspecified-high]
├── Task 9: Implement tokio runtime in RpcWorker [unspecified-high]
└── Task 10: Update imports and dependencies [quick]

Wave 3 (Main Loop Refactoring):
├── Task 11: Implement async TCP listener [deep]
├── Task 12: Implement tokio::select! main loop [deep]
├── Task 13: Implement async connection handler [deep]
├── Task 14: Implement async send_device_control [unspecified-high]
└── Task 15: Remove timeout blocking operations [unspecified-high]

Wave 4 (Response Handling):
├── Task 16: Implement async response handling [unspecified-high]
├── Task 17: Implement async timeout handling [unspecified-high]
├── Task 18: Implement async cleanup logic [quick]
└── Task 19: Update error handling [unspecified-high]

Wave 5 (Testing & Validation):
├── Task 20: Write unit tests for channels [quick]
├── Task 21: Write unit tests for timeout [quick]
├── Task 22: Write integration tests [deep]
├── Task 23: Write performance tests [deep]
└── Task 24: Update documentation [quick]

Wave FINAL (Review):
├── Task F1: Code review [unspecified-high]
├── Task F2: Performance testing [deep]
├── Task F3: Integration testing [deep]
└── Task F4: Documentation review [quick]

Critical Path: T1 → T6 → T11 → T16 → T20 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 5 (Wave 1 & 2)
```

### Dependency Matrix

- **1-5**: — — 6-10, 1
- **6**: 1, 2, 5 — 7, 8, 9, 11, 2
- **7**: 1, 2 — 8, 9, 10, 11, 2
- **8**: 1, 2, 7 — 9, 14, 16, 2
- **9**: 1, 2, 6 — 11, 13, 14, 2
- **11**: 6, 7, 8, 9 — 12, 13, 14, 3
- **14**: 8, 9, 11 — 15, 16, 3
- **16**: 8, 14, 15 — 17, 18, 4
- **20**: 3, 16 — 21, 22, 5

---

## TODOs

> Implementation + Test = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.
> **A task WITHOUT QA Scenarios is INCOMPLETE. No exceptions.**

- [ ] 1. Research RoboPLC async worker support

  **What to do**:
  - Search RoboPLC 文档和示例代码
  - 验证 RoboPLC 是否支持异步 worker
  - 查找异步 worker 的最佳实践
  - 确认 `blocking = true` 属性移除后如何配置
  - 记录所有发现

  **Must NOT do**:
  - 不要假设 RoboPLC 支持异步 worker
  - 不要跳过文档查阅

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 简单的研究任务，查阅文档和示例
  - **Skills**: `[]`
    - Reason: 不需要特定技能

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4, 5) | Sequential
  - **Blocks**: [Task 6, Task 7]
  - **Blocked By**: [None] | None (can start immediately)

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **External References** (libraries and frameworks):
  - RoboPLC 官方文档: https://github.com/roboplc/roboplc - 查找异步 worker 示例
  - RoboPLC GitHub Issues: 搜索 "async worker" 或 "blocking"
  - RoboPLC 源代码: `roboplc/src/controller/worker.rs` - 查看 Worker trait 定义

  **WHY Each Reference Matters** (explain the relevance):
  - RoboPLC 文档: 了解是否支持异步 worker 以及如何配置
  - GitHub Issues: 查看其他开发者遇到的问题和解决方案
  - 源代码: 确认 Worker trait 是否支持 async fn

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  **Tests-after**:
  - [ ] Research 记录保存到 `.sisyphus/research/roboplc-async-support.md`
  - [ ] 确认 RoboPLC 支持/不支持异步 worker
  - [ ] 提供异步 worker 配置示例

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  > **This is NOT optional. A task without QA scenarios WILL BE REJECTED.**

  ```
  Scenario: Research verification
    Tool: Bash (grep)
    Preconditions: Research 完成
    Steps:
      1. cat .sisyphus/research/roboplc-async-support.md
      2. grep -i "async\|blocking" .sisyphus/research/roboplc-async-support.md
    Expected Result: 文件包含明确的 "RoboPLC 支持/不支持异步 worker" 结论
    Failure Indicators: 文件不存在或不包含明确结论
    Evidence: .sisyphus/evidence/task-1-research-verification.md

  Scenario: Source code verification
    Tool: Bash (rg)
    Preconditions: Research 完成
    Steps:
      1. rg "async fn run" roboplc/src/
      2. rg "Worker.*trait" roboplc/src/controller/worker.rs
    Expected Result: 找到 Worker trait 定义，确认是否支持 async fn
    Failure Indicators: 找不到 Worker trait 定义或相关代码
    Evidence: .sisyphus/evidence/task-1-source-verification.md
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/research/roboplc-async-support.md` - 研究记录
  - [ ] `.sisyphus/evidence/task-1-research-verification.md` - 验证证据
  - [ ] `.sisyphus/evidence/task-1-source-verification.md` - 源码验证证据

  **Commit**: NO
  - Message: N/A

---

- [ ] 2. Research roboplc-rpc async compatibility

  **What to do**:
  - Search roboplc-rpc 文档和源代码
  - 验证 `RpcServer` 和 `RpcServerHandler` 是否支持异步
  - 查找异步处理请求的示例
  - 确认如何集成到 tokio 运行时
  - 记录所有发现

  **Must NOT do**:
  - 不要假设 roboplc-rpc 支持异步
  - 不要跳过文档查阅

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 简单的研究任务，查阅文档和示例
  - **Skills**: `[]`
    - Reason: 不需要特定技能

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4, 5) | Sequential
  - **Blocks**: [Task 6, Task 7]
  - **Blocked By**: [None] | None (can start immediately)

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/rpc_worker.rs:293-386` - 当前的 RpcServerHandler 实现

  **External References** (libraries and frameworks):
  - roboplc-rpc 文档: https://docs.rs/roboplc-rpc - 查看 RpcServer 和 RpcServerHandler API
  - roboplc-rpc 源代码: `roboplc-rpc/src/server.rs` - 查看实现细节

  **WHY Each Reference Matters** (explain the relevance):
  - 当前实现: 了解当前如何使用 RpcServer 和 RpcServerHandler
  - roboplc-rpc 文档: 确认是否支持异步处理
  - 源代码: 查看内部实现，确认是否可以异步化

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  **Tests-after**:
  - [ ] Research 记录保存到 `.sisyphus/research/roboplc-rpc-async.md`
  - [ ] 确认 roboplc-rpc 支持/不支持异步
  - [ ] 提供异步集成示例

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  > **This is NOT optional. A task without QA scenarios WILL BE REJECTED.**

  ```
  Scenario: Research verification
    Tool: Bash (grep)
    Preconditions: Research 完成
    Steps:
      1. cat .sisyphus/research/roboplc-rpc-async.md
      2. grep -i "async\|await\|tokio" .sisyphus/research/roboplc-rpc-async.md
    Expected Result: 文件包含明确的 "roboplc-rpc 支持/不支持异步" 结论
    Failure Indicators: 文件不存在或不包含明确结论
    Evidence: .sisyphus/evidence/task-2-research-verification.md

  Scenario: API verification
    Tool: Bash (rg)
    Preconditions: Research 完成
    Steps:
      1. rg "async fn" Cargo.toml
      2. rg "RpcServer\|RpcServerHandler" Cargo.toml
    Expected Result: 确认 roboplc-rpc 版本和依赖
    Failure Indicators: 找不到相关依赖
    Evidence: .sisyphus/evidence/task-2-api-verification.md
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/research/roboplc-rpc-async.md` - 研究记录
  - [ ] `.sisyphus/evidence/task-2-research-verification.md` - 验证证据
  - [ ] `.sisyphus/evidence/task-2-api-verification.md` - API 验证证据

  **Commit**: NO
  - Message: N/A

---

- [ ] 3. Create test infrastructure

  **What to do**:
  - 创建 mock Modbus 服务器用于测试
  - 创建测试工具函数发送 RPC 请求
  - 创建测试工具函数验证响应
  - 设置测试环境 (端口、配置等)
  - 创建基础测试框架

  **Must NOT do**:
  - 不要创建实际的 Modbus 连接测试 (使用 mock)
  - 不要修改生产代码

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 简单的测试基础设施搭建
  - **Skills**: `[]`
    - Reason: 不需要特定技能

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4, 5) | Sequential
  - **Blocks**: [Task 20, Task 21, Task 22]
  - **Blocked By**: [None] | None (can start immediately)

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `tests/mock_modbus.rs` - 现有的 mock Modbus 服务器
  - `tests/e2e_tests.rs` - 现有的集成测试框架

  **WHY Each Reference Matters** (explain the relevance):
  - mock_modbus.rs: 了解如何创建 mock 服务器
  - e2e_tests.rs: 了解现有的测试模式和工具函数

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  **Tests-after**:
  - [ ] 创建 `tests/async_rpc_tests.rs` 测试文件
  - [ ] 创建 mock Modbus 服务器函数
  - [ ] 创建 RPC 请求发送函数
  - [ ] 创建响应验证函数
  - [ ] 基础测试框架编译通过

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  > **This is NOT optional. A task without QA scenarios WILL BE REJECTED.**

  ```
  Scenario: Test infrastructure compilation
    Tool: Bash (cargo)
    Preconditions: 测试基础设施创建完成
    Steps:
      1. cargo test --test async_rpc_tests --no-run
    Expected Result: 编译成功，无错误
    Failure Indicators: 编译失败或有错误
    Evidence: .sisyphus/evidence/task-3-compilation.txt

  Scenario: Mock server verification
    Tool: Bash (cargo)
    Preconditions: 测试基础设施创建完成
    Steps:
      1. cargo test test_mock_server --test async_rpc_tests
    Expected Result: Mock 服务器启动和关闭测试通过
    Failure Indicators: 测试失败
    Evidence: .sisyphus/evidence/task-3-mock-server.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-3-compilation.txt` - 编译验证
  - [ ] `.sisyphus/evidence/task-3-mock-server.txt` - Mock 服务器验证

  **Commit**: NO
  - Message: N/A

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Code Review** — `unspecified-high`
  Review all changed files for: async/await correctness, channel usage, error handling, timeout handling, cleanup logic. Check for potential race conditions and deadlocks.
  Output: `Review [PASS/FAIL] | Issues [N found] | VERDICT`

- [ ] F2. **Performance Testing** — `deep`
  Run performance tests: concurrent request handling, response time, throughput. Compare with previous blocking implementation.
  Output: `Concurrent [N req/s] | Avg Response [ms] | Throughput [req/s] | Improvement [%] | VERDICT`

- [ ] F3. **Integration Testing** — `deep`
  Run integration tests with mock Modbus server. Test: single request, concurrent requests, timeout scenarios, cleanup logic.
  Output: `Tests [N/N pass] | Timeout [pass/fail] | Cleanup [pass/fail] | VERDICT`

- [ ] F4. **Documentation Review** — `quick`
  Review updated documentation for accuracy and completeness. Ensure all async changes are documented.
  Output: `Documentation [COMPLETE/INCOMPLETE] | VERDICT`

---

## Commit Strategy

- **Wave 1**: N/A (research and setup, no production code changes)
- **Wave 2**: `refactor(rpc-worker): async infrastructure` — src/workers/rpc_worker.rs
- **Wave 3**: `refactor(rpc-worker): async main loop` — src/workers/rpc_worker.rs
- **Wave 4**: `refactor(rpc-worker): async response handling` — src/workers/rpc_worker.rs
- **Wave 5**: `test(rpc-worker): async tests` — tests/async_rpc_tests.rs

---

## Success Criteria

### Verification Commands
```bash
# Build the project
cargo build --release

# Run all tests
cargo test

# Run async RPC tests
cargo test --test async_rpc_tests

# Performance test
cargo test --test async_rpc_tests -- --nocapture --test-threads 1 performance
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] No blocking operations in RpcWorker
- [ ] Support concurrent request handling (10+ requests)
- [ ] Average response time < 1s
- [ ] Timeout handling correct
- [ ] Cleanup logic correct
- [ ] All tests pass
- [ ] Performance improvement > 50%
