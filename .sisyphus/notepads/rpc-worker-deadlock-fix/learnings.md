
## Wave 2: Tokio Runtime Integration (2026-03-03)

### Key Implementation Decisions

1. **Tokio Runtime Pattern**: Used the same pattern as HttpWorker - spawn a thread with `std::thread::spawn`, create tokio runtime inside, and run async code with `rt.block_on()`. This keeps `blocking = true` in worker_opts for RoboPLC compatibility.

2. **Channel Architecture Change**:
   - `ResponseSender` changed from `std::sync::mpsc::Sender` to `tokio::sync::oneshot::Sender`
   - `device_control_tx` changed from `std::sync::mpsc::channel` to `tokio::sync::mpsc::channel(100)`
   - Buffer size 100 chosen for mpsc channel to handle burst traffic

3. **RpcServer Type Complexity**: The `RpcServer` struct has 4 generic parameters (`'a`, `RPC`, `M`, `SRC`, `R`). Rather than trying to pass it around, we create it fresh in each connection handler with `RpcServer::new(handler.clone())`.

4. **Blocking in Sync Context**: Since `RpcServerHandler::handle_call` is sync (not async), we use:
   - `mpsc::Sender::blocking_send()` to send requests from sync context
   - `oneshot::Receiver::blocking_recv()` to wait for responses

5. **Bridging std and tokio channels**: The Message type expects `std::sync::mpsc::Sender<DeviceResponseData>`, so in `run_async_server` we:
   - Create a `std::sync::mpsc::channel` for the Message
   - Use `tokio::task::spawn_blocking` to wait on the std channel
   - Forward response to the oneshot channel

### Code Patterns

```rust
// Creating server in connection handler (avoids complex generics)
let server = RpcServer::new((*handler).clone());
if let Some(response_payload) = server.handle_request_payload::<Json>(&request_payload, addr) {
    timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await??;
}
```

```rust
// Bridging std::sync::mpsc to tokio::sync::oneshot
let (std_tx, std_rx) = std::sync::mpsc::channel();
// ... send Message with std_tx ...
let respond_to = request.respond_to;
tokio::task::spawn_blocking(move || {
    match std_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(response) => { let _ = respond_to.send(response); }
        Err(_) => { tracing::warn!("timeout"); }
    }
});
```

---

## Task 5: Async Architecture Design (2026-03-03)

### Design Document
Created: `.sisyphus/research/rpc-worker-async-design.md`

### Architecture Overview
- **Pattern**: HttpWorker-style - spawn dedicated thread with tokio runtime inside sync worker
- **Keep**: `blocking = true` in worker_opts (RoboPLC compatibility)
- **Runtime**: tokio::runtime::Builder::new_multi_thread().enable_all().build()

### Channel Design
| Before | After |
|--------|-------|
| std::sync::mpsc::Sender | tokio::sync::mpsc::Sender (device_control) |
| std::channel (response) | tokio::sync::oneshot::Sender |
| std::net::TcpListener | tokio::net::TcpListener |

### Main Loop (tokio::select!)
```rust
loop {
    tokio::select! {
        result = listener.accept() => { /* handle connection */ }
        Some(request) = device_control_rx.recv() => { /* forward to hub */ }
        _ = shutdown_rx.recv() => { break; }
        _ = tokio::time::sleep(Duration::from_secs(10)) => { /* cleanup */ }
    }
}
```

### Timeout Handling
- Replace `recv_timeout(30s)` with `tokio::time::timeout(30s, receiver).await`
- Use `tokio::time::timeout(5s, stream.read(...))` for I/O

### Cleanup Logic
- Track pending requests in `Arc<tokio::sync::Mutex<HashMap<u64, PendingRequest>>>`
- Periodic cleanup every 10 seconds
- Remove requests older than 35 seconds (slightly longer than 30s timeout)

- Remove requests older than 35 seconds (slightly longer than 30s timeout)

---

## Task 4: Architecture Analysis Findings (2026-03-03)

### Document Created
- `.sisyphus/research/rpc-worker-current-architecture.md`

### Key Blocking Points Identified

1. **Primary: recv_timeout(30s)** at rpc_worker.rs:435
   - BLOCKS FOR 30 SECONDS waiting for response
   - During this time, main loop cannot process new connections or responses
   - Only ONE RPC request can be processed at a time per connection

2. **Secondary**:
   - `listener.accept()` - set non-blocking, OK
   - `stream.read()` - 200ms timeout, OK
   - `device_control_rx.try_recv()` - non-blocking, OK

### Deadlock Scenario
The deadlock occurs because:
1. recv_timeout(30s) in RpcHandler blocks the entire handle_call execution
2. While blocked, the main loop cannot accept new connections
3. Cannot process DeviceResponse messages coming back from DeviceManager
4. If multiple concurrent requests: only first one gets processed, others timeout

### Message Flow (Current)
```
Client → RpcHandler::send_device_control() → device_control_tx.send()
     → Main loop: device_control_rx.try_recv() → Hub.send(DeviceControl)
     → DeviceManager: stores sender in pending_requests[correlation_id]
     → ModbusWorker: processes request → Hub.send(DeviceResponse)
     → DeviceManager: looks up sender → sends response
     → Main loop: receives via try_recv → forwards to original response_tx
     → RpcHandler: receives via recv_timeout() → returns to client
```

### Relationship with DeviceManager
- RpcWorker sends DeviceControl via Hub with respond_to channel
- DeviceManager stores respond_to in pending_requests HashMap
- When DeviceResponse arrives, DeviceManager looks up sender and forwards
- TimeoutCleanup message removes entry from pending_requests

### Issues Summary
| Issue | Severity | Description |
|-------|----------|-------------|
| recv_timeout blocks handler | HIGH | 30s blocking prevents concurrent requests |
| Main loop blocked during recv | HIGH | Cannot accept new connections |
| Response lost after timeout | MEDIUM | Race between timeout and cleanup |
| Single-threaded design | HIGH | No parallelism for concurrent RPCs |
| Channel sender leak | MEDIUM | pending_requests entry lives until cleanup |
---
## Wave 2 Implementation Completed (2026-03-03)

### Implementation Summary

All Tasks 6-10 completed successfully:
- Task 6: Modified RpcWorker to async architecture (HttpWorker pattern)
- Task 7: Replaced std::sync::mpsc with tokio::sync::mpsc for device_control
- Task 8: Replaced blocking recv with oneshot channels
- Task 9: Implemented tokio runtime in RpcWorker (spawn thread + rt.block_on)
- Task 10: Updated imports and dependencies

### Key Changes Made

1. **HttpWorker Pattern Applied**: 
   - `std::thread::spawn(move || { rt.block_on(async_server_loop(...)) })`
   - Main worker loop: `while context.is_online() { sleep(1s); }`
   - Shutdown via oneshot channel: `shutdown_tx.send(())`

2. **Channel Architecture**:
   - `tokio::sync::mpsc::channel(100)` for device_control
   - `tokio::sync::oneshot::channel()` for responses
   - Bridge to std::sync::mpsc via `spawn_blocking` for Message compatibility

3. **RpcHandler Clone**: Manual `impl Clone for RpcHandler` required because Hub doesn't derive Clone automatically.

4. **PendingRequest**: Cannot derive Clone (oneshot::Sender doesn't implement Clone). Tracking uses `Arc<Mutex<HashMap>>`.

5. **tokio::select! Pattern**:
   ```rust
   tokio::select! {
       accept_result = listener.accept() => { /* spawn handler */ }
       Some(request) = async { device_control_rx.recv().await } => { /* forward to Hub */ }
       _ = &mut shutdown_rx => { break; }
       _ = tokio::time::sleep(Duration::from_secs(10)) => { /* cleanup */ }
   }
   ```

6. **Async I/O**: `tokio::net::TcpListener`, `tokio::io::AsyncReadExt/AsyncWriteExt`, `tokio::time::timeout`

### Build Verification
- `cargo build` - PASSED (0 errors, warnings only)
- `cargo test --lib` - PASSED (47 tests)
- `cargo clippy` - PASSED (warnings only, no errors)

### Remaining Work
- Wave 3: Integration testing and verification

---

## Final Code Review Checklist (Wave 5, Tasks F1-F4)

### F1: Code Review Results

| Category | Status | Notes |
|----------|--------|-------|
| Concurrency | ✅ PASS | tokio::select! handles concurrent accept, recv, shutdown, cleanup |
| Error Handling | ✅ PASS | All errors logged and returned as RpcResultType::Error |
| Timeouts | ✅ PASS | 30s request, 5s I/O, 500ms read, 10s cleanup interval |
| Resource Management | ✅ PASS | Pending requests tracked, cleaned up every 10s, 35s timeout |
| Thread Safety | ✅ PASS | Arc<Mutex<HashMap>>, atomic correlation counter |
| Documentation | ✅ PASS | Chinese comments throughout, module-level docs at top |

**Issues Found:**
1. **Dead code warning**: `PendingRequest.correlation_id` field never read (minor - could prefix with `_`) 
2. **Pre-existing warnings**: Other files have unused imports, variables (not related to this PR)

### F2: Performance Testing Results
- Channel throughput: 100+ messages/second (tested in performance_tests)
- Concurrent request handling: 50+ parallel requests supported
- Stress test: 200 concurrent requests handled successfully
- Memory: 1000 pending requests inserted in <10ms

### F3: Integration Testing Results
- All 8 e2e tests pass
- All 13 async_rpc_tests pass (unit + integration + performance)
- RPC to Modbus roundtrip verified with mock server
- Correlation ID preservation verified

### F4: Documentation Review
- Module-level documentation at top of rpc_worker.rs
- Section comments (第一部分, 第二部分, etc.) for structure
- Chinese documentation for main components
- Public types have doc comments (DeviceControlRequest, ResponseSender)

### Final Verification Commands
```bash
cargo test        # PASSED - all tests pass
cargo clippy      # PASSED - warnings only, no errors
cargo doc --no-deps  # Build successful
```

### Known Limitations
- Dead code warning for `correlation_id` field (cosmetic)

---

## Final Wave Verification (F1-F4) - 2026-03-03

### Verification Results

| Check | Command | Result |
|-------|---------|--------|
| Tests | `cargo test` | ✅ PASSED - 127 tests |
| Build | `cargo build` | ✅ PASSED - 0 errors |
| Clippy | `cargo clippy` | ✅ PASSED - warnings only, no errors |

### Test Breakdown
- Unit tests (lib): 62 passed
- Integration tests: 12 passed (async_rpc_tests)
- E2E tests: 20 passed
- Functional tests: 9+5+7+8+4 = 33 passed
- Doc tests: 2 passed
- **Total: 127 tests passed, 0 failed**

### Clippy Warnings (non-blocking)
- Dead code: `PendingRequest.correlation_id` - cosmetic, field reserved for future
- Unused imports in other modules (pre-existing)
- Type complexity warnings in config_loader
- All other warnings are pre-existing and not related to this refactoring

### Code Review Summary
- **Total lines in rpc_worker.rs**: 978
- **Key architectural changes**:
  1. HttpWorker pattern with tokio runtime
  2. tokio::sync::mpsc for device_control channel
  3. tokio::sync::oneshot for response channels
  4. tokio::select! for concurrent handling
  5. tokio::time::timeout for all blocking operations

### Test Coverage
- Channel tests: mpsc send/receive, ordering, buffer size
- Timeout tests: oneshot timeout, cleanup logic
- Concurrency tests: parallel requests, response routing
- Integration tests: RPC to Modbus roundtrip

### Technical Debt
- Minor: `_correlation_id` prefix could silence dead code warning (optional)
- Pre-existing: Various unused imports/variables in other modules
- No blocking issues remain

### Conclusion
**Refactoring complete and verified. All acceptance criteria met.**
