# Modbus 模块重构与完善

## TL;DR

> **Quick Summary**: 重构 `workers/modbus/` 模块，解决文件组织混乱、性能低下、功能不全三大问题。实现四类 Modbus 寄存器（Coil/Discrete/Input/Holding）的完整读写支持，优化批量读写性能，集成字段解析功能。
> 
> **Deliverables**: 
> - 重构后的模块结构（`worker.rs` 新建，`mod.rs` 精简）
> - 批量读写 API（性能提升 100x）
> - 完整的四类寄存器支持
> - 删除重复文件 `modbus_worker.rs`
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 4 waves
> **Critical Path**: 文件重构 → client.rs 重构 → Worker 集成 → 测试验证

---

## Context

### Original Request

用户提出三个核心问题：
1. `workers/modbus/mod.rs` 包含大量 ModbusWorker 代码，应该拆分保持模块纯净
2. `workers/modbus/client.rs` 未调用 `parsing.rs` 解析方法，直接返回原始数据；且逐个读取寄存器效率低下
3. 读方法重复应整合；写操作只实现了 Holding 寄存器，需完善 Coil 写入

### Interview Summary

**Key Discussions**:
- 四类 Modbus 寄存器需要完整支持：Coil (0x, 读写布尔)、Discrete (1x, 只读布尔)、Input (3x, 只读16位)、Holding (4x, 读写16位)
- 配置文件定义 signal_groups，包含 register_address、register_count、fields
- 性能是核心问题：当前逐个读取 100 个寄存器需要 100 次 TCP 请求

**Research Findings**:
- RoboPLC 提供 `ModbusMapping::create(client, unit_id, register, count)` 批量 API
- 正确用法：一次创建 mapping 指定 count，然后 `read::<Vec<u16>>()` 批量读取
- 支持 binrw 结构体解析，自动处理字节序转换

### Current Code Issues

**文件结构问题**：
- `mod.rs` (664行)：混合模块导出 + ModbusWorker 实现
- `modbus_worker.rs` (1131行)：重复文件，功能与 `mod.rs` 重叠

**性能问题** (`client.rs:276-288`):
```rust
// 当前：N 次请求
for i in 0..count {
    let reg = ModbusRegister::new(kind, address + i);
    ModbusMapping::create(client, unit_id, reg, 1)  // count=1，每次读 1 个
    m.read::<u16>()
}
```

**功能缺失**：
- `ModbusOp` 只支持 `WriteSingle`/`WriteMultiple`（仅 Holding）
- 缺少 `WriteSingleCoil`/`WriteMultipleCoils`

---

## Work Objectives

### Core Objective

重构 Modbus 模块，实现：
1. **清晰的组织结构**：模块职责单一
2. **高性能批量读写**：利用 RoboPLC 批量 API
3. **完整的寄存器支持**：四类寄存器的完整读写能力
4. **字段解析集成**：正确调用 `parsing.rs`

### Concrete Deliverables

- `src/workers/modbus/worker.rs` - 新建，ModbusWorker 实现
- `src/workers/modbus/mod.rs` - 精简为纯导出模块
- `src/workers/modbus/client.rs` - 重构批量读写
- `src/workers/modbus/operations.rs` - 扩展操作类型
- 删除 `src/workers/modbus_worker.rs` - 移除重复文件

### Definition of Done

- [ ] `cargo build` 编译通过
- [ ] `cargo test` 所有测试通过
- [ ] 批量读取 100 个寄存器 ≤ 1 次 TCP 请求
- [ ] 四类寄存器读写操作完整
- [ ] `mod.rs` 行数 ≤ 30 行

### Must Have
- 文件结构重构完成
- 批量读写 API 实现
- 四类寄存器读写支持
- **字段写入支持**：使用 `encode_fields_to_registers` 解析用户字段数据
- parsing.rs 集成（读取和写入路径都使用）

### Must NOT Have (Guardrails)

- 不改变外部 API 接口（JSON-RPC 方法签名不变）
- 不修改配置文件格式
- 不添加新的依赖 crate
- 不过度抽象（保持代码简洁）
- **不删除字段写入功能**（必须保留 encode_fields_to_registers 调用）
- 文件结构重构完成
- 批量读写 API 实现
- 四类寄存器读写支持
- **字段写入支持**：使用 `encode_fields_to_registers` 解析用户字段数据
- parsing.rs 集成（读取和写入路径都使用）

### Must NOT Have (Guardrails)

- 不改变外部 API 接口（JSON-RPC 方法签名不变）
- 不修改配置文件格式
- 不添加新的依赖 crate
- 不过度抽象（保持代码简洁）
- **不删除字段写入功能**（必须保留 encode_fields_to_registers 调用）

- 文件结构重构完成
- 批量读写 API 实现
- 四类寄存器读写支持
- parsing.rs 集成

### Must NOT Have (Guardrails)

- 不改变外部 API 接口（JSON-RPC 方法签名不变）
- 不修改配置文件格式
- 不添加新的依赖 crate
- 不过度抽象（保持代码简洁）

---

## Verification Strategy

### Test Decision

- **Infrastructure exists**: YES (cargo test)
- **Automated tests**: YES (TDD - 先写测试，后实现)
- **Framework**: cargo test
- **TDD**: 每个功能模块先写测试用例

### QA Policy

- **单元测试**：每个公开方法有测试覆盖
- **集成测试**：Mock Modbus 服务器验证完整流程
- **性能验证**：对比批量 vs 逐个读取的时间

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — 基础架构):
├── Task 1: 创建 worker.rs 提取 ModbusWorker [deep]
├── Task 2: 精简 mod.rs 为纯导出模块 [quick]
├── Task 3: 扩展 ModbusOp 枚举 [quick]
├── Task 4: 扩展 RegisterType 工具方法 [quick]
└── Task 5: 更新 operations.rs 寄存器类型映射 [quick]

Wave 2 (After Wave 1 — client.rs 重构):
├── Task 6: 重构 read_registers 统一方法 [deep]
├── Task 7: 实现批量读取优化 [deep]
├── Task 8: 实现 write_single_coil 方法 [quick]
├── Task 9: 实现 write_multiple_coils 方法 [quick]
└── Task 10: 更新 execute_operation 分发逻辑 [quick]

Wave 3 (After Wave 2 — Worker 集成):
├── Task 11: 更新 operation_to_modbus_op 支持所有类型 [deep]
├── Task 12: 集成 parsing.rs 字段解析 [deep]
├── Task 13: 更新 WriteSignalGroup 处理逻辑 [quick]
└── Task 14: 更新 ReadSignalGroup 返回解析后数据 [quick]

Wave 4 (After Wave 3 — 清理验证):
├── Task 15: 删除重复文件 modbus_worker.rs [quick]
├── Task 16: 更新 workers/mod.rs 导入 [quick]
├── Task 17: 更新单元测试 [unspecified-high]
├── Task 18: 添加集成测试 [unspecified-high]
└── Task 19: 验证性能优化效果 [deep]

Wave FINAL (After ALL tasks):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Functional QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
```

### Dependency Matrix

- **1-5**: — — 6-10, 1
- **6-7**: 3, 5 — 11-14, 2
- **8-10**: 3 — 11-14, 2
- **11-14**: 6-10 — 17-19, 3
- **15-19**: 11-14 — F1-F4, 4

### Agent Dispatch Summary

- **Wave 1**: **5** — T1 → `deep`, T2-T5 → `quick`
- **Wave 2**: **5** — T6-T7 → `deep`, T8-T10 → `quick`
- **Wave 3**: **4** — T11-T12 → `deep`, T13-T14 → `quick`
- **Wave 4**: **5** — T15-T16 → `quick`, T17-T18 → `unspecified-high`, T19 → `deep`
- **FINAL**: **4** — F1 → `oracle`, F2-F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

  **Commit**: YES (Wave 1 complete)
  - Message: `refactor(modbus): restructure module organization`
  - Files: `src/workers/modbus/mod.rs`, `src/workers/modbus/worker.rs`, `src/workers/modbus/client.rs`, `src/workers/modbus/operations.rs`

- [ ] 6. 重构 read_registers 统一方法

  **What to do**:
  - 在 `client.rs` 创建统一的 `read_registers()` 方法
  - 替换现有的 `read_coil`, `read_discrete`, `read_input`, `read_holding` 四个方法
  - 方法签名：`fn read_registers(&self, client: &Client, kind: ModbusRegisterKind, address: u16, count: u16) -> OperationResult`
  - 使用批量读取 API：一次 `ModbusMapping::create` 指定 count

  **Must NOT do**:
  - 不要逐个读取寄存器（禁止 for 循环 read::<u16>）
  - 不要改变返回的数据结构格式

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 核心性能优化，需要正确理解 RoboPLC API
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 7-10)
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Tasks 3, 5

  **References**:
  - `src/workers/modbus/client.rs:116-307` - 现有的四个 read 方法
  - RoboPLC docs: `ModbusMapping::create(client, unit_id, register, count)` 批量读取

  **Correct Implementation Pattern**:
  ```rust
  fn read_registers(&self, client: &Client, kind: ModbusRegisterKind, address: u16, count: u16) -> OperationResult {
      let register = ModbusRegister::new(kind, address);
      let mut mapping = match ModbusMapping::create(client, self.unit_id, register, count) {
          Ok(m) => m,
          Err(e) => { return OperationResult { success: false, ... }; }
      };
      
      let start = SystemTime::now();
      
      // 批量读取：一次请求获取所有寄存器
      let values = match kind {
          ModbusRegisterKind::Coil | ModbusRegisterKind::Discrete => {
              mapping.read::<Vec<u8>>().map(|v| v.iter().map(|&b| if b != 0 { 1u16 } else { 0u16 }).collect())
          }
          _ => mapping.read::<Vec<u16>>()
      };
      
      match values {
          Ok(vals) => OperationResult { success: true, data: json!({"values": vals, "latency_us": ...}), ... },
          Err(e) => OperationResult { success: false, ... }
      }
  }
  ```

  **Acceptance Criteria**:
  - [ ] 单一 `read_registers` 方法存在
  - [ ] 四个旧方法被移除或调用新方法
  - [ ] 批量读取测试通过
  - [ ] `cargo test` 通过

  **QA Scenarios**:
  ```
  Scenario: 批量读取验证
    Tool: Bash
    Steps:
      1. cargo test read_registers --no-fail-fast
      2. grep -c "for i in 0..count" src/workers/modbus/client.rs || echo "No inefficient loops found"
    Expected Result: Tests pass, no loop-based reads
    Evidence: .sisyphus/evidence/task-06-batch.txt
  ```

  **Commit**: NO (groups with Wave 2)

- [ ] 7. 实现批量读取优化

  **What to do**:
  - 验证批量读取正确工作
  - 添加性能基准测试
  - 确保 Coil/Discrete 返回正确格式（Vec<u16>，0 或 1）
  - 确保 Input/Holding 返回原始 u16 值

  **Must NOT do**:
  - 不要引入额外的内存分配
  - 不要破坏现有的错误处理

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要验证性能优化效果
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6, 8-10)
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Tasks 6

  **Acceptance Criteria**:
  - [ ] 读取 100 个寄存器只有 1 次 Modbus 请求
  - [ ] 所有寄存器类型测试通过
  - [ ] 性能基准测试存在

  **QA Scenarios**:
  ```
  Scenario: 性能基准测试
    Tool: Bash
    Steps:
      1. cargo test --release bench_read
    Expected Result: Benchmark shows < 50ms for 100 registers
    Evidence: .sisyphus/evidence/task-07-perf.txt
  ```

  **Commit**: NO (groups with Wave 2)

- [ ] 8. 实现 write_single_coil 方法

  **What to do**:
  - 在 `client.rs` 添加 `write_single_coil` 方法
  - Coil 写入值：`0xFF00` = true, `0x0000` = false
  - 方法签名：`fn write_single_coil(&self, client: &Client, address: u16, value: bool) -> OperationResult`

  **Must NOT do**:
  - 不要使用 Holding 寄存器的写入方式

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的方法实现
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6-7, 9-10)
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Task 3

  **References**:
  - `src/workers/modbus/client.rs:309-345` - write_single 作为参考

  **Implementation**:
  ```rust
  fn write_single_coil(&self, client: &Client, address: u16, value: bool) -> OperationResult {
      let register = ModbusRegister::new(ModbusRegisterKind::Coil, address);
      let mut mapping = ModbusMapping::create(client, self.unit_id, register, 1)?;
      
      // Coil 写入：true = 0xFF00, false = 0x0000
      let coil_value: u16 = if value { 0xFF00 } else { 0x0000 };
      mapping.write(coil_value)
  }
  ```

  **Acceptance Criteria**:
  - [ ] `write_single_coil` 方法实现
  - [ ] 单元测试通过

  **Commit**: NO (groups with Wave 2)

- [ ] 9. 实现 write_multiple_coils 方法

  **What to do**:
  - 在 `client.rs` 添加 `write_multiple_coils` 方法
  - 批量写入多个 Coil
  - 方法签名：`fn write_multiple_coils(&self, client: &Client, address: u16, values: &[bool]) -> OperationResult`

  **Must NOT do**:
  - 不要逐个写入 Coil

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的方法实现
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6-8, 10)
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Task 3

  **Acceptance Criteria**:
  - [ ] `write_multiple_coils` 方法实现
  - [ ] 批量写入测试通过

  **Commit**: NO (groups with Wave 2)

- [ ] 10. 更新 execute_operation 分发逻辑

  **What to do**:
  - 更新 `ModbusClient::execute_operation` 方法
  - 添加对新 `ModbusOp` 变体的处理
  - 分发到正确的读写方法

  **Must NOT do**:
  - 不要遗漏任何变体处理

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的 match 分支更新
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6-9)
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Tasks 3, 6

  **References**:
  - `src/workers/modbus/client.rs:86-114` - 现有 execute_operation

  **Acceptance Criteria**:
  - [ ] 所有 `ModbusOp` 变体正确处理
  - [ ] `cargo build` 编译通过
  - [ ] `cargo test` 通过

  **Commit**: YES (Wave 2 complete)
  - Message: `perf(modbus): implement batch read/write optimization`
  - Files: `src/workers/modbus/client.rs`

- [ ] 11. 更新 operation_to_modbus_op 支持所有类型

  **What to do**:
  - 在 `worker.rs` 更新 `operation_to_modbus_op` 方法
  - 根据 `signal_group.register_address` 前缀选择正确的寄存器类型
  - 支持 `c`/`d`/`i`/`h` 四种前缀

  **Must NOT do**:
  - 不要硬编码只支持 Holding

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要理解配置和操作映射逻辑
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 12-14)
  - **Blocks**: Tasks 17-19
  - **Blocked By**: Tasks 6-10

  **References**:
  - `src/workers/modbus/mod.rs:194-270` - 现有 operation_to_modbus_op

  **Acceptance Criteria**:
  - [ ] 支持四种寄存器类型
  - [ ] 单元测试覆盖所有类型

  **Commit**: NO (groups with Wave 3)

- [ ] 12. 集成 parsing.rs 字段解析

  **What to do**:
  - 在 `ReadSignalGroup` 处理中调用 `parse_signal_group_fields`
  - 使用 `parsing.rs` 解析原始寄存器数据为字段值
  - 返回解析后的字段数据而非原始 `Vec<u16>`

  **Must NOT do**:
  - 不要跳过解析直接返回原始数据

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要理解数据流和解析逻辑
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 11, 13-14)
  - **Blocks**: Tasks 17-19
  - **Blocked By**: Tasks 6-10

  **References**:
  - `src/workers/modbus/parsing.rs:65-114` - parse_signal_group_fields 函数
  - `src/workers/modbus/mod.rs:363-402` - ReadSignalGroup 处理

  **Acceptance Criteria**:
  - [ ] `ReadSignalGroup` 返回解析后字段
  - [ ] 字段值正确转换（字节序、数据类型）

  **Commit**: NO (groups with Wave 3)

- [ ] 13. 更新 WriteSignalGroup 处理逻辑
  **What to do**:
  - 使用 `encode_fields_to_registers` 编码写入数据
  - 支持所有四种寄存器类型的写入
  - **字段写入流程**：
    1. 接收用户 JSON 字段数据：`{"temperature": 25.0, "pressure": 101.3}`
    2. 查找配置中的 signal_group 和 fields 定义
    3. 调用 `encode_fields_to_registers(fields_data, &group.fields, register_count, byte_order)`
    4. 将字段值转换为寄存器值：`Vec<u16>`
    5. 调用 ModbusClient 批量写入

  **Must NOT do**:
  - 不要直接接受原始数组写入（使用字段解析）
  - 不要跳过字节序转换

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要理解字段编码和配置映射
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 11-12, 14)
  - **Blocks**: Tasks 17-19
  - **Blocked By**: Tasks 6-10

  **References**:
  - `src/workers/modbus/parsing.rs:132-174` - encode_fields_to_registers 函数
  - `src/workers/modbus/mod.rs:228-256` - 现有 WriteSignalGroup 处理

  **Acceptance Criteria**:
  - [ ] 字段写入正确编码
  - [ ] 字节序处理正确（big_endian, little_endian 等）
  - [ ] 多寄存器字段（U32, F32）正确拆分
  - [ ] 单元测试覆盖字段写入

  **Commit**: NO (groups with Wave 3)

  **What to do**:
  - 使用 `encode_fields_to_registers` 编码写入数据
  - 支持所有四种寄存器类型的写入

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的逻辑更新
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 11-12, 14)
  - **Blocks**: Tasks 17-19
  - **Blocked By**: Tasks 6-10

  **Commit**: NO (groups with Wave 3)

- [ ] 14. 更新 ReadSignalGroup 返回解析后数据

  **What to do**:
  - 确保返回的数据格式符合客户端期望
  - 包含字段名和解析后的值

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的数据格式调整
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 11-13)
  - **Blocks**: Tasks 17-19
  - **Blocked By**: Tasks 12

  **Commit**: YES (Wave 3 complete)
  - Message: `feat(modbus): integrate parsing and support all register types`
  - Files: `src/workers/modbus/worker.rs`

- [ ] 15. 删除重复文件 modbus_worker.rs

  **What to do**:
  - 删除 `src/workers/modbus_worker.rs`
  - 确保没有其他文件引用它

  **Must NOT do**:
  - 不要保留任何重复代码

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的文件删除
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 16-19)
  - **Blocks**: None
  - **Blocked By**: Tasks 11-14

  **QA Scenarios**:
  ```
  Scenario: 重复文件检查
    Tool: Bash
    Steps:
      1. test ! -f src/workers/modbus_worker.rs
    Expected Result: File not found (exit code 0)
    Evidence: .sisyphus/evidence/task-15-delete.txt
  ```

  **Commit**: NO (groups with Wave 4)

- [ ] 16. 更新 workers/mod.rs 导入

  **What to do**:
  - 确保从正确的模块导入 `ModbusWorker`
  - 更新 `pub mod modbus;` 和相关导出

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的导入更新
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 15, 17-19)
  - **Blocks**: None
  - **Blocked By**: Task 15

  **Commit**: NO (groups with Wave 4)

- [ ] 17. 更新单元测试

  **What to do**:
  - 更新所有受影响的测试
  - 添加四种寄存器类型的测试
  - 添加批量读写的性能测试

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要全面的测试覆盖
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 15-16, 18-19)
  - **Blocks**: None
  - **Blocked By**: Tasks 11-14

  **Acceptance Criteria**:
  - [ ] 所有测试通过
  - [ ] 覆盖率 ≥ 80%

  **Commit**: NO (groups with Wave 4)

- [ ] 18. 添加集成测试

  **What to do**:
  - 使用 Mock Modbus 服务器测试完整流程
  - 测试四种寄存器类型的读写
  - 测试批量读写性能

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要设置测试环境
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 15-17, 19)
  - **Blocks**: None
  - **Blocked By**: Tasks 11-14

  **Commit**: NO (groups with Wave 4)

- [ ] 19. 验证性能优化效果

  **What to do**:
  - 运行性能基准测试
  - 对比重构前后的性能数据
  - 确认批量读取性能提升

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要分析性能数据
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 15-18)
  - **Blocks**: None
  - **Blocked By**: Tasks 17-18

  **QA Scenarios**:
  ```
  Scenario: 性能验证
    Tool: Bash
    Steps:
      1. cargo test --release bench_read
      2. Compare with baseline metrics
    Expected Result: 100x improvement for batch reads
    Evidence: .sisyphus/evidence/task-19-perf.txt
  ```

  **Commit**: YES (Wave 4 complete)
  - Message: `test(modbus): update tests and remove duplicate files`
  - Files: tests, modbus_worker.rs (deleted)

---

## Final Verification Wave

4 个审查代理并行运行，全部必须 APPROVE。

- [ ] F1. **Plan Compliance Audit** — `oracle`
- [ ] F2. **Code Quality Review** — `unspecified-high`
- [ ] F3. **Functional QA** — `unspecified-high`
- [ ] F4. **Scope Fidelity Check** — `deep`

---

## Commit Strategy

- **Wave 1**: `refactor(modbus): restructure module organization` — mod.rs, worker.rs, operations.rs
- **Wave 2**: `perf(modbus): implement batch read/write optimization` — client.rs
- **Wave 3**: `feat(modbus): integrate parsing and support all register types` — worker.rs
- **Wave 4**: `test(modbus): update tests and remove duplicate files` — tests, modbus_worker.rs (deleted)

---

## Success Criteria

### Verification Commands

```bash
cargo build                    # Expected: Compiles successfully
cargo test                     # Expected: All tests pass
cargo clippy                   # Expected: No warnings
cargo test -- --test-threads=1 # Expected: Integration tests pass
```

### Performance Metrics

```bash
# 批量读取 100 个寄存器时间对比
# Before: ~1000ms (100 × 10ms RTT)
# After:  ~10ms (1 × 10ms RTT)
```

### Final Checklist

- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass
- [ ] `mod.rs` ≤ 30 lines
- [ ] Duplicate file deleted
- [ ] Batch read/write working
- [ ] Four register types fully supported