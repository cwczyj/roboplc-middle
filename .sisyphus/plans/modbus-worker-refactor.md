# Modbus Worker Refactoring - Complete Register Type Support

## TL;DR

> **Quick Summary**: Refactor modbus_worker.rs to support all Modbus register types (Coils, Discrete Inputs, Input Registers, Holding Registers) with proper data type parsing, while improving code organization by splitting into modular components and deleting unused profiles module.
> 
> **Deliverables**:
> - Complete register type support (c, d, i, h)
> - Data type parsing (U16, U32, I16, I32, F32, Bool)
> - Batch read + field parsing for SignalGroups
> - Dual write API (field-based + raw values)
> - Code reorganization into src/workers/modbus/ subdirectory
> - Shared data_conversion.rs module
> - Removed MoveTo operation handling
> - Deleted src/profiles/ module
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 4 waves
> **Critical Path**: Setup → Data Conversion → Modbus Modules → Integration

---

## Context

### Original Request

User requested 5 specific improvements to `src/workers/modbus_worker.rs`:
1. Implement register content parsing (missing data type conversion)
2. Support all Modbus register types (currently only Holding registers)
3. Remove MoveTo operation (replaced by signal groups)
4. Continue current manual approach (not using roboplc binrw)
5. Refactor code structure for better organization

### Interview Summary

**Key Discussions**:
- **Parsing Strategy**: Chose batch read (single Modbus operation) + memory parse - most efficient
- **Data Conversion**: Move DataTypeConverter to shared src/data_conversion.rs module
- **Register Types**: One register type per SignalGroup (keep current design)
- **Write API**: Support both field-based JSON and raw values approaches
- **Code Organization**: Create src/workers/modbus/ subdirectory with 5 files
- **Profiles Module**: Delete entirely, keep useful parts in shared module
- **Testing**: TDD approach - write tests before implementation

**Research Findings**:
- RoboPLC 0.6.4 supports all 4 register types via ModbusRegisterKind enum
- DataTypeConverter exists in profiles/device_profile.rs (well-tested)
- Current modbus_worker only supports Holding registers
- parse_address() extracts prefix but ignores it (always returns holding)
- Individual register reads are inefficient (creates new ModbusMapping per register)

### Metis Review

**Identified Gaps** (addressed):
- **Backward compatibility for MoveTo**: Will remove operation handling entirely, return "Operation not supported" error (not "not implemented")
- **Batch read as replacement**: Replace individual register loop only for register type support, not as optimization feature
- **Error recovery**: If field parsing fails, return None and skip field (don't add retry logic)
- **Coil/Discrete packing**: Single-bit values read as u8 (0 or 1), not packed 8 coils = 1 register
- **Dual write API precedence**: Field-based JSON takes precedence if both provided; validation error if field names don't match config
- **SignalGroup validation**: Enforce one register type per group (validated in config.rs)
- **Test coverage**: Comprehensive test list covering all register types, data types, edge cases

**Guardrails Applied** (from Metis review):
- MUST NOT: Modify Message enum, Hub routing, DeviceManager, RpcWorker, Variables structure
- MUST NOT: Add configuration fields beyond SignalGroup enhancements
- MUST NOT: Add retry logic, caching, or auto-discovery
- MUST: Preserve all existing unit tests (22 tests)
- MUST: Follow TDD approach with comprehensive test coverage
- MUST: Preserve heartbeat, connection state machine, exponential backoff, latency monitoring

---

## Work Objectives

### Core Objective

Refactor src/workers/modbus_worker.rs (1108 lines) into modular components with complete Modbus register type support, proper data type parsing, and improved code organization, while preserving all existing behavior and backward compatibility.

### Concrete Deliverables

1. **src/data_conversion.rs** (NEW)
   - DataTypeConverter trait and DefaultDataTypeConverter impl
   - convert_byte_order, bytes_len helper functions
   - RegisterPair struct for multi-register values
   - Comprehensive unit tests

2. **src/workers/modbus/** (NEW DIRECTORY)
   - `mod.rs` - Main ModbusWorker struct and Worker trait impl
   - `client.rs` - ModbusClient with connection management
   - `operations.rs` - ModbusOp enum with register type support
   - `parsing.rs` - Signal group field parser and data extraction
   - `types.rs` - TransactionId, ConnectionState, Backoff, TimeoutHandler, OperationQueue

3. **src/profiles/** (DELETE)
   - Remove entire directory
   - Keep DataTypeConverter in shared module

4. **Updated Tests**
   - All 22 existing unit tests preserved
   - New tests for register type support (Coil, Discrete, Input, Holding)
   - New tests for data type parsing (U16, U32, I16, I32, F32, Bool)
   - New tests for batch read + field parsing
   - New tests for dual write API

### Definition of Done

- [ ] All 4 register types supported: Coils (c), Discrete Inputs (d), Input Registers (i), Holding Registers (h)
- [ ] All 6 data types parsed: U16, U32, I16, I32, F32, Bool
- [ ] Batch read implemented: single Modbus operation reads all registers in SignalGroup
- [ ] Field parsing implemented: extract field values based on offset and data type
- [ ] Dual write API implemented: field-based JSON and raw values both supported
- [ ] MoveTo operation removed from handling
- [ ] Code reorganized into src/workers/modbus/ subdirectory
- [ ] DataTypeConverter moved to shared src/data_conversion.rs
- [ ] src/profiles/ directory deleted
- [ ] All existing unit tests passing (22 tests)
- [ ] New tests passing for all register types and data types
- [ ] `cargo build` succeeds with no errors
- [ ] `cargo test` succeeds with all tests passing
- [ ] `cargo clippy` passes with no warnings

### Must Have

- Complete register type support (Coil, Discrete, Input, Holding)
- Data type parsing using DataTypeConverter
- Batch read + field parsing for SignalGroups
- Dual write API (field-based + raw values)
- Code reorganization into modular components
- All existing tests preserved and passing
- TDD approach followed

### Must NOT Have (Guardrails)

- NO changes to Message enum or Operation variants
- NO changes to Hub routing or DeviceManager
- NO changes to RpcWorker request handling
- NO changes to Variables shared state structure
- NO retry logic for field parsing failures
- NO register caching or dirty bit tracking
- NO auto-discovery or auto-configuration
- NO batch read optimization beyond register type support
- NO new configuration fields in Device struct
- NO changes to WorkerOpts configuration (cpu, scheduling, priority)

---

## Verification Strategy

### Test Decision

- **Infrastructure exists**: YES (cargo test framework)
- **Automated tests**: YES (TDD - tests first)
- **Framework**: cargo test with Rust built-in test framework
- **TDD Workflow**: Each task follows RED (failing test) → GREEN (minimal impl) → REFACTOR

### QA Policy

Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Library/Module**: Use Bash (cargo test) — Run unit tests, integration tests
- **API/Backend**: Use Bash (curl) — Test JSON-RPC endpoints
- **Code Quality**: Use Bash (cargo clippy, cargo fmt --check) — Lint and format checks

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundation + scaffolding):
├── Task 1: Create data_conversion.rs module [quick]
├── Task 2: Create src/workers/modbus/ directory structure [quick]
├── Task 3: Extract types.rs (TransactionId, ConnectionState, etc.) [quick]
└── Task 4: Extract client.rs (ModbusClient) [quick]

Wave 2 (After Wave 1 — register type support):
├── Task 5: Extract operations.rs with register type support [deep]
├── Task 6: Extract parsing.rs for field parsing [deep]
├── Task 7: Refactor mod.rs with all register types [deep]
└── Task 8: Delete src/profiles/ directory [quick]

Wave 3 (After Wave 2 — integration + write API):
├── Task 9: Implement dual write API (field-based + raw) [unspecified-high]
├── Task 10: Remove MoveTo operation handling [quick]
├── Task 11: Update module imports and exports [quick]
└── Task 12: Comprehensive test coverage [unspecified-high]

Wave 4 (After Wave 3 — verification + cleanup):
├── Task 13: Integration tests (all register types) [deep]
├── Task 14: Edge case tests (20 scenarios) [unspecified-high]
├── Task 15: Code quality checks (clippy, fmt) [quick]
└── Task 16: Documentation and comments [writing]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
```

### Dependency Matrix

- **1-4**: No dependencies (can run in parallel)
- **5-7**: Depend on 1-4 (need data_conversion and types)
- **8**: No dependencies (can run parallel with 5-7)
- **9-11**: Depend on 5-7 (need operations and parsing)
- **12**: Depends on 9-11 (integration tests)
- **13-16**: Depend on 12 (comprehensive tests)
- **F1-F4**: Depend on all previous tasks

### Agent Dispatch Summary

- **Wave 1**: 4 tasks → `quick` (T1-T4)
- **Wave 2**: 4 tasks → `deep` (T5-T7), `quick` (T8)
- **Wave 3**: 4 tasks → `unspecified-high` (T9, T12), `quick` (T10-T11)
- **Wave 4**: 4 tasks → `deep` (T13), `unspecified-high` (T14), `quick` (T15), `writing` (T16)
- **Wave FINAL**: 4 tasks → `oracle` (F1), `unspecified-high` (F2-F3), `deep` (F4)

---

- [ ] 1. Create src/data_conversion.rs Module

  **What to do**:
  - Create new file: src/data_conversion.rs
  - Copy DataTypeConverter trait from profiles/device_profile.rs
  - Copy DefaultDataTypeConverter implementation
  - Copy helper functions: convert_byte_order(), bytes_len()
  - Copy RegisterPair struct and impl
  - Add module declaration in src/lib.rs
  - Write comprehensive unit tests (TDD approach)
  
  **Tests to write**:
  ```rust
  #[test] fn convert_u16_to_f64()
  #[test] fn convert_i32_with_negative_value()
  #[test] fn convert_f32_preserves_precision()
  #[test] fn convert_with_byte_order_big_endian()
  #[test] fn convert_with_byte_order_little_endian()
  #[test] fn register_pair_to_u32()
  #[test] fn register_pair_to_f32()
  ```

  **Must NOT do**:
  - Do NOT add validation beyond existing DataTypeConverter
  - Do NOT add new data types beyond U16, U32, I16, I32, F32, Bool
  - Do NOT modify profiles module yet (will delete in Task 8)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: File creation with existing code, straightforward task
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 5, 6, 7 (need data_conversion module)
  - **Blocked By**: None

  **References**:
  - `src/profiles/device_profile.rs:127-196` - DataTypeConverter trait and impl to move
  - `src/profiles/device_profile.rs:108-125` - RegisterPair struct
  - `src/lib.rs:49-50` - Module declarations location
  - `AGENTS.md:44-46` - Import organization pattern

  **Acceptance Criteria**:
  - [ ] File created: src/data_conversion.rs exists
  - [ ] DataTypeConverter trait moved (not copied)
  - [ ] All helper functions present
  - [ ] Unit tests passing: `cargo test --lib data_conversion`
  - [ ] No compilation errors: `cargo build`

  **QA Scenarios**:
  ```
  Scenario: Convert U16 to f64 successfully
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test convert_u16_to_f64 --lib
    Expected Result: Test passes
    Evidence: .sisyphus/evidence/task-1-u16-conversion.txt

  Scenario: Convert with Little Endian byte order
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test convert_with_byte_order_little_endian --lib
    Expected Result: Test passes with correct byte swapping
    Evidence: .sisyphus/evidence/task-1-little-endian.txt
  ```

  **Commit**: YES (groups with 1)
  - Message: `refactor(modbus): create shared data_conversion module`
  - Files: src/data_conversion.rs, src/lib.rs
  - Pre-commit: `cargo test --lib data_conversion`

- [ ] 2. Create src/workers/modbus/ Directory Structure

  **What to do**:
  - Create directory: src/workers/modbus/
  - Create empty module files:
    - src/workers/modbus/mod.rs
    - src/workers/modbus/client.rs
    - src/workers/modbus/operations.rs
    - src/workers/modbus/parsing.rs
    - src/workers/modbus/types.rs
  - Update src/workers/mod.rs to declare modbus submodule
  - Add basic module exports in modbus/mod.rs
  - Verify cargo build succeeds with empty modules

  **Tests to write**:
  - No tests needed for directory creation
  - Verify: cargo build succeeds

  **Must NOT do**:
  - Do NOT implement any code yet (just module structure)
  - Do NOT delete modbus_worker.rs yet (will do in later task)
  - Do NOT modify any existing worker code

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple directory and file creation
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Tasks 3, 4, 5, 6, 7
  - **Blocked By**: None

  **References**:
  - `src/profiles/mod.rs` - Example of submodule pattern
  - `src/workers/mod.rs` - Location to add modbus submodule
  - `AGENTS.md:67-72` - Project structure pattern

  **Acceptance Criteria**:
  - [ ] Directory created: src/workers/modbus/ exists
  - [ ] All 5 files created: mod.rs, client.rs, operations.rs, parsing.rs, types.rs
  - [ ] src/workers/mod.rs updated to declare modbus module
  - [ ] cargo build succeeds with empty modules

  **QA Scenarios**:
  ```
  Scenario: Module structure compiles
    Tool: Bash (cargo build)
    Steps:
      1. Run: cargo build
    Expected Result: Build succeeds with no errors
    Evidence: .sisyphus/evidence/task-2-module-structure.txt

  Scenario: Module structure follows pattern
    Tool: Bash (ls)
    Steps:
      1. Run: ls -la src/workers/modbus/
    Expected Result: Shows 5 files (mod.rs, client.rs, operations.rs, parsing.rs, types.rs)
    Evidence: .sisyphus/evidence/task-2-directory-listing.txt
  ```

  **Commit**: NO (part of Task 1 commit)

- [ ] 3. Extract types.rs (TransactionId, ConnectionState, etc.)

  **What to do**:
  - Copy from src/workers/modbus_worker.rs to src/workers/modbus/types.rs:
    - TransactionId struct and impl (lines 52-69)
    - ConnectionState enum (lines 73-78)
    - Backoff struct and impl (lines 82-111)
    - TimeoutHandler struct and impl (lines 115-146)
    - OperationQueue struct and impl (lines 150-197)
  - Add proper imports at top of types.rs
  - Add pub exports in types.rs
  - Write unit tests for each type (preserve existing tests)
  
  **Tests to write**:
  ```rust
  #[test] fn transaction_id_increments()
  #[test] fn transaction_id_has_timestamp()
  #[test] fn backoff_next_delay_is_exponential()
  #[test] fn backoff_reset_restores_state()
  #[test] fn timeout_handler_doubles_on_timeout()
  #[test] fn operation_queue_limits_concurrency()
  ```

  **Must NOT do**:
  - Do NOT modify the logic of these types (preserve exactly)
  - Do NOT delete from modbus_worker.rs yet (will do in Task 7)
  - Do NOT add new types or functionality

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Extract existing code to new file, preserve behavior
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Tasks 5, 7
  - **Blocked By**: Task 2 (need modbus directory)

  **References**:
  - `src/workers/modbus_worker.rs:52-197` - Types to extract
  - `src/workers/modbus_worker.rs:842-1067` - Existing tests for these types
  - `AGENTS.md:44-46` - Import organization

  **Acceptance Criteria**:
  - [ ] File created: src/workers/modbus/types.rs with all 5 types
  - [ ] All unit tests passing: `cargo test --lib workers::modbus::types`
  - [ ] Types properly exported via mod.rs
  - [ ] cargo build succeeds

  **QA Scenarios**:
  ```
  Scenario: All types tests pass
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test --lib workers::modbus::types
    Expected Result: 6 tests pass
    Evidence: .sisyphus/evidence/task-3-types-tests.txt

  Scenario: TransactionId increments correctly
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test transaction_id_increments --lib
    Expected Result: Test passes, IDs are unique
    Evidence: .sisyphus/evidence/task-3-transaction-id.txt
  ```

  **Commit**: NO (part of Task 1 commit)

- [ ] 4. Extract client.rs (ModbusClient)

  **What to do**:
  - Copy from src/workers/modbus_worker.rs to src/workers/modbus/client.rs:
    - ModbusOp enum (lines 201-206)
    - OperationResult struct (lines 209-214)
    - QueuedOperation struct (lines 217-221)
    - ModbusClient struct and impl (lines 225-419)
  - Update imports to use types from types.rs
  - Add pub exports
  - Write unit tests (preserve existing tests)
  
  **Tests to write**:
  ```rust
  #[test] fn modbus_client_new_starts_disconnected()
  // Note: Most client tests require actual Modbus connection, 
  // so focus on structural tests
  ```

  **Must NOT do**:
  - Do NOT modify ModbusClient logic yet (will add register types in Task 5)
  - Do NOT delete from modbus_worker.rs yet
  - Do NOT change the ModbusOp enum structure (will enhance in Task 5)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Extract existing code, straightforward
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Tasks 5, 7
  - **Blocked By**: Task 2 (need modbus directory), Task 3 (need types)

  **References**:
  - `src/workers/modbus_worker.rs:201-419` - Client code to extract
  - `src/workers/modbus_worker.rs:877-883` - Existing client test
  - `src/workers/modbus_worker.rs:22-36` - Required imports

  **Acceptance Criteria**:
  - [ ] File created: src/workers/modbus/client.rs
  - [ ] ModbusClient, ModbusOp, OperationResult extracted
  - [ ] Unit tests passing
  - [ ] cargo build succeeds

  **QA Scenarios**:
  ```
  Scenario: Client module compiles
    Tool: Bash (cargo build)
    Steps:
      1. Run: cargo build --lib
    Expected Result: Build succeeds with client.rs
    Evidence: .sisyphus/evidence/task-4-client-build.txt

  Scenario: Client tests pass
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test modbus_client --lib
    Expected Result: Tests pass
    Evidence: .sisyphus/evidence/task-4-client-tests.txt
  ```

  **Commit**: YES (Wave 1 complete)
  - Message: `refactor(modbus): extract types and client to separate modules`
  - Files: src/workers/modbus/{types.rs, client.rs, mod.rs}, src/workers/mod.rs
  - Pre-commit: `cargo test --lib workers::modbus`

---


> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files exist. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo build`, `cargo clippy`, `cargo fmt --check`, `cargo test`. Review all changed files for: `as any`/`@ts-ignore` (Rust: `unwrap()` without error handling), empty catches, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task. Test cross-task integration. Test edge cases: empty SignalGroup, invalid field names, negative values. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance. Detect cross-task contamination.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **After Wave 1**: `refactor(modbus): create shared data_conversion module and extract types`
- **After Wave 2**: `feat(modbus): add complete register type support (Coil, Discrete, Input, Holding)`
- **After Wave 3**: `feat(modbus): implement dual write API and remove MoveTo operation`
- **After Wave 4**: `test(modbus): add comprehensive test coverage for all register types`
- **Final**: `refactor(modbus): complete modbus_worker reorganization with TDD approach`

Pre-commit checks: `cargo test && cargo clippy && cargo fmt --check`

---

## Success Criteria

### Verification Commands

```bash
# Verify all register types supported
cargo test read_coil_register
cargo test read_discrete_input_register
cargo test read_input_register
cargo test read_holding_register

# Verify data type parsing
cargo test convert_u16_to_f64
cargo test convert_f32_preserves_precision
cargo test convert_with_byte_order

# Verify batch read + field parsing
cargo test batch_read_signal_group
cargo test parse_fields_correctly

# Verify dual write API
cargo test write_signal_group_field_based
cargo test write_signal_group_raw_values

# Verify code quality
cargo build --release
cargo test --all
cargo clippy -- -D warnings
cargo fmt --check

# Verify profiles deleted
test ! -d src/profiles/

# Verify modbus subdirectory created
test -f src/workers/modbus/mod.rs
test -f src/workers/modbus/client.rs
test -f src/workers/modbus/operations.rs
test -f src/workers/modbus/parsing.rs
test -f src/workers/modbus/types.rs

# Verify data_conversion module created
test -f src/data_conversion.rs
```

### Final Checklist

- [ ] All 4 register types supported (c, d, i, h)
- [ ] All 6 data types parsed (U16, U32, I16, I32, F32, Bool)
- [ ] Batch read + field parsing implemented
- [ ] Dual write API implemented
- [ ] MoveTo operation removed
- [ ] Code reorganized into modbus subdirectory
- [ ] src/profiles/ deleted
- [ ] All 22 existing tests passing
- [ ] New comprehensive tests passing
- [ ] `cargo build` succeeds
- [ ] `cargo test` all passing
- [ ] `cargo clippy` no warnings
- [ ] `cargo fmt --check` passes