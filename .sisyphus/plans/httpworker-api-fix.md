# HttpWorker 接口修复计划

## TL;DR

> **Quick Summary**: 修复 HttpWorker 中硬编码和未实现的 HTTP API 端点，使其与文档一致，并提供真实的功能实现。
>
> **Deliverables**:
> - 完全实现的 GET /api/health（全面健康检查）
> - 完全实现的 GET /api/config（返回实际配置）
> - 重命名的设备状态端点 `/api/devices/{id}` → `/api/devices/{id}/status`
> - POST /api/config/reload 返回成功（实际由文件 watcher 触发）
> - 删除过时的 src/api.rs 文件
>
> **Estimated Effort**: Medium
> **Parallel Execution**: NO - sequential tasks
> **Critical Path**: 所有任务顺序执行（每个任务依赖前一个完成）

---

## Context

### Original Request
HttpWorker 接口存在多个硬编码和未实现的问题，需要修复以匹配 README 文档中的 API 说明。

### Interview Summary
**Key Discussions**:
- **api.rs 处理**: 用户选择删除 src/api.rs，避免代码重复
- **配置重载机制**: 使用文件系统触发（ConfigLoader 监控），HTTP 端点只返回成功
- **健康检查范围**: 全面检查（系统健康、设备状态、延迟监控）
- **设备控制端点**: 不通过 HTTP 实现，只使用 JSON-RPC

**Research Findings**:
- `src/api.rs` 是过时模块，包含已实现的设备控制端点但未被使用
- `HttpWorker` 是 blocking worker，在独立线程中运行 tokio runtime
- `AppState` 只包含 `device_states`，缺少 `config` 和其他共享变量
- `RpcWorker` 使用 tokio mpsc channel 在异步线程和 Hub 之间传递消息

### Metis Review
**Identified Gaps** (addressed):
- AppState 需要扩展以包含更多共享变量
- 健康检查需要访问多个数据源
- 配置重载触发机制已明确（文件 watcher）

---

## Work Objectives

### Core Objective
修复 HttpWorker 的 HTTP API 端点，消除硬编码和未实现的问题，使功能与文档一致。

### Concrete Deliverables
- 完全实现的 `GET /api/health` 端点
- 完全实现的 `GET /api/config` 端点
- 重命名的 `GET /api/devices/{id}/status` 端点
- 修正的 `POST /api/config/reload` 端点（返回成功说明）
- 删除的 `src/api.rs` 文件
- 更新的文档（README.md）反映实际实现

### Definition of Done
- [ ] 运行 `cargo test --test http_worker` → PASS
- [ ] 运行 `cargo build` → SUCCESS
- [ ] 所有 HTTP API 端点返回真实数据（非硬编码）
- [ ] README.md 中的 API 文档与实际实现一致

### Must Have
- GET /api/health 返回真实健康状态
- GET /api/config 返回实际配置
- 设备状态端点路径为 `/api/devices/{id}/status`
- 删除 src/api.rs

### Must NOT Have (Guardrails)
- 不实现设备控制端点（set_register, batch, move）- 用户明确要求只使用 JSON-RPC
- 不通过 HTTP API 直接修改配置文件 - 保持文件 watcher 机制
- 不在 AppState 中添加不必要的数据 - 只添加需要的共享变量
- 不破坏现有的 JSON-RPC 功能 - 保持完全独立

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring "user manually tests/confirms" are FORBIDDEN.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: YES (Tests after implementation)
- **Framework**: tokio-test / cargo test
- **测试策略**: 先实现端点，然后添加测试

### QA Policy
Every task MUST include agent-executed QA scenarios (see TODO template below).
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **API 测试**: Use Bash (curl) — 发送 HTTP 请求，解析 JSON，验证字段和值
- **健康检查**: Use Bash (curl) — 验证健康报告包含所有必要字段
- **配置访问**: Use Bash (curl) — 验证返回配置包含实际设备列表

---

## Execution Strategy

### Sequential Execution

> Task 依赖关系：按顺序执行，每个任务完成后验证再进行下一个

```
Task 1: 扩展 AppState 结构
    ↓
Task 2: 实现 GET /api/health（全面健康检查）
    ↓
Task 3: 实现 GET /api/config（返回实际配置）
    ↓
Task 4: 重命名设备状态端点路径
    ↓
Task 5: 修正 POST /api/config/reload 文档说明
    ↓
Task 6: 删除 src/api.rs
    ↓
Task 7: 更新 README.md API 文档
    ↓
Task 8: 运行测试和构建验证
```

### Dependency Matrix

- **1**: — — 2-8, 1
- **2**: 1 — 3, 2
- **3**: 1, 2 — 4, 2
- **4**: 1 — 5, 2
- **5**: — — 6, 2
- **6**: — — 7, 2
- **7**: 1-6 — 8, 2
- **8**: 1-7 — — 3

---

## TODOs

- [ ] 1. **扩展 AppState 结构**

  **What to do**:
  - 修改 `src/workers/http_worker.rs` 中的 `AppState` 结构
  - 添加字段：
    - `config: Arc<Config>` - 当前配置的只读引用
    - `system_healthy: Arc<AtomicBool>` - 系统健康状态
    - `latency_samples: Arc<DataBuffer<LatencySample>>` - 延迟样本数据
  - 修改 `HttpWorker::run()` 方法，在创建 AppState 时初始化这些新字段

  **Must NOT do**:
  - 不要添加其他不需要的共享变量
  - 不要在 HTTP handlers 中直接修改共享状态（只读访问）

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `unspecified-low`
    - Reason: 简单的结构修改任务，无需复杂技能
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 2, Task 3, Task 4]
  - **Blocked By**: | None

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/http_worker.rs:43-50` - 当前 AppState 定义
  - `src/workers/http_worker.rs:202-216` - HttpWorker::run() 中创建 AppState 的代码
  - `src/lib.rs:116-123` - Variables 结构定义（查看有哪些共享变量可用）
  - `src/workers/rpc_worker.rs:319-334` - RpcWorker 如何克隆 Hub 和共享变量到异步线程

  **API/Type References** (contracts to implement against):
  - `src/config.rs:Config` - 配置结构体定义
  - `src/lib.rs:116` - `Variables` 共享状态结构
  - `src/lib.rs:107-112` - `LatencySample` 延迟样本定义

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `http_worker.rs:43-50`: 了解当前 AppState 结构，确定需要添加哪些字段
  - `http_worker.rs:202-216`: 了解 AppState 如何被创建和传递，确定如何初始化新字段
  - `lib.rs:116-123`: 查看 Variables 有哪些共享变量，确定 AppState 需要引用哪些
  - `rpc_worker.rs:319-334`: 参考 RpcWorker 如何克隆共享变量到异步线程的模式
  - `config.rs`: 了解 Config 结构，确保正确引用
  - `lib.rs:107-112`: 了解 LatencySample 定义，正确添加到 AppState

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] 文件 `src/workers/http_worker.rs` 已修改，AppState 包含新字段
  - [ ] `cargo build` → SUCCESS（编译通过）
  - [ ] `cargo clippy -- -D warnings` → No warnings

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  > **This is NOT optional. A task without QA scenarios WILL BE REJECTED.**
  >
  > Write scenario tests that verify the ACTUAL BEHAVIOR of what you built.
  > Minimum: 1 happy path + 1 failure/edge case per task.
  > Each scenario = exact tool + exact steps + exact assertions + evidence path.
  >
  > **The executing agent MUST run these scenarios after implementation.**
  > **The orchestrator WILL verify evidence files exist before marking task complete.**

  ```
  Scenario: AppState 包含所有必需字段
    Tool: Bash (cargo check)
    Preconditions: 代码已修改
    Steps:
      1. cargo check 2>&1 | head -20
    Expected Result: 编译成功，没有类型错误
    Failure Indicators: 编译错误、类型不匹配
    Evidence: .sisyphus/evidence/task-1-cargo-check.txt

  Scenario: AppState 字段类型正确
    Tool: Bash (grep)
    Preconditions: 代码已修改
    Steps:
      1. grep -A 10 "pub struct AppState" src/workers/http_worker.rs
    Expected Result: 输出包含 config: Arc<Config>, system_healthy: Arc<AtomicBool>, latency_samples: Arc<DataBuffer<LatencySample>>
    Failure Indicators: 字段缺失或类型不正确
    Evidence: .sisyphus/evidence/task-1-appstate-fields.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-1-cargo-check.txt` - cargo check 输出
  - [ ] `task-1-appstate-fields.txt` - AppState 结构定义

  **Commit**: YES
  - Message: `refactor(http_worker): extend AppState with config, health, and latency data`
  - Files: `src/workers/http_worker.rs`

---

- [ ] 2. **实现 GET /api/health（全面健康检查）**

  **What to do**:
  - 修改 `src/workers/http_worker.rs` 中的 `get_health()` 函数
  - 从 `AppState` 读取健康数据：
    - `system_healthy` - 系统健康状态
    - `device_states` - 遍历检查所有设备连接状态
    - `latency_samples` - 检查最近的延迟样本
  - 返回 JSON 响应：
    ```json
    {
      "status": "healthy" | "degraded" | "unhealthy",
      "system_healthy": true/false,
      "devices": {
        "total": N,
        "connected": M,
        "disconnected": K
      },
      "latency": {
        "latest_us": XXX,
        "avg_us": YYY
      }
    }
    ```
  - 健康状态判定逻辑：
    - `healthy`: 所有设备连接 + 系统健康
    - `degraded`: 部分设备断线或系统亚健康
    - `unhealthy`: 所有设备断线或系统不健康

  **Must NOT do**:
  - 不要硬编码任何健康值
  - 不要修改共享状态（只读访问）

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `unspecified-low`
    - Reason: 实现 HTTP handler，无需复杂技能
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 3]
  - **Blocked By**: [Task 1]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/http_worker.rs:134-136` - 当前 get_health() 硬编码实现
  - `src/workers/http_worker.rs:60-97` - get_devices() 如何读取 AppState 和构建 JSON 响应
  - `src/lib.rs:66-71` - DeviceStatus 结构定义

  **API/Type References** (contracts to implement against):
  - `src/lib.rs:116-123` - Variables 结构（了解有哪些健康指标可用）
  - `src/lib.rs:107-112` - LatencySample 定义

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `http_worker.rs:134-136`: 了解当前 get_health 实现，确定需要修改的位置
  - `http_worker.rs:60-97`: 参考 get_devices 的实现模式，学习如何读取 AppState 和构建 JSON
  - `lib.rs:66-71`: 了解 DeviceStatus 字段，确定如何判断设备健康状态
  - `lib.rs:116-123`: 了解 Variables 结构，确认有哪些健康指标可用
  - `lib.rs:107-112`: 了解 LatencySample 字段，确定如何计算延迟统计

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] `get_health()` 函数已修改，不再返回硬编码值
  - [ ] 响应 JSON 包含 status, system_healthy, devices, latency 字段
  - [ ] 运行 `cargo test --test http_worker` → PASS（所有测试通过）

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: GET /api/health 返回完整健康报告
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中，有设备配置
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s http://localhost:8081/api/health | jq '.'
      4. kill %1
    Expected Result: JSON 包含 status, system_healthy, devices (total, connected, disconnected), latency (latest_us, avg_us)
    Failure Indicators: 字段缺失、值不正确、HTTP 错误
    Evidence: .sisyphus/evidence/task-2-health-response.json

  Scenario: 健康状态正确反映设备和系统状态
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中，有设备连接
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. health=$(curl -s http://localhost:8081/api/health | jq -r '.status')
      4. kill %1
      5. echo "Health status: $health"
    Expected Result: status 为 "healthy", "degraded", 或 "unhealthy" 之一，反映实际状态
    Failure Indicators: status 为硬编码值、状态与实际情况不符
    Evidence: .sisyphus/evidence/task-2-health-status.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-2-health-response.json` - /api/health 响应
  - [ ] `task-2-health-status.txt` - 健康状态检查结果

  **Commit**: YES
  - Message: `feat(http_worker): implement comprehensive health check endpoint`
  - Files: `src/workers/http_worker.rs`

---

- [ ] 3. **实现 GET /api/config（返回实际配置）**

  **What to do**:
  - 修改 `src/workers/http_worker.rs` 中的 `get_config()` 函数
  - 从 `AppState.config` 读取当前配置
  - 序列化配置为 JSON 并返回
  - 返回格式：
    ```json
    {
      "config": {
        "server": { "rpc_port": 8080, "http_port": 8081 },
        "devices": [...],
        "logging": {...}
      }
    }
    ```

  **Must NOT do**:
  - 不要返回空对象
  - 不要修改配置（只读访问）

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `unspecified-low`
    - Reason: 实现 HTTP handler，简单任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 4]
  - **Blocked By**: [Task 1, Task 2]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/http_worker.rs:140-142` - 当前 get_config() 硬编码实现
  - `src/workers/http_worker.rs:60-97` - get_devices() 如何构建 JSON 响应

  **API/Type References** (contracts to implement against):
  - `src/config.rs:Config` - 配置结构定义

  **External References** (libraries and frameworks):
  - `serde_json` - 用于序列化 Config 为 JSON

  **WHY Each Reference Matters** (explain the relevance):
  - `http_worker.rs:140-142`: 了解当前 get_config 实现，确定需要修改的位置
  - `http_worker.rs:60-97`: 参考 get_devices 的实现模式，学习如何构建 JSON 响应
  - `config.rs`: 了解 Config 结构，确保正确序列化

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] `get_config()` 函数已修改，不再返回空对象
  - [ ] 返回的配置 JSON 包含实际设备列表和服务器配置
  - [ ] 运行 `cargo test --test http_worker` → PASS

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: GET /api/config 返回实际配置
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中，config.toml 存在
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s http://localhost:8081/api/config | jq '.config.devices | length'
      4. kill %1
    Expected Result: 设备数量大于 0（如果 config.toml 中有设备）
    Failure Indicators: 返回空对象、设备数量为 0（配置中有设备）、HTTP 错误
    Evidence: .sisyphus/evidence/task-3-config-devices.txt

  Scenario: 配置 JSON 包含服务器端口信息
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s http://localhost:8081/api/config | jq '.config.server.rpc_port, .config.server.http_port'
      4. kill %1
    Expected Result: 输出 rpc_port 和 http_port 的值（如 8080 和 8081）
    Failure Indicators: 字段缺失、值为 null
    Evidence: .sisyphus/evidence/task-3-config-ports.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-3-config-devices.txt` - 设备数量检查
  - [ ] `task-3-config-ports.txt` - 端口配置检查

  **Commit**: YES
  - Message: `feat(http_worker): implement /api/config endpoint returning actual configuration`
  - Files: `src/workers/http_worker.rs`

---

- [ ] 4. **重命名设备状态端点路径**

  **What to do**:
  - 修改 `src/workers/http_worker.rs` 中的路由配置 `configure_routes()` 函数
  - 将路由从 `/api/devices/{id}` 改为 `/api/devices/{id}/status`
  - 保持 handler 函数 `get_device_by_id()` 逻辑不变
  - 更新函数注释，反映新路径

  **Must NOT do**:
  - 不要修改 handler 函数逻辑
  - 不要更改其他路由

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 简单路由修改任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 5]
  - **Blocked By**: [Task 1, Task 3]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/http_worker.rs:153-170` - configure_routes() 路由配置
  - `src/workers/http_worker.rs:102-130` - get_device_by_id() handler

  **API/Type References** (contracts to implement against):
  - None

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `http_worker.rs:153-170`: 了解当前路由配置，确定需要修改的路由行
  - `http_worker.rs:102-130`: 了解 get_device_by_id 函数，确认不需要修改逻辑

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] 路由从 `/api/devices/{id}` 改为 `/api/devices/{id}/status`
  - [ ] 运行 `cargo test --test http_worker` → PASS
  - [ ] 旧路径 `/api/devices/{id}` 返回 404

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: GET /api/devices/{id}/status 返回设备状态
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中，有设备配置
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s http://localhost:8081/api/devices/plc-1/status | jq '.id, .connected'
      4. kill %1
    Expected Result: 返回设备 id 和 connected 状态
    Failure Indicators: 404 Not Found、JSON 解析错误
    Evidence: .sisyphus/evidence/task-4-new-path-response.json

  Scenario: 旧路径 /api/devices/{id} 返回 404
    Tool: Bash (curl)
    Preconditions: 系统运行中
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s -w "\n%{http_code}\n" http://localhost:8081/api/devices/plc-1
      4. kill %1
    Expected Result: HTTP 404 响应码
    Failure Indicators: 返回 200（旧路径仍然工作）
    Evidence: .sisyphus/evidence/task-4-old-path-404.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-4-new-path-response.json` - 新路径响应
  - [ ] `task-4-old-path-404.txt` - 旧路径 404 验证

  **Commit**: YES
  - Message: `refactor(http_worker): rename device status endpoint to /api/devices/{id}/status`
  - Files: `src/workers/http_worker.rs`

---

- [ ] 5. **修正 POST /api/config/reload 文档说明**

  **What to do**:
  - 修改 `src/workers/http_worker.rs` 中的 `reload_config()` 函数
  - 更新注释，明确说明：
    - 实际配置重载由 ConfigLoader 的文件 watcher 触发
    - 此端点仅返回成功，用于外部系统确认 API 可用
    - 如需触发重载，应修改 config.toml 文件
  - 保持返回值不变：`{"reload": "ok"}`

  **Must NOT do**:
  - 不要改变函数返回值
  - 不要实现实际的配置重载逻辑

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 仅修改注释，简单任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 6]
  - **Blocked By**: []

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/workers/http_worker.rs:145-149` - reload_config() 当前实现

  **API/Type References** (contracts to implement against):
  - None

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `http_worker.rs:145-149`: 了解当前 reload_config 实现，确定需要修改的注释

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] 函数注释已更新，明确说明配置重载机制
  - [ ] 返回值保持不变：`{"reload": "ok"}`
  - [ ] 运行 `cargo test --test http_worker` → PASS

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: POST /api/config/reload 返回成功响应
    Tool: Bash (curl + jq)
    Preconditions: 系统运行中
    Steps:
      1. cargo run --bin roboplc-middleware &
      2. sleep 3
      3. curl -s -X POST http://localhost:8081/api/config/reload | jq '.'
      4. kill %1
    Expected Result: 返回 {"reload": "ok"}
    Failure Indicators: 返回值改变、HTTP 错误
    Evidence: .sisyphus/evidence/task-5-reload-response.json

  Scenario: 函数注释说明配置重载机制
    Tool: Bash (grep)
    Preconditions: 代码已修改
    Steps:
      1. grep -A 15 "async fn reload_config" src/workers/http_worker.rs | head -20
    Expected Result: 注释包含 "ConfigLoader 的文件 watcher 触发" 或类似说明
    Failure Indicators: 注释未更新
    Evidence: .sisyphus/evidence/task-5-reload-comments.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-5-reload-response.json` - reload 响应
  - [ ] `task-5-reload-comments.txt` - 函数注释

  **Commit**: YES
  - Message: `docs(http_worker): clarify /api/config/reload endpoint behavior`
  - Files: `src/workers/http_worker.rs`

---

- [ ] 6. **删除 src/api.rs 文件**

  **What to do**:
  - 删除 `src/api.rs` 文件
  - 从 `src/lib.rs` 中移除 `pub mod api;` 声明

  **Must NOT do**:
  - 不要删除任何测试文件
  - 不要影响其他模块的编译

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: 删除文件和模块声明，简单任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 7]
  - **Blocked By**: [Task 5]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `src/lib.rs:47` - pub mod api; 声明位置

  **API/Type References** (contracts to implement against):
  - None

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `lib.rs:47`: 了解 api 模块声明的位置，确定需要删除的行

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] `src/api.rs` 文件已删除
  - [ ] `src/lib.rs` 中的 `pub mod api;` 已删除
  - [ ] 运行 `cargo build` → SUCCESS
  - [ ] 运行 `cargo test` → PASS

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: src/api.rs 文件已删除
    Tool: Bash (ls)
    Preconditions: 代码已修改
    Steps:
      1. ls src/api.rs 2>&1
    Expected Result: No such file or directory 错误
    Failure Indicators: 文件仍然存在
    Evidence: .sisyphus/evidence/task-6-api-deleted.txt

  Scenario: lib.rs 不再导出 api 模块
    Tool: Bash (grep)
    Preconditions: 代码已修改
    Steps:
      1. grep "pub mod api" src/lib.rs 2>&1
    Expected Result: 无输出（grep 未找到匹配）
    Failure Indicators: 找到 pub mod api; 声明
    Evidence: .sisyphus/evidence/task-6-lib-rs-no-api.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-6-api-deleted.txt` - api.rs 删除验证
  - [ ] `task-6-lib-rs-no-api.txt` - lib.rs 无 api 模块验证

  **Commit**: YES
  - Message: `refactor: remove obsolete src/api.rs module`
  - Files: `src/api.rs`, `src/lib.rs`

---

- [ ] 7. **更新 README.md API 文档**

  **What to do**:
  - 修改 `README.md` 中的 HTTP API 端点表格
  - 更新 `/api/devices/{id}/status` 路径（从 `/api/devices/{id}` 改为 `/api/devices/{id}/status`）
  - 移除设备控制端点（set_register, batch, move），这些只通过 JSON-RPC 提供
  - 更新端点描述，反映实际实现：
    - `/api/health`: Comprehensive health check
    - `/api/config`: Current configuration (returns actual config)
    - `/api/config/reload`: Trigger config reload via file watcher

  **Must NOT do**:
  - 不要修改 JSON-RPC API 文档
  - 不要添加未实现的端点

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `unspecified-low`
    - Reason: 更新文档，简单任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: [Task 8]
  - **Blocked By**: [Task 1, Task 2, Task 3, Task 4, Task 5, Task 6]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `README.md:128-139` - HTTP API 端点表格

  **API/Type References** (contracts to implement against):
  - None

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `README.md:128-139`: 了解当前 API 文档，确定需要修改的内容

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] README.md 中的 HTTP API 表格已更新
  - [ ] 设备状态端点路径为 `/api/devices/{id}/status`
  - [ ] 移除了 set_register, batch, move 端点
  - [ ] 端点描述与实际实现一致

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: README.md 不再包含设备控制端点
    Tool: Bash (grep)
    Preconditions: README.md 已更新
    Steps:
      1. grep "set_register\|batch\|move" README.md | grep -v "JSON-RPC"
    Expected Result: HTTP API 部分无输出（不包含这些端点）
    Failure Indicators: HTTP API 部分仍包含设备控制端点
    Evidence: .sisyphus/evidence/task-7-no-device-control.txt

  Scenario: 设备状态端点路径正确
    Tool: Bash (grep)
    Preconditions: README.md 已更新
    Steps:
      1. grep "/api/devices/.*status" README.md
    Expected Result: 包含 `/api/devices/{id}/status` 路径
    Failure Indicators: 仍使用 `/api/devices/{id}` 路径
    Evidence: .sisyphus/evidence/task-7-status-path.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-7-no-device-control.txt` - 无设备控制端点验证
  - [ ] `task-7-status-path.txt` - 状态路径验证

  **Commit**: YES
  - Message: `docs: update HTTP API documentation in README`
  - Files: `README.md`

---

- [ ] 8. **运行测试和构建验证**

  **What to do**:
  - 运行所有测试确保功能正确
  - 运行构建确保没有编译错误
  - 运行 clippy 检查代码质量

  **Must NOT do**:
  - 不要跳过任何测试
  - 不要忽略 clippy 警告

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `unspecified-low`
    - Reason: 运行测试和构建，验证任务
  - **Skills**: `[]`
    - 不需要特殊技能
  - **Skills Evaluated but Omitted**:
    - 无

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Final
  - **Blocks**: []
  - **Blocked By**: [Task 1, Task 2, Task 3, Task 4, Task 5, Task 6, Task 7]

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - `AGENTS.md` - Build/Test Commands 部分

  **API/Type References** (contracts to implement against):
  - None

  **External References** (libraries and frameworks):
  - None

  **WHY Each Reference Matters** (explain the relevance):
  - `AGENTS.md`: 了解项目的测试和构建命令

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  - [ ] `cargo test` → PASS（所有测试通过）
  - [ ] `cargo build` → SUCCESS（编译成功）
  - [ ] `cargo clippy -- -D warnings` → No warnings

  **QA Scenarios (MANDATORY — task is INCOMPLETE without these):**

  ```
  Scenario: 所有测试通过
    Tool: Bash (cargo test)
    Preconditions: 所有代码修改完成
    Steps:
      1. cargo test 2>&1 | tail -20
    Expected Result: test result: ok. X passed; Y failed; Z ignored
    Failure Indicators: 测试失败
    Evidence: .sisyphus/evidence/task-8-test-results.txt

  Scenario: 编译成功且无警告
    Tool: Bash (cargo build + cargo clippy)
    Preconditions: 所有代码修改完成
    Steps:
      1. cargo build 2>&1 | tail -10
      2. cargo clippy -- -D warnings 2>&1 | tail -10
    Expected Result: Compiling roboplc-middleware v... Finished, 无 warning
    Failure Indicators: 编译错误、clippy 警告
    Evidence: .sisyphus/evidence/task-8-build-clippy.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-8-test-results.txt` - 测试结果
  - [ ] `task-8-build-clippy.txt` - 构建和 clippy 结果

  **Commit**: NO
  - 此任务不创建 commit，只是验证

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 运行全面的验证，确保所有修改正确且功能正常。

- [ ] F1. **API 端点功能验证** — `unspecified-low`
  启动服务并测试所有 HTTP API 端点，确保返回正确的数据和响应格式。
  - GET /api/health - 返回完整健康报告
  - GET /api/config - 返回实际配置
  - GET /api/devices - 返回设备列表
  - GET /api/devices/{id}/status - 返回设备状态
  - POST /api/config/reload - 返回成功响应
  验证旧路径 /api/devices/{id} 返回 404
  验证设备控制端点不存在
  输出: `Endpoints [5/5 PASS] | Old paths [0 accessible] | Device control [0 accessible] | VERDICT`

- [ ] F2. **代码质量检查** — `unspecified-low`
  运行 `cargo test`, `cargo clippy`, `cargo fmt --check`。检查代码风格、编译警告、测试覆盖率。
  输出: `Tests [PASS/FAIL] | Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | VERDICT`

- [ ] F3. **文档一致性检查** — `deep`
  对比 README.md 中的 API 文档与实际实现，确保文档准确反映端点路径和功能。
  输出: `Documentation [N/N accurate] | Outdated [0] | Missing [0] | VERDICT`

- [ ] F4. **删除验证检查** — `unspecified-low`
  确认 src/api.rs 已删除，lib.rs 不再导出 api 模块，无编译错误。
  输出: `Files [deleted] | Exports [removed] | Build [SUCCESS] | VERDICT`

---

## Commit Strategy

- **1**: `refactor(http_worker): extend AppState with config, health, and latency data` — src/workers/http_worker.rs
- **2**: `feat(http_worker): implement comprehensive health check endpoint` — src/workers/http_worker.rs
- **3**: `feat(http_worker): implement /api/config endpoint returning actual configuration` — src/workers/http_worker.rs
- **4**: `refactor(http_worker): rename device status endpoint to /api/devices/{id}/status` — src/workers/http_worker.rs
- **5**: `docs(http_worker): clarify /api/config/reload endpoint behavior` — src/workers/http_worker.rs
- **6**: `refactor: remove obsolete src/api.rs module` — src/api.rs, src/lib.rs
- **7**: `docs: update HTTP API documentation in README` — README.md

---

## Success Criteria

### Verification Commands
```bash
# 健康检查端点
curl -s http://localhost:8081/api/health | jq '.'

# 配置端点
curl -s http://localhost:8081/api/config | jq '.config.devices | length'

# 设备状态端点（新路径）
curl -s http://localhost:8081/api/devices/plc-1/status | jq '.id, .connected'

# 旧路径应返回 404
curl -s -w "\n%{http_code}\n" http://localhost:8081/api/devices/plc-1

# 配置重载端点
curl -s -X POST http://localhost:8081/api/config/reload | jq '.'

# 运行测试
cargo test

# 构建和 clippy
cargo build
cargo clippy -- -D warnings
```

### Final Checklist
- [ ] GET /api/health 返回真实健康报告（包含设备状态、延迟信息）
- [ ] GET /api/config 返回实际配置（非空对象）
- [ ] GET /api/devices/{id}/status 路径正确工作
- [ ] GET /api/devices/{id} 旧路径返回 404
- [ ] POST /api/config/reload 返回成功说明（文档已更新）
- [ ] src/api.rs 已删除
- [ ] README.md API 文档与实现一致
- [ ] 所有测试通过
- [ ] 无 clippy 警告
