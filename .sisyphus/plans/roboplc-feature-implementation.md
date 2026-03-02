# RoboPLC Feature Implementation Plan

## TL;DR

> **Quick Summary**: Implement 8 missing/partial features for RoboPLC middleware to enable end-to-end JSON-RPC to Modbus communication with HTTP management API.
>
> **Deliverables**:
> - actix-web HTTP API with GET + POST endpoints
> - Channel-based RPC-to-Hub integration
> - DeviceControl forwarding to ModbusWorker instances
> - DeviceResponse routing to requesters
> - Actual Modbus operation execution (Read/Write/Batch)
> - OperationQueue integration for concurrency control
> - Config hot-reload worker that applies updates
> - Batch read/write operations
>
> **Estimated Effort**: Large (8 features, ~50 tasks, complex interdependencies)
> **Parallel Execution**: YES - 4 waves with 5-8 tasks per wave
> **Critical Path**: T6 (OperationQueue) → T12 (DeviceManager + correlation_id) → T20-T25 (RPC + Modbus ops + Response routing) → T30-T34 (POST endpoints + Config + Batch)

---

## Context

### Original Request
User requested detection of unimplemented features and creation of an implementation plan for 8 features:
1. HTTP API 模块（高级 Web 框架实现）
2. RPC 到 Hub 的消息传递
3. DeviceControl 消息转发到 ModbusWorker
4. DeviceResponse 消息路由回请求者
5. Modbus 操作的实际执行
6. 操作队列的使用
7. 配置更新的动态应用
8. 批量读写操作（ReadBatch, WriteBatch）

### Interview Summary

**Key Discussions**:
- HTTP framework choice: User chose **actix-web** instead of manual Tokio HTTP parsing
- Test strategy: User chose **TDD approach** (tests first)
- OperationQueue decision: User chose **integrate for concurrency control** (not remove)

**Research Findings**:
- Current implementation: ~20% complete - infrastructure exists but core features missing
- Critical path broken: RPC cannot access Hub, DeviceManager doesn't forward, ModbusWorker doesn't execute
- OperationQueue fully tested but marked dead_code - will be integrated
- ConfigLoader broadcasts ConfigUpdate but no worker applies it

**Audit Results**:
| Feature | Status | Completeness |
|---------|--------|-------------|
| 1. HTTP API | Partial | 30% - basic GET only |
| 2. RPC to Hub | Missing | 0% |
| 3. DeviceControl forwarding | Missing | 0% |
| 4. DeviceResponse routing | Missing | 0% |
| 5. Modbus operations | Missing | 0% |
| 6. OperationQueue | Implemented (unused) | 100% - dead_code |
| 7. Config updates | Partial | 50% - broadcast only |
| 8. Batch operations | Missing | 10% - interface only |

### Metis Review

**Identified Gaps (addressed):**
- **Gap 1: Circular dependency between Features 4 and 5** → **Resolved**: Implement as unified task T20-T25
- **Gap 2: Missing TDD acceptance criteria** → **Resolved**: Every task includes executable test commands
- **Gap 3: Implicit dependencies missed** → **Resolved**: Dependency graph includes all relationships (see Dependency Matrix)
- **Gap 4: Scope creep risk** → **Resolved**: Added explicit Must NOT Have guardrails

**Guardrails Applied (from Metis):**
- DO NOT add features beyond 8 identified (no auth, metrics, WebSocket, caching, etc.)
- DO NOT refactor unrelated code (keep existing http_worker.rs GET endpoints working)
- DO NOT invent new architectural patterns (use existing channel/correlation_id patterns)
- Use EXACT patterns from SystemStatus message (sender in message) for RPC integration
- Preserve existing JSON-RPC methods and response formats unchanged

---

## Work Objectives

### Core Objective
Enable complete end-to-end request/response flow from JSON-RPC/HTTP clients to Modbus devices via RoboPLC Hub messaging system.

### Concrete Deliverables
- `src/api.rs`: Complete actix-web implementation with GET + POST endpoints
- `Cargo.toml`: Added actix-web dependency
- `src/workers/rpc_worker.rs`: Channel-based Hub integration
- `src/workers/manager.rs`: DeviceControl forwarding + DeviceResponse routing
- `src/workers/modbus_worker.rs`: Modbus operation execution + OperationQueue integration
- `src/workers/config_updater.rs`: New worker to apply ConfigUpdate messages
- `src/messages.rs`: Added correlation_id to DeviceControl/DeviceResponse
- All features covered by TDD tests

### Definition of Done
```bash
# All tests pass
cargo test

# Build succeeds
cargo build

# End-to-end verification
cargo test integration::tests::rpc_to_modbus_roundtrip
cargo test integration::tests::http_to_modbus_roundtrip
```

### Must Have
- ✅ actix-web framework added and configured
- ✅ Channel-based RPC-to-Hub integration working
- ✅ DeviceControl messages forward to correct ModbusWorker
- ✅ DeviceResponse messages route to correct requester via correlation_id
- ✅ Modbus operations execute (ReadHolding, WriteSingle, WriteMultiple)
- ✅ OperationQueue integrated for concurrency control
- ✅ Config updates applied dynamically (no restart required)
- ✅ Batch operations (ReadBatch, WriteBatch) working
- ✅ TDD tests for all features
- ✅ Integration tests for end-to-end flows

### Must NOT Have (Guardrails)

**Scope Boundaries:**
- ❌ NO authentication/middleware (out of scope)
- ❌ NO rate limiting or caching (out of scope)
- ❌ NO WebSocket support (out of scope)
- ❌ NO metrics/telemetry (out of scope)
- ❌ NO API versioning (preserve existing endpoints)
- ❌ NO new JSON-RPC methods (keep existing methods)
- ❌ NO new config fields (use existing schema)

**AI Slop Prevention:**
- ❌ DO NOT over-engineer RPC channel solution (use SystemStatus pattern exactly)
- ❌ DO NOT add "nice-to-have" features during implementation
- ❌ DO NOT refactor unrelated code (keep existing http_worker.rs GET endpoints)
- ❌ DO NOT invent new message types (extend existing DeviceControl/DeviceResponse)
- ❌ DO NOT add comments explaining obvious code
- ❌ DO NOT use generic names (data, item, temp - use descriptive names)

**Existing Code Preservation:**
- ✅ KEEP existing http_worker.rs GET endpoint response formats
- ✅ KEEP existing JSON-RPC method signatures
- ✅ KEEP existing config.toml schema
- ✅ KEEP existing message types (only add fields, don't remove)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring "user manually tests/confirms" are FORBIDDEN.

### Test Decision
- **Infrastructure exists**: YES (cargo test, tempfile, reqwest, portpicker)
- **Automated tests**: TDD (RED-GREEN-REFACTOR)
- **Framework**: cargo test (native Rust testing)
- **If TDD**: Each task follows RED (failing test) → GREEN (minimal impl) → REFACTOR

### QA Policy
Every task MUST include agent-executed QA scenarios (see TODO template below).
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Backend/Worker Logic**: Use Bash (cargo test) — Run unit tests, verify assertions
- **Integration Tests**: Use Bash (cargo test) — Run end-to-end tests with mock devices
- **HTTP API**: Use Bash (curl) — Send requests, assert status codes and response JSON
- **Message Flow**: Use Bash (cargo test) — Verify channel operations and message routing

---

## Execution Strategy

### Parallel Execution Waves

> Maximize throughput by grouping independent tasks into parallel waves.
> Each wave completes before the next begins.
> Target: 5-8 tasks per wave. Fewer than 3 per wave (except final) = under-splitting.

```
Wave 0 (Start Immediately - dependencies and scaffolding):
├── T1: Add actix-web dependency [quick]
├── T2: Create HTTP API module structure [quick]
├── T3: Add correlation_id to Message types [quick]
└── T4: Create ConfigUpdater worker skeleton [quick]

Wave 1 (After Wave 0 - foundation + device tracking):
├── T5: DeviceManager - device_id → worker mapping [quick]
├── T6: ModbusWorker - integrate OperationQueue [deep]
├── T7: ModbusWorker - register with DeviceManager [quick]
├── T8: RpcWorker - channel architecture setup [deep]
└── T9: HttpWorker - migrate to actix-web GET endpoints [visual-engineering]

Wave 2 (After Wave 1 - core message flow, MAX PARALLEL):
├── T10: RpcWorker - send DeviceControl via channel [deep]
├── T11: RpcWorker - receive DeviceResponse and correlate [deep]
├── T12: DeviceManager - forward DeviceControl to ModbusWorker [deep]
├── T13: ModbusWorker - receive DeviceControl messages [deep]
├── T14: ModbusWorker - execute ReadHolding operation [deep]
├── T15: ModbusWorker - execute WriteSingle operation [deep]
├── T16: ModbusWorker - execute WriteMultiple operation [deep]
└── T17: ModbusWorker - send DeviceResponse messages [unspecified-high]

Wave 3 (After Wave 2 - response routing + config + batch):
├── T18: DeviceManager - route DeviceResponse via correlation_id [deep]
├── T19: ConfigUpdater - apply ConfigUpdate messages [unspecified-high]
├── T20: ModbusWorker - ReadBatch operation [deep]
├── T21: ModbusWorker - WriteBatch operation [deep]
├── T22: HTTP API - POST /api/devices/{id}/register [visual-engineering]
├── T23: HTTP API - POST /api/devices/{id}/batch [visual-engineering]
└── T24: HTTP API - POST /api/devices/{id}/move [visual-engineering]

Wave 4 (After Wave 3 - cleanup + final verification):
├── T25: Integration test - RPC to Modbus roundtrip [deep]
├── T26: Integration test - HTTP to Modbus roundtrip [deep]
├── T27: Integration test - multiple devices routing [deep]
├── T28: Remove dead_code markers from OperationQueue [quick]
├── T29: Clean up http_worker.rs old manual parsing [quick]
└── T30: Update README with new endpoints [writing]

Wave FINAL (After ALL tasks - independent review, 4 parallel):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA (unspecified-high)
└── F4: Scope fidelity check (deep)

Critical Path: T1 → T6 → T12 → T14 → T18 → F1-F4
Parallel Speedup: ~65% faster than sequential
Max Concurrent: 8 (Wave 2)
```

### Dependency Matrix

**Wave 0 (4 parallel, no dependencies):**
- T1: — — T2, T3, T4, T5-30, 1
- T2: — — T5-30, T9, 1
- T3: — — T5, T8, T10-11, 1
- T4: — — T19, 1

**Wave 1 (5 parallel, depends on Wave 0):**
- T5: T3 — — T12, T19, 2
- T6: T1 — — T7, T13-17, T20-21, 2
- T7: T6 — — T12, 2
- T8: T3 — — T10-11, 2
- T9: T1, T2 — — T22-24, 2

**Wave 2 (8 parallel, depends on Wave 1):**
- T10: T8 — — T25, 3
- T11: T8 — — T25, 3
- T12: T5, T7 — — T14-17, T25, 3
- T13: T6 — — T14-17, T20-21, 3
- T14: T13 — — T17, T20, T25, 3
- T15: T13 — — T17, T21, T25, 3
- T16: T13 — — T17, T21, T25, 3
- T17: T14-16 — — T18, T25, 3

**Wave 3 (7 parallel, depends on Wave 2):**
- T18: T17 — — T25-27, 4
- T19: T4, T5 — — T25, 4
- T20: T13, T14 — — T26, 4
- T21: T13, T15-16 — — T26, 4
- T22: T9 — — T26, 4
- T23: T9 — — T26, 4
- T24: T9 — — T26, 4

**Wave 4 (6 parallel, depends on Wave 3):**
- T25: T10-12, T17-18 — — F1-F4, 5
- T26: T20-24 — — F1-F4, 5
- T27: T12, T18 — — F1-F4, 5
- T28: T6 — — F1-F4, 5
- T29: T9 — — F1-F4, 5
- T30: All previous — — F1-F4, 5

**Wave FINAL (4 parallel, depends on ALL previous):**
- F1: T25-30 — — (final verification)
- F2: T25-30 — — (final verification)
- F3: T25-30 — — (final verification)
- F4: T25-30 — — (final verification)

> This is the complete dependency matrix for ALL tasks.

### Agent Dispatch Summary

- **Wave 0**: **4** — T1, T2, T3, T4 → all `quick`
- **Wave 1**: **5** — T5 → `quick`, T6 → `deep`, T7 → `quick`, T8 → `deep`, T9 → `visual-engineering`
- **Wave 2**: **8** — T10 → `deep`, T11 → `deep`, T12 → `deep`, T13 → `deep`, T14 → `deep`, T15 → `deep`, T16 → `deep`, T17 → `unspecified-high`
- **Wave 3**: **7** — T18 → `deep`, T19 → `unspecified-high`, T20 → `deep`, T21 → `deep`, T22 → `visual-engineering`, T23 → `visual-engineering`, T24 → `visual-engineering`
- **Wave 4**: **6** — T25 → `deep`, T26 → `deep`, T27 → `deep`, T28 → `quick`, T29 → `quick`, T30 → `writing`
- **FINAL**: **4** — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

> Implementation + Test = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.
> **A task WITHOUT QA Scenarios is INCOMPLETE. No exceptions.**

---

### Wave 0: Dependencies and Scaffolding

- [x] 1. Add actix-web dependency
  **What to do**: Add `actix-web = "4.4"` and `actix-rt = "2.9"` to Cargo.toml [dependencies]
  **Must NOT do**: DO NOT add authentication middleware
  **Acceptance**: cargo check passes, cargo build succeeds
  **QA**: Run cargo check, verify no errors; Run cargo build, verify compiles
  **Evidence**: .sisyphus/evidence/task-1-build.txt
  **Commit**: deps: add actix-web and actix-rt dependencies

- [x] 2. Create HTTP API module structure  
  **What to do**: Replace TODO in src/api.rs with actix-web module skeleton - import actix-web, create AppState with device_states, define handler functions (get_devices, get_device_by_id, get_health, get_config, reload_config)
  **Must NOT do**: DO NOT implement handler logic yet, DO NOT add POST endpoints
  **Acceptance**: src/api.rs compiles with handler signatures
  **QA**: cargo check --lib, grep for "pub async fn" handlers
  **Evidence**: .sisyphus/evidence/task-2-compile.txt
  **Commit**: api: create actix-web module structure with handler skeletons

- [x] 3. Add correlation_id to Message types
  **What to do**: Add `correlation_id: u64` field to Message::DeviceControl and Message::DeviceResponse variants in src/messages.rs
  **Must NOT do**: DO NOT change other Message variants
  **Acceptance**: Message enum compiles, fields exist
  **QA**: cargo test messages; grep for correlation_id in both variants
  **Evidence**: .sisyphus/evidence/task-3-correlation.txt
  **Commit**: messages: add correlation_id to DeviceControl and DeviceResponse

- [x] 4. Create ConfigUpdater worker skeleton
  **What to do**: Create src/workers/config_updater.rs with Worker trait skeleton - define ConfigUpdater struct, implement run() loop, register Hub for ConfigUpdate messages, stub handler that logs "applying config"
  **Must NOT do**: DO NOT implement actual config update logic
  **Acceptance**: Worker compiles, Hub registration stub exists
  **QA**: cargo test; grep for "context.hub().register" and "ConfigUpdate"
  **Evidence**: .sisyphus/evidence/task-4-compile.txt
  **Commit**: workers: create ConfigUpdater worker skeleton

---

### Wave 1: Foundation and Device Tracking

- [x] 5. DeviceManager - device_id → worker mapping
  **What to do**: Add `worker_map: HashMap<String, String>` to DeviceManager struct, implement static mapping from config in register_devices(), update manager.rs:127 TODO to use worker_map
  **Must NOT do**: DO NOT implement dynamic worker registration
  **Acceptance**: DeviceManager compiles with worker_map field
  **QA**: cargo test manager; grep for "worker_map: HashMap"
  **Evidence**: .sisyphus/evidence/task-5-mapping.txt
  **Commit**: manager: add device_id to worker_name mapping

- [x] 6. ModbusWorker - integrate OperationQueue
  **What to do**: Remove #[allow(dead_code)] from operation_queue field in ModbusWorker (line 239), integrate queue into run() method - queue DeviceControl messages, start operations via queue.complete()
  **Must NOT do**: DO NOT change queue logic, use existing implementation
  **Acceptance**: OperationQueue used in run(), no dead_code warnings
  **QA**: cargo test modbus_worker; grep for "operation_queue" usage in run()
  **Evidence**: .sisyphus/evidence/task-6-queue-integration.txt
  **Commit**: modbus_worker: integrate OperationQueue into execution flow

- [ ] 7. ModbusWorker - register with DeviceManager
  **What to do**: Add registration message to ModbusWorker startup, send message to DeviceManager to register device_id and worker_name
  **Must NOT do**: DO NOT implement dynamic registration
  **Acceptance**: Worker sends registration message on startup
  **QA**: cargo test; grep for registration message in worker code
  **Evidence**: .sisyphus/evidence/task-7-registration.txt
  **Commit**: modbus_worker: add device registration with DeviceManager

- [x] 8. RpcWorker - channel architecture setup
  **What to do**: Create channels between RpcWorker and RpcHandler - (device_control_tx, device_control_rx) for DeviceControl, (response_tx, response_rx) for DeviceResponse, add correlation_id generator, modify RpcHandler to send DeviceControl via channel instead of returning stub
  **Must NOT do**: DO NOT use SystemStatus pattern exactly (sender in message)
  **Acceptance**: Channels created, RpcHandler sends via channel
  **QA**: cargo test rpc_worker; grep for mpsc::channel usage
  **Evidence**: .sisyphus/evidence/task-8-channels.txt
  **Commit**: rpc_worker: implement channel-based Hub integration

- [x] 9. HttpWorker - migrate to actix-web GET endpoints
  **What to do**: Update HttpWorker to use actix-web HttpServer, bind to config.http_port, route GET endpoints to existing logic, preserve response formats
  **Must NOT do**: DO NOT change JSON response formats
  **Acceptance**: HttpWorker uses actix-web, GET endpoints work
  **QA**: cargo build; curl http://localhost:8081/api/devices, verify JSON response
  **Evidence**: .sisyphus/evidence/task-9-migration.txt
  **Commit**: http_worker: migrate to actix-web framework

---

### Wave 2: Core Message Flow

- [x] 10. RpcWorker - send DeviceControl via channel
  **What to do**: In RpcWorker's channel receiver loop, when DeviceControl received, add correlation_id, send via context.hub().send()
  **Must NOT do**: DO NOT block RPC worker
  **Acceptance**: DeviceControl messages sent with correlation_id
  **QA**: cargo test rpc_worker; grep for hub().send in channel handler
  **Evidence**: .sisyphus/evidence/task-10-send.txt
  **Commit**: rpc_worker: send DeviceControl messages via channel and Hub

- [x] 11. RpcWorker - receive DeviceResponse and correlate
  **What to do**: Register RpcWorker for DeviceResponse messages, match by correlation_id, send response via response_tx channel
  **Must NOT do**: DO NOT ignore correlation_id
  **Acceptance**: DeviceResponse correlated and sent to requester
  **QA**: cargo test rpc_worker; grep for correlation matching logic
  **Evidence**: .sisyphus/evidence/task-11-correlate.txt
  **Commit**: rpc_worker: receive DeviceResponse and route by correlation_id

- [x] 12. DeviceManager - forward DeviceControl to ModbusWorker
  **What to do**: Implement manager.rs:127 TODO - use worker_map to lookup ModbusWorker name, send DeviceControl via context.hub().send() to target worker
  **Must NOT do**: DO NOT hardcode device names
  **Acceptance**: DeviceControl forwarded to correct worker
  **QA**: cargo test manager; grep for hub().send with worker_name
  **Evidence**: .sisyphus/evidence/task-12-forward.txt
  **Commit**: manager: forward DeviceControl to appropriate ModbusWorker

- [x] 13. ModbusWorker - receive DeviceControl messages
  **What to do**: Register ModbusWorker for DeviceControl messages, push to operation_queue, trigger operation start if can_start()
  **Must NOT do**: DO NOT execute operations yet
  **Acceptance**: ModbusWorker receives and queues operations
  **QA**: cargo test modbus_worker; grep for DeviceControl message handling
  **Evidence**: .sisyphus/evidence/task-13-receive.txt
  **Commit**: modbus_worker: receive DeviceControl and queue operations

- [x] 14. ModbusWorker - execute ReadHolding operation
  **What to do**: Implement ModbusOp::ReadHolding execution - use ModbusClient, call client.read_holding_registers(), handle errors, send DeviceResponse
  **Must NOT do**: DO NOT add retry logic
  **Acceptance**: ReadHolding executes, DeviceResponse sent
  **QA**: cargo test modbus_worker; grep for ReadHolding execution
  **Evidence**: .sisyphus/evidence/task-14-read-holding.txt
  **Commit**: modbus_worker: implement ReadHolding operation execution

- [x] 15. ModbusWorker - execute WriteSingle operation
  **What to do**: Implement ModbusOp::WriteSingle execution - use ModbusClient, call client.write_single_register(), handle errors, send DeviceResponse
  **Must NOT do**: DO NOT add retry logic
  **Acceptance**: WriteSingle executes, DeviceResponse sent
  **QA**: cargo test modbus_worker; grep for WriteSingle execution
  **Evidence**: .sisyphus/evidence/task-15-write-single.txt
  **Commit**: modbus_worker: implement WriteSingle operation execution

- [x] 16. ModbusWorker - execute WriteMultiple operation
  **What to do**: Implement ModbusOp::WriteMultiple execution - use ModbusClient, call client.write_multiple_registers(), handle errors, send DeviceResponse
  **Must NOT do**: DO NOT add retry logic
  **Acceptance**: WriteMultiple executes, DeviceResponse sent
  **QA**: cargo test modbus_worker; grep for WriteMultiple execution
  **Evidence**: .sisyphus/evidence/task-16-write-multiple.txt
  **Commit**: modbus_worker: implement WriteMultiple operation execution

- [x] 17. ModbusWorker - send DeviceResponse messages
  **What to do**: After operation execution, send DeviceResponse via context.hub().send() with correlation_id, success, data, error
  **Must NOT do**: DO NOT ignore errors
  **Acceptance**: DeviceResponse sent after all operations
  **QA**: cargo test modbus_worker; grep for DeviceResponse hub().send
  **Evidence**: .sisyphus/evidence/task-17-response.txt
  **Commit**: modbus_worker: send DeviceResponse messages after operations

---

### Wave 3: Response Routing, Config, and Batch Operations

- [x] 18. DeviceManager - route DeviceResponse via correlation_id
  **What to do**: Implement manager.rs:141 TODO - lookup pending request by correlation_id, send via response_tx channel, remove from pending_requests
  **Must NOT do**: DO NOT ignore unmatched responses
  **Acceptance**: DeviceResponse routed to correct requester
  **QA**: cargo test manager; grep for correlation_id matching
  **Evidence**: .sisyphus/evidence/task-18-route.txt
  **Commit**: manager: route DeviceResponse by correlation_id

- [x] 19. ConfigUpdater - apply ConfigUpdate messages
  **What to do**: Implement config update logic - parse ConfigUpdate, update device list gracefully, preserve existing connections, reload workers if needed
  **Must NOT do**: DO NOT restart system, DO NOT break connections
  **Acceptance**: Config updates applied dynamically
  **QA**: cargo test; grep for config reload logic
  **Evidence**: .sisyphus/evidence/task-19-config-update.txt
  **Commit**: config_updater: apply ConfigUpdate messages dynamically

- [x] 20. ModbusWorker - ReadBatch operation
  **What to do**: Implement batch read - iterate addresses, queue multiple ReadHolding ops, collect results, return as array
  **Must NOT do**: DO NOT batch-split automatically
  **Acceptance**: ReadBatch executes multiple reads
  **QA**: cargo test modbus_worker; grep for ReadBatch handling
  **Evidence**: .sisyphus/evidence/task-20-read-batch.txt
  **Commit**: modbus_worker: implement ReadBatch operation

- [x] 21. ModbusWorker - WriteBatch operation
  **What to do**: Implement batch write - iterate (address, value) pairs, queue multiple WriteSingle/WriteMultiple ops, track success
  **Must NOT do**: DO NOT batch-split automatically
  **Acceptance**: WriteBatch executes multiple writes
  **QA**: cargo test modbus_worker; grep for WriteBatch handling
  **Evidence**: .sisyphus/evidence/task-21-write-batch.txt
  **Commit**: modbus_worker: implement WriteBatch operation

- [x] 22. HTTP API - POST /api/devices/{id}/register
  **What to do**: Add POST endpoint in src/api.rs that sends DeviceControl message with SetRegister operation, parse JSON body, validate device_id
  **Must NOT do**: DO NOT add authentication
  **Acceptance**: POST /api/devices/{id}/register works
  **QA**: curl -X POST with JSON body; verify 200 response
  **Evidence**: .sisyphus/evidence/task-22-post-register.txt
  **Commit**: api: add POST /api/devices/{id}/register endpoint

- [x] 23. HTTP API - POST /api/devices/{id}/batch
  **What to do**: Add POST endpoint for batch operations - support ReadBatch and WriteBatch, parse JSON array, send DeviceControl
  **Must NOT do**: DO NOT add authentication
  **Acceptance**: POST /api/devices/{id}/batch works
  **QA**: curl -X POST with batch JSON; verify 200 response
  **Evidence**: .sisyphus/evidence/task-23-post-batch.txt
  **Commit**: api: add POST /api/devices/{id}/batch endpoint

- [x] 24. HTTP API - POST /api/devices/{id}/move
  **What to do**: Add POST endpoint for move operation - send DeviceControl with MoveTo operation, parse position parameter
  **Must NOT do**: DO NOT add authentication
  **Acceptance**: POST /api/devices/{id}/move works
  **QA**: curl -X POST with position; verify 200 response
  **Evidence**: .sisyphus/evidence/task-24-post-move.txt
  **Commit**: api: add POST /api/devices/{id}/move endpoint

---

### Wave 4: Cleanup and Final Verification

- [x] 25. Integration test - RPC to Modbus roundtrip
  **What to do**: Create integration test - send JSON-RPC request, verify DeviceControl sent, ModbusWorker executes, DeviceResponse routed back
  **Must NOT do**: DO NOT use mock Modbus devices
  **Acceptance**: End-to-end RPC flow works
  **QA**: cargo test integration::tests::rpc_to_modbus_roundtrip
  **Evidence**: .sisyphus/evidence/task-25-integration-rpc.txt
  **Commit**: tests: add RPC to Modbus roundtrip integration test

- [x] 26. Integration test - HTTP to Modbus roundtrip
  **What to do**: Create integration test - curl POST to HTTP API, verify DeviceControl sent, operation executed, response received
  **Must NOT do**: DO NOT use mock Modbus devices
  **Acceptance**: End-to-end HTTP flow works
  **QA**: cargo test integration::tests::http_to_modbus_roundtrip
  **Evidence**: .sisyphus/evidence/task-26-integration-http.txt
  **Commit**: tests: add HTTP to Modbus roundtrip integration test

- [x] 27. Integration test - multiple devices routing
  **What to do**: Create integration test - configure 2 devices, send requests to both, verify they route to correct workers
  **Must NOT do**: DO NOT test single device only
  **Acceptance**: Multiple devices route correctly
  **QA**: cargo test integration::tests::multiple_devices_routing
  **Evidence**: .sisyphus/evidence/task-27-integration-multi.txt
  **Commit**: tests: add multiple devices routing integration test

- [x] 28. Remove dead_code markers from OperationQueue
  **What to do**: Remove #[allow(dead_code)] from operation_queue field and all OperationQueue usage
  **Must NOT do**: DO NOT remove queue logic
  **Acceptance**: No dead_code warnings
  **QA**: cargo clippy; verify no dead_code warnings for OperationQueue
  **Evidence**: .sisyphus/evidence/task-28-deadcode.txt
  **Commit**: modbus_worker: remove dead_code markers from OperationQueue

- [x] 29. Clean up http_worker.rs old manual parsing
  **What to do**: Remove manual HTTP parsing code (lines 20-77 approximately) since actix-web now handles it
  **Must NOT do**: DO NOT remove actix-web setup
  **Acceptance**: No manual parsing code
  **QA**: grep for "starts_with("GET"; verify minimal matches
  **Evidence**: .sisyphus/evidence/task-29-cleanup.txt
  **Commit**: http_worker: remove manual HTTP parsing code

- [x] 30. Update README with new endpoints
  **What to do**: Update README.md API endpoints section with POST endpoints (/api/devices/{id}/register, /api/devices/{id}/batch, /api/devices/{id}/move)
  **Must NOT do**: DO NOT change GET endpoint documentation
  **Acceptance**: README documents all endpoints
  **QA**: grep README for each POST endpoint
  **Evidence**: .sisyphus/evidence/task-30-readme.txt
  **Commit**: docs: update README with POST endpoints

