# Fix RPC Response Deadlock

## TL;DR

> **Quick Summary**: Fix critical deadlock where all device control RPC requests block forever due to broken response routing between RpcWorker and DeviceManager.
>
> **Deliverables**:
> - Message::TimeoutCleanup for explicit timeout cleanup
> - DeviceManager handles DeviceControlRequest via direct channel (bypass Hub)
> - RpcWorker removes unused pending_requests HashMap, sends timeout cleanup
> - 30-second timeout with explicit cleanup
> - Integration tests covering request-response flow, timeout, and concurrent requests
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 4 waves
> **Critical Path**: Message::TimeoutCleanup → DeviceManager direct channel → RpcWorker timeout cleanup → Tests

---

## Context

### Original Request
User discovered that `RpcHandler::send_device_control()` blocks forever on `response_rx.recv()` at rpc_worker.rs:429. This causes all device control RPC requests (GetStatus, SetRegister, GetRegister, MoveTo, ReadBatch, WriteBatch) to deadlock.

### Interview Summary
**Key Discussions**:
- **Fix Strategy**: Direct routing (similar to SystemStatus pattern) - RpcWorker sends DeviceControlRequest via direct channel to DeviceManager, bypassing Hub to avoid serialization issues with channel Sender
- **HashMap Type Conversion**: Convert DeviceResponseData to tuple (bool, JsonValue, Option<String>) when sending back to RpcWorker
- **Timeout Cleanup**: Explicit cleanup via new Message::TimeoutCleanup sent by RpcWorker when timeout occurs
- **Timeout Protection**: 30-second timeout using `recv_timeout(Duration::from_secs(30))`
- **Test Strategy**: TDD - write integration tests first, then implement

**Research Findings**:
- Metis identified critical serialization blocker: Message::DeviceControl derives DataPolicy and uses Hub (requires serialization), but channel Sender cannot be serialized
- Solution: Bypass Hub for DeviceControlRequest, use direct channel RpcWorker → DeviceManager
- DeviceManager already has `pending_requests: HashMap<u64, Sender<DeviceResponseData>>`
- Test infrastructure exists: tests/ directory with mock_modbus.rs, integration_tests.rs, e2e_tests.rs

### Metis Review
**Identified Gaps (addressed)**:
- **Serialization blocker**: Resolved by using direct routing (bypass Hub)
- **HashMap type mismatch**: Resolved by converting DeviceResponseData → tuple on send
- **Timeout cleanup**: Resolved by adding Message::TimeoutCleanup
- **Memory leak**: Resolved by explicit cleanup on timeout

**Edge Cases Addressed**:
- Timeout + late response: DeviceManager logs warning if channel closed
- Channel closed: Handled by existing error logging
- Concurrent requests: correlation_id ensures correct routing
- Memory leak: TimeoutCleanup removes stale entries

---

## Work Objectives

### Core Objective
Fix RPC response deadlock by implementing proper request-response routing through direct channel communication between RpcWorker and DeviceManager.

### Concrete Deliverables
- Modified Message enum with TimeoutCleanup variant
- Modified DeviceManager to handle DeviceControlRequest via direct channel
- Modified RpcWorker to remove unused pending_requests and send timeout cleanup
- Integration tests validating request-response flow, timeout, and concurrent requests

### Definition of Done
- [ ] `cargo test` passes all tests (including new integration tests)
- [ ] RPC requests complete successfully within timeout
- [ ] Timeout cleanup removes pending requests from HashMap
- [ ] Concurrent requests don't interfere with each other

### Must Have
- Fix response routing deadlock (no blocking recv() calls)
- Add 30-second timeout protection
- Explicit timeout cleanup to prevent memory leaks
- Integration tests for new flow

### Must NOT Have (Guardrails)
- Refactor unrelated parts of Message enum
- Add new message types beyond TimeoutCleanup
- Optimize HashMap performance
- Change correlation ID generation strategy
- Refactor DeviceManager's worker_map logic
- Touch HTTP API or config loading
- Add metrics or monitoring
- Clean up commented code unless related to fix

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (tests/ directory, mock_modbus.rs, Cargo.toml dev-deps)
- **Automated tests**: TDD (tests first, then implementation)
- **Framework**: Standard Rust #[test] + tokio for async
- **TDD Workflow**: Each task follows RED (failing test) → GREEN (minimal impl) → REFACTOR

### QA Policy
Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Integration Tests**: Use Bash (cargo test) — Run tests, assert pass/fail
- **RPC Server Tests**: Use Bash (curl/reqwest) — Send RPC requests, verify responses
- **Concurrency Tests**: Use Bash (parallel curl) — Send concurrent requests, verify correct routing

---

## Execution Strategy

### Parallel Execution Waves

> Maximize throughput by grouping independent tasks into parallel waves.
> Target: 3-4 tasks per wave.

```
Wave 1 (Start Immediately — message definition + test setup):
├── Task 1: Add Message::TimeoutCleanup to messages.rs [quick]
└── Task 2: Create integration test skeleton [quick]

Wave 2 (After Wave 1 — core modifications):
├── Task 3: Modify DeviceManager to handle DeviceControlRequest [deep]
├── Task 4: Modify DeviceManager to handle TimeoutCleanup [quick]
└── Task 5: Remove unused pending_requests from RpcWorker [quick]

Wave 3 (After Wave 2 — timeout + cleanup):
├── Task 6: Add timeout to RpcHandler::send_device_control [quick]
└── Task 7: Add timeout cleanup message sending [quick]

Wave 4 (After Wave 3 — TDD implementation):
├── Task 8: Write integration test for successful request-response [deep]
├── Task 9: Write integration test for timeout scenario [deep]
└── Task 10: Write integration test for concurrent requests [deep]

Wave 5 (After Wave 4 — verification):
├── Task 11: Run full test suite and verify all pass [quick]
└── Task 12: Manual QA - Send real RPC requests and verify [unspecified-high]

Critical Path: Task 1 → Task 3 → Task 5 → Task 6 → Task 8 → Task 11
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 3 (Waves 1, 3, 4)
```

### Dependency Matrix

- **1**: — — 3, 2
- **2**: — — 8-10, 1
- **3**: 1 — 4, 5, 1
- **4**: 1, 3 — 6, 7, 1
- **5**: 1, 3 — 6, 7, 1
- **6**: 1, 3, 4, 5 — 7, 1
- **7**: 1, 3, 4, 5, 6 — 1
- **8**: 2 — 11, 2
- **9**: 2 — 11, 2
- **10**: 2 — 11, 2
- **11**: 1, 8, 9, 10 — 12, 3
- **12**: 11 — — 4

### Agent Dispatch Summary

- **1**: **2** — T1 → `quick`, T2 → `quick`
- **2**: **3** — T3 → `deep`, T4 → `quick`, T5 → `quick`
- **3**: **2** — T6 → `quick`, T7 → `quick`
- **4**: **3** — T8 → `deep`, T9 → `deep`, T10 → `deep`
- **5**: **2** — T11 → `quick`, T12 → `unspecified-high`

---

## TODOs

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings` + `cargo test`. Review all changed files for AI slop.
  Output: `Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute ALL QA scenarios from EVERY task. Test cross-task integration. Save evidence to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 match. Check "Must NOT do" compliance.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **1**: `feat(messages): add TimeoutCleanup variant for explicit timeout cleanup` — src/messages.rs
- **2**: `feat(manager): handle DeviceControlRequest via direct channel` — src/workers/manager.rs
- **3**: `feat(rpc): add timeout and cleanup to RpcHandler` — src/workers/rpc_worker.rs
- **4**: `test(integration): add RPC response routing tests` — tests/integration_tests.rs

---

## Success Criteria

### Verification Commands
```bash
# Run all tests
cargo test
# Expected: All tests pass, including new integration tests

# Check compilation
cargo clippy -- -D warnings
# Expected: No warnings

# Manual RPC test (example)
echo '{"jsonrpc":"2.0","method":"ping","id":1}' | nc localhost 8080
# Expected: Valid JSON-RPC response
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass (cargo test)
- [ ] No clippy warnings
- [ ] Manual QA confirms RPC requests complete
