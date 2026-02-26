# RoboPLC Communication Middleware - Implementation Plan

## TL;DR

> **Quick Summary**: Build a production-ready communication middleware using RoboPLC framework that converts JSON-RPC 2.0 (from upper-level software) to Modbus TCP (for robot arms and PLCs).
>
> **Deliverables**:
> - RoboPLC-based middleware with Controller-Worker architecture
> - JSON-RPC 2.0 Server with complete method implementations
> - Modbus TCP device drivers with per-device configuration profiles
> - HTTP Management API for device monitoring and configuration
> - Comprehensive logging, fault tolerance, and error handling
> - Complete test coverage (functional, edge case, performance, reliability)
>
> **Estimated Effort**: XL (comprehensive with production-grade edge case handling)
> **Parallel Execution**: YES - 8 waves with 4-7 tasks each
> **Critical Path**: Core Setup → Device Profiles → Modbus Workers → JSON-RPC → Testing → Final Review

---

## Context

### Original Request
用户需要利用RoboPLC框架实现一个通信中间件，将上位软件的信号转换为机械臂及PLC可用的Modbus TCP协议，上位机采用json-rpc协议与中间件通信。要求：
1. 充分利用RoboPLC框架的特性，发挥其稳定性及实时性优势
2. 详细的日志系统
3. 容错机制
4. JSON-RPC和Modbus的具体内容需要设计

### Interview Summary

**Key Discussions**:
- **Application Scenario**: Distributed system with multiple upper-level computers
- **Communication Direction**: Bidirectional (control command + status feedback)
- **Device Scale**: Medium (5-20 devices)
- **Device Types**: PLC + Robot Arm
- **Real-time Requirement**: Standard (<10ms response)
- **Fault Tolerance**: Basic (auto-reconnect + logging)
- **Logging**: INFO level (production suitable)
- **Protocol Scope**: Framework + complete protocol design
- **Deployment**: Single process + configuration file + HTTP management API
- **Architecture Decision**: Single Controller + Multi-Worker architecture (validated by user)

**Technical Decisions**:
- Framework: RoboPLC with roboplc-rpc and io::modbus
- Message System: Hub-based async communication
- Config Format: TOML
- Modbus Mapping: PLC (control h0-99, data h100-499, params h500-999), Robot (control h0-49, feedback h50-99, IO c0-31)
- JSON-RPC Methods: Ping, GetVersion, GetDeviceList, SetRegister, GetRegister, WriteBatch, ReadBatch, MoveTo, GetStatus
- **State Management Strategy**: Hybrid approach (validated by user)
  - Atomics (AtomicU32/AtomicBool) for counters/flags (devices_count, system_healthy)
  - Arc<RwLock<HashMap<>>> for device states (random access, concurrent reads)
  - DataBuffer for bulk data collection (latency monitoring, Modbus logging, event streaming)

### Research Findings

**RoboPLC Framework**:
- Controller-Worker architecture with Hub messaging
- Real-time thread scheduling (FIFO/RoundRobin, priority, CPU affinity)
- Built-in Modbus TCP/RTU support via io::modbus with binrw struct mapping
- Separate roboplc-rpc library for JSON-RPC 2.0
- Built-in logging system (roboplc::configure_logger)
- Signal handling for graceful shutdown

**Production Modbus Issues (Critical)**:
- Addressing ambiguity: 0-based vs 1-based (40001 maps to different addresses)
- Byte order variations: big-endian, little-endian, mid-little, mid-big
- Transaction ID bugs: some devices always return ID=0
- Half-open connections: devices crash without TCP FIN
- Latency degradation: device performance degrades over time
- Connection storms: simultaneous reconnections overwhelm network
- libmodbus timeout doubling: configured timeout is doubled internally
- TCP_NODELAY sensitivity: some devices reject batched packets

**DataBuffer vs Shared Variables (Decision Made)**:
- **DataBuffer**: Producer-consumer pattern, fixed-capacity ring buffer, take_all() for bulk processing
  - Best for: 延迟监控数据收集, Modbus日志, 事件流处理
  - Performance: ~1-10μs per operation, supports bulk take_all()
  - Limitations: No efficient random access (O(n)遍历)
- **AtomicU32/AtomicBool**: Lock-free atomic operations, ~1-10ns per operation
  - Best for: counters, flags (devices_count, system_healthy)
  - Limitations: Only supports single values, not collections
- **Arc<RwLock<HashMap<>>>**: Read-write lock with concurrent reads, O(1) random access
  - Best for: Shared state with random access (device_states)
  - Performance: ~1-10μs per read/write, supports concurrent reads
  - Limitations: Read-write contention under high write load
- **Selected Hybrid Approach**:
  - devices_count: AtomicU32 (atomic counter)
  - system_healthy: AtomicBool (atomic flag)
  - device_states: Arc<RwLock<HashMap<String, DeviceState>>> (random access)
  - latency_samples: DataBuffer<Duration> (bulk statistics)
  - modbus_transactions: DataBuffer<ModbusLogEntry> (ring buffer for logging)
  - device_events: DataBuffer<DeviceEvent> (event streaming)

### Metis Review - Critical Findings

**Identified Gaps (Addressed)**:
- Device-specific addressing mode and byte order configuration
- Transaction ID validation to prevent stale data
- Application-layer heartbeats (not TCP keepalives)
- Latency trend monitoring (3-sigma degradation detection)
- Exponential backoff with jitter for reconnection
- Connection pool limits per device
- Partial success reporting for batch operations

**Critical Guardrails Applied**:
- Must NOT share Modbus contexts across threads (libmodbus not thread-safe)
- Must validate Transaction ID before using response data
- Must use application-layer heartbeats (not TCP keepalives)
- Must implement per-connection Modbus contexts
- Must handle both 0-based and 1-based addressing per device
- Must support 4 byte order variants per device
- Must limit concurrent operations per device (max 3-5)
- Must NOT ignore TCP_NODELAY configuration requirements

**Edge Cases Covered**:
- 12 critical edge case tests (addressing, byte order, transaction ID, half-open, latency, TCP_NODELAY, reconnection jitter, connection limits, timeout accumulation, stale data, idle timeout)
- Performance SLA (95th percentile < 10ms)
- Reliability tests (graceful shutdown, crash isolation, config reload)
- Error handling tests (malformed RPC, unknown methods, invalid params, Modbus exceptions)

---

## Work Objectives

### Core Objective
Build a production-ready, fault-tolerant communication middleware that converts JSON-RPC 2.0 requests to Modbus TCP commands, supporting 5-20 devices with <10ms response time, comprehensive logging, and automatic error recovery.

### Concrete Deliverables

**Core Implementation**:
- Main Controller with signal handling and logger setup
- 6 Worker types: JSON-RPC Server, Device Manager, Modbus Workers (N), HTTP Management, Config Loader, Latency Monitor
- Complete JSON-RPC 2.0 method implementations (9 methods)
- Modbus TCP device driver with per-device profiles
- Device profile system with addressing mode, byte order, data type mapping
- HTTP Management API with 5 endpoints

**Configuration & Management**:
- TOML configuration file schema with validation
- Hot configuration reload functionality
- Device registration, routing, and connection management
- Application-layer heartbeat system
- **Latency monitoring with DataBuffer (producer-consumer pattern)**
- **Modbus transaction logging with DataBuffer (ring buffer)**
- **Device event streaming with DataBuffer**
- Exponential backoff with jitter for reconnection

**Testing & Documentation**:
- Functional tests (9 tests)
- Edge case tests (12 tests)
- Performance tests (4 tests)
- Reliability tests (5 tests)
- Error handling tests (10 tests)
- HTTP API tests (5 tests)
- README, configuration guide, troubleshooting guide

### Definition of Done

- [ ] All 6 worker types implemented and tested
- [ ] All 9 JSON-RPC methods working with correct error responses
- [ ] All 40+ acceptance criteria tests passing
- [ ] Modbus communication validated with real/simulated devices
- [ ] Hot config reload working without dropping connections
- [ ] Graceful shutdown handling in-flight requests
- [ ] All edge cases tested and passing
- [ ] Performance SLA met (95th percentile < 10ms)
- [ ] Documentation complete (README, config guide, troubleshooting)
- [ ] Code review passed (linter, clippy, audit)

### Must Have

- RoboPLC framework with Controller-Worker architecture
- JSON-RPC 2.0 Server with all 9 methods
- Modbus TCP device drivers with per-device configuration
- Device profile system (addressing, byte order, data types)
- Application-layer heartbeat monitoring
- **Latency trend monitoring with DataBuffer (3-sigma degradation detection)**
- **Modbus transaction logging with DataBuffer (ring buffer)**
- **Device event streaming with DataBuffer**
- **State management: AtomicU32/AtomicBool for counters/flags, Arc<RwLock<HashMap<>>> for device states, DataBuffer for批量数据**
- Exponential backoff with jitter for reconnection
- Transaction ID validation
- HTTP Management API with 5 endpoints
- TOML configuration with hot reload
- Comprehensive logging (INFO level, structured format)
- Graceful shutdown (5s timeout, drain in-flight requests)
- All 45+ acceptance criteria tests passing

### Must NOT Have (Guardrails)

- WebSocket, SSE, or push notification mechanisms
- Modbus RTU, OPC-UA, or protocols other than Modbus TCP
- Database persistence (Redis, PostgreSQL, SQLite)
- Caching layers (Redis, Memcached, in-memory beyond current state)
- Authentication/authorization systems (assume trusted network)
- Encryption/TLS for Modbus or HTTP (clear text only)
- Device auto-discovery (explicit TOML config only)
- Configuration validation beyond TOML parsing (no JSON Schema)
- Hot reload beyond config file reload (no code hot-swapping)
- Metrics collection beyond basic counter logs (no Prometheus exporter)
- Time-series data logging or historical data storage
- UI beyond HTTP management API (no web dashboard)
- REPL or debug console (HTTP API only)
- Rate limiting beyond per-device connection limits (no token bucket)
- Transaction rollback in batch operations (Modbus doesn't support transactions)
- Shared Modbus context objects across workers/threads

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring "user manually tests/confirms" are FORBIDDEN.

### Test Decision

- **Infrastructure exists**: NO (need to set up)
- **Automated tests**: YES (TDD)
- **Framework**: bun test (fast, integrates with Rust)
- **If TDD**: Each task follows RED (failing test) → GREEN (minimal impl) → REFACTOR

### QA Policy

Every task MUST include agent-executed QA scenarios (see TODO template below).
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Frontend/UI**: N/A (HTTP API tested with curl, JSON-RPC with client)
- **TUI/CLI**: N/A (daemon process with HTTP API)
- **API/Backend**: Use Bash (curl) — Send requests, assert status + response fields
- **Library/Module**: Use Bash (bun/node REPL) — Not applicable
- **Integration**: Use Bash (tcpdump, mock Modbus servers) — Capture network traffic, validate protocol

**Evidence Types**:
- curl response bodies (saved as .json)
- tcpdump captures (saved as .pcap, analyzed with tshark)
- Log files (saved as .log, parsed for expected entries)
- Process exit codes (saved as .txt)
- Timing data (saved as .csv, analyzed for SLA)

---

## Execution Strategy

### Parallel Execution Waves

> Maximize throughput by grouping independent tasks into parallel waves.
> Each wave completes before the next begins.
> Target: 5-8 tasks per wave. Fewer than 3 per wave (except final) = under-splitting.

```
Wave 1 (Start Immediately — project setup + core infrastructure):
├── Task 1: Cargo project + RoboPLC dependencies [quick]
├── Task 2: Project structure + module layout [quick]
├── Task 3: Configuration schema (TOML) + validation [quick]
├── Task 4: Message types (enum) + DataPolicy [quick]
├── Task 5: Shared Variables struct (Atomics + RwLock + DataBuffer) [quick]
├── Task 6: Basic Controller setup (main.rs) [quick]
└── Task 7: Logger setup (structured, daily rotation) [quick]

Wave 2 (After Wave 1 — Worker scaffolding):
├── Task 8: JSON-RPC Server Worker scaffolding [unspecified-high]
├── Task 9: Device Manager Worker scaffolding [unspecified-high]
├── Task 10: HTTP Management Worker scaffolding [unspecified-high]
├── Task 11: Config Loader Worker scaffolding [quick]
├── Task 12: Modbus Worker scaffolding (single device) [deep]
├── Task 13: Device profile system structure [deep]
└── Task 14: Latency Monitor Worker scaffolding [quick]

Wave 3 (After Wave 2 — Device Profiles + Validation):
├── Task 15: TOML config parsing + validation [unspecified-high]
├── Task 16: Addressing mode support (0-based/1-based) [deep]
├── Task 17: Byte order support (4 variants) [deep]
├── Task 18: Data type mapping (binrw structs) [deep]
├── Task 19: Device profile loading from TOML [unspecified-high]
├── Task 20: Config validation (unique IDs, address ranges) [unspecified-high]
└── Task 21: Hot config reload (diff + update) [deep]

Wave 4 (After Wave 3 — Modbus Worker + Critical Components):
├── Task 22: Modbus TCP client setup (per-device) [deep]
├── Task 23: Transaction ID validation [deep]
├── Task 24: Application-layer heartbeat system [deep]
├── Task 25: Latency trend monitoring (3-sigma) [deep]
├── Task 26: Exponential backoff with jitter [deep]
├── Task 27: Connection pool limits (per device) [deep]
└── Task 28: Timeout handling (libmodbus doubling) [quick]

Wave 5 (After Wave 4 — JSON-RPC Methods):
├── Task 29: JSON-RPC Server implementation (roboplc-rpc) [unspecified-high]
├── Task 30: JSON-RPC methods: Ping, GetVersion, GetDeviceList [quick]
├── Task 31: JSON-RPC methods: SetRegister, GetRegister [deep]
├── Task 32: JSON-RPC methods: WriteBatch, ReadBatch (partial success) [deep]
├── Task 33: JSON-RPC methods: MoveTo, GetStatus (robot-specific) [deep]
├── Task 34: Error responses (RPC error mapping) [quick]
└── Task 35: Hub message integration (JSON-RPC ↔ Device Manager) [unspecified-high]

Wave 6 (After Wave 5 — Device Manager + HTTP API):
├── Task 36: Device Manager routing logic [deep]
├── Task 37: Device registration + lifecycle management [unspecified-high]
├── Task 38: HTTP API: GET /api/devices, GET /api/devices/{id}/status [quick]
├── Task 39: HTTP API: GET /api/config, POST /api/config/reload [quick]
├── Task 40: HTTP API: POST /api/devices (add), DELETE /api/devices/{id} [unspecified-high]
├── Task 41: HTTP API: GET /api/health [quick]
└── Task 42: Config Loader implementation (notify + reload) [deep]

Wave 7 (After Wave 6 — Signal Handling + Shutdown):
├── Task 43: Signal handlers (SIGINT/SIGTERM) [quick]
├── Task 44: Graceful shutdown (drain in-flight, 5s timeout) [deep]
├── Task 45: Worker termination + cleanup [deep]
├── Task 46: Panic handler setup (roboplc::setup_panic) [quick]
└── Task 47: Simulated mode setup (development) [quick]

Wave 8 (After Wave 7 — Testing):
├── Task 48: Mock Modbus server for testing [unspecified-high]
├── Task 49: Functional tests (9 tests) [deep]
├── Task 50: Edge case tests (12 tests) [deep]
├── Task 51: Performance tests (4 tests) [deep]
├── Task 52: Reliability tests (5 tests) [deep]
├── Task 53: Error handling tests (10 tests) [deep]
└── Task 54: HTTP API tests (5 tests) [quick]

Wave 9 (After Wave 8 — Documentation):
├── Task 55: README (architecture, quick start, examples) [writing]
├── Task 56: Configuration guide (TOML schema, examples) [writing]
├── Task 57: Troubleshooting guide (common issues, solutions) [writing]
└── Task 58: Integration guide (deployment, monitoring) [writing]

Wave FINAL (After ALL tasks — independent review):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: Task 1 → Task 4 → Task 6 → Task 12 → Task 22 → Task 27 → Task 31 → Task 36 → Task 44 → Task 49 → F1-F4
Parallel Speedup: ~75% faster than sequential
Max Concurrent: 7 (Waves 1, 2, 3, 4, 5, 6, 8)
```

### Dependency Matrix (abbreviated — show ALL tasks in your generated plan)

- **1-7**: — — 8-14, 1
- **8-14**: 1-7 — 15-21, 22-28, 2
- **15-21**: 8-14 — 29-35, 36-42, 3
- **22-28**: 15-21 — 29-35, 4
- **29-35**: 22-28 — 36-42, 5
- **36-42**: 29-35 — 43-47, 6
- **43-47**: 36-42 — 48-54, 7
- **48-54**: 43-47 — 55-58, 8
- **55-58**: 48-54 — F1-F4, 9

> This is abbreviated for reference. YOUR generated plan must include the FULL matrix for ALL tasks.

### Agent Dispatch Summary

- **1**: **7** — T1-T7 → `quick`
- **2**: **7** — T8,T9,T10,T11,T13,T14 → `quick`, T12 → `deep`
- **3**: **7** — T15,T19,T20 → `unspecified-high`, T16,T17,T18,T21 → `deep`
- **4**: **7** — T23,T24,T25,T26,T27 → `deep`, T22,T28 → `quick`
- **5**: **7** — T29,T30,T34,T35 → `quick`, T31,T32,T33 → `deep`
- **6**: **7** — T36,T37,T40,T42 → `unspecified-high`, T38,T39,T41 → `quick`
- **7**: **5** — T43,T46,T47 → `quick`, T44,T45 → `deep`
- **8**: **7** — T48,T54 → `quick`, T49,T50,T51,T52,T53 → `deep`
- **9**: **4** — T55,T56,T57,T58 → `writing`
- **FINAL**: **4** — F1 → `oracle`, F2,F3 → `unspecified-high`, F4 → `deep`

---



## TODOs

> Implementation + Test = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.
> **A task WITHOUT QA Scenarios is INCOMPLETE. No exceptions.**

- [x] 1. **Cargo project + RoboPLC dependencies**

  **What to do**:
  - Initialize Cargo project with `cargo init`
  - Add dependencies to Cargo.toml:
    - `roboplc` (with features: controller, modbus, io)
    - `roboplc-rpc` (JSON-RPC 2.0 server)
    - `serde` and `serde_json` (for JSON-RPC)
    - `toml` and `serde_derive` (for config parsing)
    - `tracing` and `tracing-subscriber` (for structured logging)
    - `notify` (for config file watching)
    - `tokio` (async runtime, though RoboPLC handles this)
  - Set edition to "2021"
  - Configure release profile with opt-level 3

  **Must NOT do**:
  - Do NOT add WebSocket, SSE, or push notification dependencies
  - Do NOT add database dependencies (Redis, PostgreSQL, SQLite)
  - Do NOT add authentication/authorization libraries

  **Recommended Agent Profile**:
  > Select category + skills based on task domain. Justify each choice.
  - **Category**: `quick`
    - Reason: Simple Cargo.toml setup with dependency declarations, straightforward
  - **Skills**: None needed
  - **Skills Evaluated but Omitted**:
    - None: No specialized skills needed for Cargo.toml configuration

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2-7)
  - **Blocks**: None
  - **Blocked By**: None

  **References** (CRITICAL - Be Exhaustive):

  > The executor has NO context from your interview. References are their ONLY guide.
  > Each reference must answer: "What should I look at and WHY?"

  **Pattern References** (existing code to follow):
  - N/A: New project, no existing code

  **API/Type References** (contracts to implement against):
  - https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html - Cargo.toml dependency format

  **Test References** (testing patterns to follow):
  - N/A: No tests yet

  **External References** (libraries and frameworks):
  - https://github.com/roboplc/roboplc - RoboPLC repository and documentation
  - https://crates.io/crates/roboplc-rpc - roboplc-rpc crate documentation
  - https://docs.rs/tokio/latest/tokio/ - Tokio async runtime docs

  **WHY Each Reference Matters** (explain the relevance):
  - RoboPLC repo: Core framework documentation, examples, best practices
  - roboplc-rpc crate: JSON-RPC server implementation pattern
  - Cargo.toml spec: Correct dependency declaration syntax

  **Acceptance Criteria**:

  > **AGENT-EXECUTABLE VERIFICATION ONLY** — No human action permitted.
  > Every criterion MUST be verifiable by running a command or using a tool.

  **If TDD (tests enabled)**:
  - [ ] N/A: No test infrastructure yet

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
  Scenario: Cargo project initializes successfully
    Tool: Bash (cargo)
    Preconditions: None
    Steps:
      1. Run `cargo init --name roboplc-middleware`
      2. Verify Cargo.toml exists and has correct format
    Expected Result: Cargo.toml created with name = "roboplc-middleware", edition = "2021"
    Failure Indicators: cargo init fails, Cargo.toml not created, or contains syntax errors
    Evidence: .sisyphus/evidence/task-1-cargo-init.txt

  Scenario: Dependencies are correctly specified
    Tool: Bash (cargo check)
    Preconditions: Cargo.toml contains all required dependencies
    Steps:
      1. Run `cargo check`
      2. Verify all dependencies resolve without errors
    Expected Result: `cargo check` succeeds with no resolution errors
    Failure Indicators: Dependency resolution fails, version conflicts, or syntax errors
    Evidence: .sisyphus/evidence/task-1-deps-resolve.txt
  ```

  **Evidence to Capture**:
  - [ ] task-1-cargo-init.txt: Output of `cargo init`
  - [ ] task-1-deps-resolve.txt: Output of `cargo check`

  **Commit**: YES
  - Message: `feat(project): init cargo project + dependencies`
  - Files: Cargo.toml
  - Pre-commit: `cargo check`

- [x] 2. **Project structure + module layout**

  **What to do**:
  - Create directory structure:
    ```
    src/
      main.rs
      lib.rs
      messages.rs
      config.rs
      workers/
        mod.rs
        rpc_worker.rs
        manager.rs
        modbus_worker.rs
        http_worker.rs
        config_loader.rs
        latency_monitor.rs
      profiles/
        mod.rs
        device_profile.rs
      api.rs
    tests/
      functional/
      edge_cases/
      performance/
      reliability/
      error_handling/
    ```
  - Create module declarations in each mod.rs file
  - Add `pub mod` declarations to lib.rs

  **Must NOT do**:
  - Do NOT create UI/web frontend directories
  - Do NOT create database directories
  - Do NOT create auto-discovery modules

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Directory creation and module declarations are straightforward
  - **Skills**: None

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3-7)
  - **Blocks**: None
  - **Blocked By**: Task 1 (Cargo.toml must exist first)

  **References**:
  **Pattern References**:
  - RoboPLC examples: https://github.com/roboplc/roboplc/tree/main/examples - Module structure patterns

  **API/Type References**:
  - Rust book: https://doc.rust-lang.org/book/ch07-05-separating-modules-into-files.html - Module system

  **Acceptance Criteria**:
  - [ ] All directories created
  - [ ] All mod.rs files contain `pub mod` declarations
  - [ ] lib.rs imports all top-level modules

  **QA Scenarios**:
  ```
  Scenario: Directory structure is complete
    Tool: Bash (ls)
    Preconditions: Task 1 completed
    Steps:
      1. Run `find src -type f -name "*.rs"`
      2. Verify all expected files exist
    Expected Result: 15 files found (main.rs, lib.rs, messages.rs, config.rs, api.rs, workers/mod.rs + 6 workers, profiles/mod.rs, profiles/device_profile.rs)
    Failure Indicators: Missing files or directories
    Evidence: .sisyphus/evidence/task-2-structure.txt

  Scenario: Module declarations are valid
    Tool: Bash (cargo check)
    Preconditions: All mod.rs files have declarations
    Steps:
      1. Run `cargo check`
      2. Verify no "unresolved module" errors
    Expected Result: All modules resolve successfully
    Failure Indicators: Module resolution errors
    Evidence: .sisyphus/evidence/task-2-modules.txt
  ```

  **Evidence to Capture**:
  - [ ] task-2-structure.txt: File listing
  - [ ] task-2-modules.txt: cargo check output

  **Commit**: YES (group with Tasks 3-7)

- [ ] 3. **Configuration schema (TOML) + validation**

  **What to do**:
  - Define Config struct with serde Deserialize
  - Create TOML schema:
    ```toml
    [server]
    rpc_port = 8080
    http_port = 8081

    [logging]
    level = "info"
    file = "/var/log/roboplc-middleware.log"
    daily_rotation = true

    [[devices]]
    id = "plc1"
    type = "plc"
    address = "192.168.1.10"
    port = 502
    unit_id = 1
    addressing_mode = "zero_based"  # or "one_based"
    byte_order = "big_endian"      # or "little_endian", "little_endian_byte_swap", "mid_big"
    tcp_nodelay = true
    max_concurrent_ops = 3
    heartbeat_interval_sec = 30
    
    [[devices.register_mappings]]
    signal_name = "motor_speed"
    address = "h100"
    data_type = "u16"
    access = "rw"  # "read", "write", "rw"
    description = "Motor speed setpoint"
    ```
  - Implement Config::from_file(path: &str) -> Result<Self>
  - Add validation: unique device IDs, valid ports (0-65535), valid addressing_mode, valid byte_order

  **Must NOT do**:
  - Do NOT add JSON Schema validation (TOML parsing only)
  - Do NOT add authentication fields
  - Do NOT add encryption/TLS fields

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: TOML schema definition and serde deserialization are straightforward
  - **Skills**: None

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1-2, 4-7)
  - **Blocks**: Task 4 (Message types depend on Config)
  - **Blocked By**: Task 2 (module structure must exist)

  **References**:
  **Pattern References**:
  - serde-toml examples: https://serde.rs/ - TOML deserialization patterns

  **API/Type References**:
  - toml crate: https://docs.rs/toml/latest/toml/ - TOML parsing API

  **Acceptance Criteria**:
  - [ ] Config struct deserializes from TOML
  - [ ] Validation rejects duplicate device IDs
  - [ ] Validation rejects invalid ports
  - [ ] Validation rejects invalid addressing_mode and byte_order

  **QA Scenarios**:
  ```
  Scenario: Valid TOML config parses successfully
    Tool: Bash (cargo test)
    Preconditions: config.toml exists with valid data
    Steps:
      1. Create test config.toml
      2. Run `cargo test config_parse`
    Expected Result: Config deserializes without errors
    Failure Indicators: TOML parse errors, serde errors
    Evidence: .sisyphus/evidence/task-3-valid-config.txt

  Scenario: Duplicate device IDs are rejected
    Tool: Bash (cargo test)
    Preconditions: config.toml has duplicate device IDs
    Steps:
      1. Create config with duplicate IDs
      2. Attempt to parse
    Expected Result: Error "Duplicate device ID: plc1"
    Failure Indicators: Config with duplicate IDs accepted
    Evidence: .sisyphus/evidence/task-3-duplicate-ids.txt
  ```

  **Evidence to Capture**:
  - [ ] task-3-valid-config.txt: Test output
  - [ ] task-3-duplicate-ids.txt: Test output

  **Commit**: YES (group with Tasks 4-7)

---
## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, curl endpoint, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo test`. Review all changed files for: `as any`/`@ts-ignore`, empty catches, println! in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names (data/result/item/temp).
  Output: `Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration (features working together, not isolation). Test edge cases: empty state, invalid input, rapid actions. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance. Detect cross-task contamination: Task N touching Task M's files. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **1**: `feat(project): init cargo project + dependencies` — Cargo.toml, src/main.rs
- **2-7**: `feat(structure): project layout + core types` — src/lib.rs, src/messages.rs, src/config.rs
- **8-14**: `feat(workers): worker scaffolding` — src/workers/*.rs
- **15-21**: `feat(profiles): device profile system` — src/profiles/*.rs
- **22-28**: `feat(modbus): modbus worker + critical components` — src/workers/modbus_worker.rs
- **29-35**: `feat(rpc): json-rpc methods` — src/workers/rpc_worker.rs
- **36-42**: `feat(manager): device manager + http api` — src/workers/manager.rs, src/api.rs
- **43-47**: `feat(shutdown): signal handling + graceful shutdown` — src/main.rs
- **48-54**: `test(comprehensive): all test suites` — tests/*.rs
- **55-58**: `docs(guide): documentation` — README.md, docs/*.md

---

## Success Criteria

### Verification Commands
```bash
# Build and test
cargo build --release
cargo test --all

# Run middleware
cargo run --release --config config.toml

# Test JSON-RPC API
curl -X POST http://localhost:8080/jsonrpc -d '{"jsonrpc":"2.0","method":"Ping","params":[],"id":1}'
curl -X POST http://localhost:8080/jsonrpc -d '{"jsonrpc":"2.0","method":"GetDeviceList","params":[],"id":2}'
curl -X POST http://localhost:8080/jsonrpc -d '{"jsonrpc":"2.0","method":"SetRegister","params":["plc1","h100",42],"id":3}'

# Test HTTP API
curl http://localhost:8080/api/devices
curl http://localhost:8080/api/health
curl -X POST http://localhost:8080/api/config/reload

# Performance test
bash tests/perf/latency_test.sh  # Should report P95 < 10ms
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All 45+ tests passing
- [ ] Performance SLA met (P95 < 10ms)
- [ ] Graceful shutdown working
- [ ] Hot config reload working
- [ ] All edge cases tested
- [ ] Documentation complete
- [ ] Code review passed (clippy, fmt, audit)
