# Modbus 代码清理与重构计划

## TL;DR

> **快速总结**: 移除 MoveTo 死代码，重构 Modbus 写入方法为统一泛型接口
> 
> **交付物**:
> - 移除 Operation::MoveTo 和 RpcMethod::MoveTo
> - 创建统一的 write_registers 泛型方法
> - 所有测试通过，行为不变
> 
> **预估工作量**: Medium
> **并行执行**: YES - 2 waves
> **关键路径**: Task 1 → Task 3 → Task 5

---

## Context

### 原始请求
用户提出三个问题：
1. MoveTo 信号是否可以移除
2. 写寄存器代码是否冗余，是否可以合成一个函数
3. RPC 命令的编解码流程

### 讨论总结
**关键决策**:
- MoveTo: 确认为死代码，可以安全移除
- 写入重构: 使用枚举包装类型 `WriteValue` 统一 Coil/Holding
- FC 选择: 根据 values.len() 自动选择 FC05/06 或 FC15/16
- 测试策略: 运行现有测试验证重构

**研究发现**:
- MoveTo 在 ModbusWorker 中返回 "Operation not supported"，从未真正实现
- 当前 4 个写入方法服务于不同的 Modbus 功能码
- RoboPLC 的 ModbusMapping::write 已支持多种类型

### Metis Review
**识别的差距** (已解决):
- 类型差异处理: 使用 `enum WriteValue { Coil(bool), Holding(u16) }`
- FC 自动选择: len() == 1 用单写，len() > 1 用批写
- 空值处理: 返回错误

---

## Work Objectives

### 核心目标
移除死代码并简化写入接口，同时保持现有行为不变

### 具体交付物
- 移除 `Operation::MoveTo` 枚举变体
- 移除 `RpcMethod::MoveTo` 及处理代码
- 移除 ModbusWorker 中的 MoveTo 错误处理
- 创建 `WriteValue` 枚举类型
- 创建统一的 `write_registers` 方法
- 所有测试通过

### 完成定义
- [ ] `cargo test` 通过
- [ ] `cargo clippy` 无警告
- [ ] MoveTo 相关代码完全移除
- [ ] 写入方法重构完成且行为不变

### 必须有
- 保持现有 RPC API 不变
- 保持 Modbus FC 行为正确
- 保持测试覆盖率

### 必须没有 (Guardrails)
- 不要修改 HTTP API 端点
- 不要添加新的 RPC 方法
- 不要改变 ModbusOp 的公共签名
- 不要在重构中添加新功能

---

## Verification Strategy (MANDATORY)

> **零人工干预** — 所有验证由 agent 执行。无例外。

### 测试决策
- **基础设施存在**: YES (cargo test)
- **自动化测试**: Tests-after (重构后运行现有测试)
- **框架**: cargo test

### QA 策略
每个任务必须包含 agent 执行的 QA 场景。
证据保存到 `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`。

---

## Execution Strategy

### 并行执行波次

```
Wave 1 (立即开始 — 独立任务):
├── Task 1: 移除 MoveTo 信号 [quick]
└── Task 2: 创建 WriteValue 枚举类型 [quick]

Wave 2 (Wave 1 完成后):
├── Task 3: 重构写入方法为统一接口 [unspecified-high]
└── Task 4: 更新 ModbusOp 使用新的写入方法 [unspecified-high]

Wave FINAL (所有任务完成后):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Test verification (unspecified-high)
└── Task F4: Scope fidelity check (deep)

关键路径: Task 1 → Task 3 → Task F1-F4
并行加速: ~40% 快于顺序执行
最大并发: 2 (Wave 1)
```

### 依赖矩阵
- **1**: — — 3, F1-F4
- **2**: — — 3, F1-F4
- **3**: 1, 2 — 4, F1-F4
- **4**: 3 — F1-F4
- **F1-F4**: 1, 2, 3, 4 — —

### Agent 分发摘要
- **Wave 1**: **2** — T1 → `quick`, T2 → `quick`
- **Wave 2**: **2** — T3 → `unspecified-high`, T4 → `unspecified-high`
- **FINAL**: **4** — F1 → `oracle`, F2-F3 → `unspecified-high`, F4 → `deep`

---

## TODOs
- [x] 1. **移除 MoveTo 信号**

  **What to do**:
  - 从 `src/messages.rs` 移除 `Operation::MoveTo` 枚举变体 (line 152)
  - 移除 `test_operation_move_to_serialization` 测试 (lines 201-208)
  - 移除 `test_device_control_clone_with_move_to` 测试 (lines 256-271)

  **Must NOT do**:
  - 不要修改其他 Operation 变体
  - 不要修改消息传递机制

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 2)
  - **Blocks**: Task 3, F1-F4
  - **Blocked By**: None

  **References**:
  - `src/messages.rs:152` - MoveTo 定义位置

  **Acceptance Criteria**:
  - [ ] `grep -r "MoveTo" src/messages.rs` 无结果
  - [ ] `cargo build` 成功

  **Commit**: NO

- [x] 2. **创建 WriteValue 枚举类型**

  **What to do**:
  - 在 `src/workers/modbus/client.rs` 中创建 `WriteValue` 枚举
  - 定义: `pub enum WriteValue { Coil(bool), Holding(u16) }`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 1)
  - **Blocked By**: None

  **References**:
  - `src/workers/modbus/client.rs:1-26` - 类型定义位置

  **Commit**: NO

- [x] 3. **重构写入方法为统一接口**

  **What to do**:
  - 创建 `write_registers(kind, address, values: &[WriteValue])` 方法
  - 实现 FC 自动选择: len==1 → FC05/06, len>1 → FC15/16
  - 处理空 values 错误

  **Must NOT do**:
  - 不要修改 ModbusOp 的公共签名
  - 不要改变 RPC API

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Blocked By**: Task 1, Task 2

  **References**:
  - `src/workers/modbus/client.rs:207-363` - 现有写入方法

  **Commit**: NO

- [x] 4. **更新调用点并清理**

  **What to do**:
  - 更新 `execute_operation` 使用新的 `write_registers`
  - 从 `rpc_worker.rs` 移除 `RpcMethod::MoveTo` (lines 76-79, 207-213)
  - 从 `modbus/worker.rs` 移除 MoveTo 处理 (lines 385-394)
  - 运行 `cargo test` 验证

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Blocked By**: Task 3

  **References**:
  - `src/workers/rpc_worker.rs:76-79, 207-213`
  - `src/workers/modbus/worker.rs:385-394`

  **Acceptance Criteria**:
  - [ ] `grep -r "MoveTo" src/workers/` 无结果
  - [ ] `cargo test` 通过
  - [ ] `cargo clippy` 无警告

  **Commit**: YES
  - Message: `refactor(modbus): remove MoveTo and unify write methods`

---

## Final Verification Wave (MANDATORY)

4 个 review agent 并行运行。全部必须 APPROVE。拒绝 → 修复 → 重新运行。

- [x] F1. **Plan Compliance Audit** — `oracle`
  读取计划全文。检查每个 "Must Have": 实现是否存在。检查每个 "Must NOT Have": 是否有违规。检查证据文件。对比交付物与计划。

- [x] F2. **Code Quality Review** — `unspecified-high`
  运行 `cargo clippy` + `cargo test`。检查所有修改文件。检查 AI slop。

- [x] F3. **Test Verification** — `unspecified-high`
  运行所有测试。验证 MoveTo 相关测试已移除。验证写入测试通过。

- [x] F4. **Scope Fidelity Check** — `deep`
  检查每个任务是否只做了 "What to do" 中的内容。检测范围蔓延。

---

## Commit Strategy

- **单次提交**: `refactor(modbus): remove MoveTo and unify write methods`
  - 所有修改的文件
  - Pre-commit: `cargo test && cargo clippy`

---

## Success Criteria

### 验证命令
```bash
cargo test        # 期望: 所有测试通过
cargo clippy      # 期望: 无警告
grep -r "MoveTo" src/  # 期望: 无结果
```

### 最终检查清单
- [ ] 所有 "Must Have" 存在
- [ ] 所有 "Must NOT Have" 不存在
- [ ] 所有测试通过
- [ ] MoveTo 完全移除
- [ ] 写入方法统一且行为正确