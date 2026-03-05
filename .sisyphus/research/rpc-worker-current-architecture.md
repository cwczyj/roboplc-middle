# RpcWorker Current Architecture Analysis

**Document Date:** March 3, 2026  
**Task:** Task 4 - Document current RpcWorker architecture, identify blocking points and message flow

---

## 1. Component Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              RpcWorker                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                        Main Loop (run)                              │    │
│  │  ┌──────────────┐  ┌────────────────┐  ┌────────────────────────┐  │    │
│  │  │ TCP Listener │─▶│ Accept Connection│─▶│ Handle JSON-RPC    │  │    │
│  │  │ (non-block)  │  │ (with 200ms timeout)│  │ Request           │  │    │
│  │  └──────────────┘  └────────────────┘  └────────────────────────┘  │    │
│  │         │                    │                       │            │    │
│  │         ▼                    ▼                       ▼            │    │
│  │  ┌─────────────────────────────────────────────────────────┐     │    │
│  │  │            DeviceControlResponse Handler                │     │    │
│  │  │         (device_control_rx.try_recv loop)              │     │    │
│  │  └─────────────────────────────────────────────────────────┘     │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                    │                                          │
│                                    ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                      RpcHandler (per connection)                    │    │
│  │  ┌──────────────────────────────────────────────────────────────┐  │    │
│  │  │ send_device_control()                                        │  │    │
│  │  │  1. Create channel for response (response_tx, response_rx)   │  │    │
│  │  │  2. Send DeviceControlRequest to device_control_tx           │  │    │
│  │  │  3. BLOCK: response_rx.recv_timeout(30s) ◄─── PRIMARY BLOCK  │  │    │
│  │  └──────────────────────────────────────────────────────────────┘  │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
         │                                            ▲
         │ device_control_tx                         │
         ▼                                            │ response_tx
┌─────────────────────────────────────────────────────────────────────────────┐
│                         DeviceManager                                        │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                       Main Loop (run)                                │    │
│  │  for msg in client {  // Hub messages                               │    │
│  │    match msg {                                                        │    │
│  │      DeviceControl:                                                  │    │
│  │        - Store respond_to in pending_requests[correlation_id]        │    │
│  │        - Forward to ModbusWorker via Hub                           │    │
│  │      DeviceResponse:                                                 │    │
│  │        - Lookup sender from pending_requests[correlation_id]        │    │
│  │        - Send response via sender                                   │    │
│  │      TimeoutCleanup:                                                 │    │
│  │        - Remove from pending_requests                               │    │
│  │    }                                                                 │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
         │
         │ Hub (Message::DeviceControl, Message::DeviceResponse)
         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         ModbusWorker                                         │
│  - Receives DeviceControl messages                                         │
│  - Executes Modbus operations                                               │
│  - Sends DeviceResponse messages back via Hub                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Message Flow Diagram

### Normal Flow (Success Case)

```
Client                    RpcWorker              Hub              DeviceManager       ModbusWorker
  │                         │                     │                    │                  │
  │─── TCP Request ───────▶│                     │                    │                  │
  │                         │                     │                    │                  │
  │                         │──(1)────────────────▶│                    │                  │
  │                         │  DeviceControl       │                    │                  │
  │                         │  (with respond_to)    │                    │                  │
  │                         │                     │──(2)───────────────▶│                  │
  │                         │                     │  DeviceControl     │                  │
  │                         │                     │                    │──(3)────────────▶│
  │                         │                     │                    │    DeviceControl │
  │                         │                     │                    │                  │
  │                         │                     │                    │◀──(4)────────────│
  │                         │                     │                    │  DeviceResponse  │
  │                         │◀───(5)──────────────│                    │                  │
  │                         │  DeviceResponse      │                    │                  │
  │                         │                     │                    │                  │
  │◀─── TCP Response ──────│                     │                    │                  │
```

**Step-by-step:**

1. **RpcWorker receives TCP request** → `listener.accept()` → `server.handle_request_payload()`
2. **RpcHandler::send_device_control()** creates:
   - `channel()` for response (response_tx, response_rx)
   - `DeviceControlRequest` with `respond_to: response_tx`
3. **RpcWorker main loop** picks up request via `device_control_rx.try_recv()` 
   - Sends `Message::DeviceControl{respond_to: Some(response_tx), ...}` to Hub
4. **DeviceManager** receives message:
   - Stores `response_tx` in `pending_requests[correlation_id]`
   - Forwards to ModbusWorker via Hub
5. **ModbusWorker** processes request → sends `DeviceResponse{correlation_id, ...}` to Hub
6. **DeviceManager** receives response:
   - Looks up sender in `pending_requests.remove(correlation_id)`
   - Sends response data via sender
7. **RpcWorker main loop** receives via `device_control_rx.try_recv()` → forwards to original `response_tx`
8. **RpcHandler::send_device_control()** receives via `response_rx.recv_timeout()` → returns to client

---

## 3. Blocking Points Analysis

### Primary Blocking Point: recv_timeout (Line 435-465 in rpc_worker.rs)

```rust
// From rpc_worker.rs, lines 435-465
match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
    Ok((success, data, error)) => {
        // Response received - process normally
    }
    Err(RecvTimeoutError::Timeout) => {
        // TIMEOUT: 30 seconds elapsed
        tracing::warn!(correlation_id, "Request timed out, sending cleanup");
        // Send TimeoutCleanup to DeviceManager
        self.hub.send(Message::TimeoutCleanup { correlation_id });
        Ok(RpcResultType::Error { error: "Request timed out".to_string() })
    }
    Err(RecvTimeoutError::Disconnected) => {
        // Channel disconnected
    }
}
```

**Impact:**
- **BLOCKS FOR 30 SECONDS** waiting for response
- During this time, the RpcHandler cannot process ANY new connections
- Only ONE RPC request can be processed at a time per connection
- If client sends multiple requests, they queue up or timeout

### Secondary Blocking Points

| Location | Type | Timeout | Impact |
|----------|------|---------|--------|
| `listener.accept()` | Non-blocking | N/A | Set non-blocking via `set_nonblocking(true)`, OK |
| `stream.read()` | Blocking | 200ms | Per-connection, OK |
| `device_control_rx.try_recv()` | Non-blocking | N/A | Non-blocking, OK |
| `thread::sleep(50ms)` | Sleep | 50ms | In WouldBlock case, OK |

---

## 4. The Deadlock Scenario

### Sequence Diagram

```
Time    RpcWorker Main Loop              RpcHandler                  DeviceManager
 │              │                           │                            │
T=0      ┌────────────────────┐           │                            │
         │ Accept connection  │           │                            │
         └─────────┬──────────┘           │                            │
                   │                      │                            │
                   ▼                      ▼                            │
         ┌─────────────────────────────────────────────────┐           │
         │ handle_call() → send_device_control()          │           │
         │                                                 │           │
         │ 1. channel() → response_tx, response_rx       │           │
         │ 2. device_control_tx.send(request)             │           │
         │ 3. ◄── BLOCKS HERE ── recv_timeout(30s)        │           │
         │           │                                     │           │
         │           │  Main loop CANNOT process          │           │
         │           │  new connections or responses!     │           │
         │           │                                     │           │
T=30s    │           │ (timeout expires)                  │           │
         │           ▼                                     │           │
         │   TimeoutCleanup sent to Hub                   │           │
         │           │                                     │           │
         │           │     (main loop still blocked!)      │           │
         │           │                                     │           │
         │           │     DeviceManager receives          │◀─────────│
         │           │     TimeoutCleanup                  │           │
         │           │     removes pending_requests[id]    │           │
         │           │                                     │           │
T=35s    │           │                                     │  Late DeviceResponse arrives
         │           │     (sender already removed)         │◀─────────│
         │           │     Response is LOST              │           │
```

### Root Causes

1. **recv_timeout(30s) is synchronous and blocks the entire handler**
   - The main loop cannot accept new connections or process responses during this time
   - All subsequent RPC requests must wait

2. **Single-threaded design with blocking operations**
   - One connection = one blocked handler = no parallelism
   - Cannot handle concurrent RPC requests efficiently

3. **Potential lost responses after timeout**
   - When `recv_timeout` expires, `response_rx` is dropped
   - TimeoutCleanup is sent but there's a race window
   - Late responses may be lost if cleanup happens before response arrives

---

## 5. Timeout Cleanup Mechanism

### Current Implementation (Lines 449-453)

```rust
Err(RecvTimeoutError::Timeout) => {
    tracing::warn!(correlation_id, "Request timed out, sending cleanup");
    // Send TimeoutCleanup to DeviceManager
    self.hub.send(Message::TimeoutCleanup { correlation_id });
    Ok(RpcResultType::Error {
        error: "Request timed out".to_string(),
    })
}
```

### DeviceManager Handler (Lines 298-302)

```rust
Message::TimeoutCleanup { correlation_id } => {
    if let Some(_) = self.pending_requests.remove(&correlation_id) {
        tracing::debug!(correlation_id, "Cleaned up timed-out request");
    }
}
```

### Issue with Current Cleanup

1. **Race condition:** If DeviceResponse arrives between timeout and TimeoutCleanup processing, response is sent but nobody receives it (response_rx already dropped)
2. **response_tx is discarded:** After recv_timeout returns Timeout, the channel sender goes out of scope
3. **Memory leak potential:** If TimeoutCleanup is never processed, pending_requests entry remains

---

## 6. Relationship with DeviceManager

### Channel Architecture

```
RpcWorker                                    DeviceManager
┌────────────────────┐                      ┌─────────────────────┐
│ device_control_tx  │───(send)────────────▶│ device_control_rx   │
│ (Sender<Request>)  │                      │ (Receiver<Request>) │
└────────────────────┘                      └─────────────────────┘
                                                     │
                                                     ▼
                                              ┌─────────────────────┐
                                              │ pending_requests    │
                                              │ HashMap<u64,        │
                                              │   Sender<Response>> │
                                              └─────────────────────┘
```

### DeviceManager Message Routing

```rust
// From manager.rs, lines 188-244
match msg {
    Message::DeviceControl {
        device_id,
        operation,
        params,
        correlation_id,
        respond_to,
    } => {
        // Store respond_to in pending_requests
        if let Some(sender) = respond_to {
            self.pending_requests.insert(correlation_id, sender);
        }
        
        // Forward to ModbusWorker
        match self.get_worker_name(&device_id) {
            Some(worker_name) => {
                context.hub().send(Message::DeviceControl {
                    device_id,
                    operation,
                    params,
                    correlation_id,
                    respond_to: None,  // Don't pass to ModbusWorker
                });
            }
            None => { /* Error: no worker for device */ }
        }
    }
    // ...
}
```

### Response Routing

```rust
// From manager.rs, lines 246-285
Message::DeviceResponse {
    device_id,
    success,
    data,
    error,
    correlation_id,
} => {
    // Lookup sender and forward response
    if let Some(sender) = self.pending_requests.remove(&correlation_id) {
        let response_data = (success, data, error);
        if let Err(e) = sender.send(response_data) {
            tracing::warn!(...);
        }
    } else {
        tracing::warn!(correlation_id, "No pending request found");
    }
}
```

---

## 7. Summary of Issues

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| recv_timeout blocks handler | **HIGH** | rpc_worker.rs:435 | 30s blocking prevents concurrent requests |
| Main loop blocked during recv | **HIGH** | rpc_worker.rs:435 | Cannot accept new connections |
| Response lost after timeout | **MEDIUM** | rpc_worker.rs:449-453 | Race between timeout and cleanup |
| Single-threaded design | **HIGH** | rpc_worker.rs:310-387 | No parallelism for concurrent RPCs |
| Channel sender leak | **MEDIUM** | manager.rs:59 | pending_requests entry lives until cleanup |

---

## 8. References

- RpcWorker source: `src/workers/rpc_worker.rs`
- Message types: `src/messages.rs`
- DeviceManager: `src/workers/manager.rs`
- Inherited wisdom: `.sisyphus/notepads/rpc-worker-deadlock-fix/learnings.md`
- RoboPLC research: `.sisyphus/research/roboplc-async-support.md`
- roboplc-rpc research: `.sisyphus/research/roboplc-rpc-async.md`