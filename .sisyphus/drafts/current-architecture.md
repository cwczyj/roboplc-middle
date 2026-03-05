# Current RpcWorker Architecture

## Overview

RpcWorker implements a JSON-RPC 2.0 TCP server that receives external client requests and forwards them to device managers for processing.

## Message Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RpcWorker Thread                                   │
│  ┌─────────────────┐      ┌──────────────────┐      ┌────────────────────┐  │
│  │   TCP Client    │─────▶│  RpcServer       │─────▶│  RpcHandler        │  │
│  │   (JSON-RPC)    │      │  handle_request  │      │  handle_call()     │  │
│  └─────────────────┘      └──────────────────┘      └─────────┬──────────┘  │
│                                                                 │            │
│  ┌───────────────────────────────────────────────────────────────▼──────────┐│
│  │                    send_device_control()                          LINE 393│
│  │  ┌──────────────────────────────────────────────────────────────┐       ││
│  │  │ 1. Create oneshot channel (response_tx, response_rx)        │       ││
│  │  │ 2. Send DeviceControlRequest via device_control_tx          │       ││
│  │  │ 3. BLOCK on response_rx.recv_timeout(30s) ◄── DEADLOCK      │       ││
│  │  └──────────────────────────────────────────────────────────────┘       ││
│  └──────────────────────────────────────────────────────────────────────────┘│
│                                     │                                         │
└─────────────────────────────────────│─────────────────────────────────────────┘
                                      │
                    ┌─────────────────▼─────────────────┐
                    │     Main Loop (run() line 555)     │
                    │  ┌─────────────────────────────┐  │
                    │  │ listener.accept()           │  │
                    │  │                             │  │
                    │  │ No connection:              │  │
                    │  │   try_recv() device_control │  │
                    │  │        │                    │  │
                    │  │        ▼                    │  │
                    │  │   hub.send(DeviceControl)   │  │ LINE 640
                    │  └─────────────────────────────┘  │
                    └────────────────────────────────────┘

                                      │
                    ┌─────────────────▼─────────────────┐
                    │         RoboPLC Hub                │
                    │    (Message Bus - all workers)     │
                    └────────────────────────────────────┘
                                      │
                    ┌─────────────────▼─────────────────┐
                    │       DeviceManager Worker         │
                    │  - Receives DeviceControl          │
                    │  - Routes to ModbusWorker          │
                    │  - Stores respond_to in            │
                    │    pending_requests (HashMap)      │
                    └────────────────────────────────────┘
                                      │
                    ┌─────────────────▼─────────────────┐
                    │       ModbusWorker                │
                    │  - Executes Modbus TCP operation  │
                    │  - Sends DeviceResponse           │
                    └────────────────────────────────────┘
                                      │
                    ┌─────────────────▼─────────────────┐
                    │       DeviceManager Worker         │
                    │  - Receives DeviceResponse         │
                    │  - Looks up respond_to sender      │
                    │  - Sends via channel to RpcWorker  │
                    └────────────────────────────────────┘
                                      │
                    ┌─────────────────▼─────────────────┐
                    │         RpcWorker Thread           │
                    │  response_rx.recv_timeout()        │
                    │       UNBLOCKS ✓                   │
                    └────────────────────────────────────┘
```

## Thread Model

- **Single thread**: All processing happens in the main RpcWorker thread
- The worker uses `blocking = true` in WorkerOpts (line 474)
- Listener is set to non-blocking mode, but that doesn't help because:
  - `handle_call()` is synchronous and blocks

## Channels

| Channel | Type | Created At | Purpose | Blocking? |
|---------|------|------------|---------|-----------|
| `device_control_tx` | mpsc::Sender<DeviceControlRequest> | Line 520 | Internal queue for DeviceControl messages | No (buffered) |
| `device_control_rx` | mpsc::Receiver<DeviceControlRequest> | Line 520 | Main loop consumes from here | No (try_recv) |
| `response_tx` | mpsc::Sender<(bool, JsonValue, Option<String>)> | Line 404 (per request) | Response return path | No |
| `response_rx` | mpsc::Receiver<(bool, JsonValue, Option<String>)> | Line 404 (per request) | Waiting for response | **YES - recv_timeout(30s)** |

## Blocking Points

### 1. Line 433: `response_rx.recv_timeout(30s)`

```rust
match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
```

**Location**: `RpcHandler::send_device_control()` method (line 393-464)

**Impact**: 
- Blocks the current thread for up to 30 seconds
- During this time, the `handle_call()` method cannot return
- The main run() loop continues but is in the same thread

**What happens**:
1. Client sends JSON-RPC request
2. `server.handle_request_payload()` calls `handle_call()`
3. `handle_call()` calls `send_device_control()`
4. `send_device_control()` sends request to `device_control_tx`
5. **BLOCKS** on `response_rx.recv_timeout(30s)`
6. Meanwhile, main loop at line 628 calls `device_control_rx.try_recv()`

### 2. Line 628: `device_control_rx.try_recv()`

```rust
while let Ok(request) = device_control_rx.try_recv() {
```

**Location**: Main loop in `run()` method (line 555-652)

**Impact**:
- Cannot execute while `handle_call()` is running in the same thread
- Messages queue in `device_control_rx` but are never forwarded to Hub
- Even if main loop "runs", it's in the same thread as blocking code

## Deadlock Analysis

### Why Deadlock Occurs

1. **Single Thread Constraint**: RpcWorker runs on a single thread. When `handle_call()` is executing, the main loop is blocked waiting for `handle_call()` to return.

2. **Sequential Processing**: `handle_call()` is called synchronously from `server.handle_request_payload()`:
   - Line 613: `server.handle_request_payload::<Json>(&request_payload, source)`
   - Inside: calls `handle_call()` which calls `send_device_control()`
   - `send_device_control()` blocks at line 433

3. **Message Queue Buildup**: 
   - At line 419: `self.device_control_tx.send(request)` succeeds (message queued)
   - Main loop at line 628 tries to drain via `try_recv()` 
   - BUT: The main loop is in the SAME thread and cannot make progress until `handle_call()` returns
   - The `try_recv()` at line 628 runs in the same thread context

4. **The Critical Problem**:
   - Even if messages are queued (line 419), they are never forwarded to Hub (line 640)
   - Line 640 is inside the main loop's "no connection" branch (line 624-646)
   - But the main loop iteration that would process these messages never gets a chance to run
   - Because `handle_call()` is still blocking in line 433

5. **Result**:
   - ModbusWorker never receives DeviceControl message
   - ModbusWorker never sends DeviceResponse back
   - `response_rx.recv_timeout(30s)` times out
   - Client gets timeout error

### Timing Diagram

```
Time    Thread State
────    ────────────
T0      Client sends RPC request
T1      handle_request_payload() called
T2      handle_call() → send_device_control()
T3      device_control_tx.send() succeeds (queued)
T4      BLOCK: response_rx.recv_timeout(30s) starts
        │
        │ [Main loop cannot progress - same thread]
        │
T34     TIMEOUT after 30 seconds
T35     Returns error to client: "Request timed out"
T36     Meanwhile: messages still queued in device_control_rx
```

## Message Routing Summary

| Step | Location | Action |
|------|----------|--------|
| 1 | rpc_worker.rs:342 | `handle_call()` receives RPC method |
| 2 | rpc_worker.rs:342-383 | Maps to Operation enum |
| 3 | rpc_worker.rs:402 | `send_device_control()` generates correlation_id |
| 4 | rpc_worker.rs:404 | Creates oneshot response channel |
| 5 | rpc_worker.rs:410-417 | Creates DeviceControlRequest with respond_to |
| 6 | rpc_worker.rs:419 | Sends to device_control_tx (queued) |
| 7 | rpc_worker.rs:433 | **BLOCKS** waiting for response |
| ... | (30 seconds pass) | Message never reaches Hub |
| 8 | rpc_worker.rs:447-454 | Timeout, sends TimeoutCleanup to Hub |
| 9 | rpc_worker.rs:452 | Returns error to client |

### What Should Happen (but doesn't due to deadlock)

| Step | Location | Action |
|------|----------|--------|
| 7a | rpc_worker.rs:628 | Main loop receives from device_control_rx |
| 7b | rpc_worker.rs:631-640 | Forwards to Hub as Message::DeviceControl |
| 7c | manager.rs:225 | DeviceManager receives, routes to ModbusWorker |
| 7d | modbus_worker.rs | Executes Modbus operation |
| 7e | modbus_worker.rs | Sends DeviceResponse to Hub |
| 7f | manager.rs:263-269 | DeviceManager looks up respond_to, sends response |
| 7g | rpc_worker.rs:435 | response_rx receives, unblocks |

## Key Code References

| Line | Code | Description |
|------|------|-------------|
| 265 | `device_control_tx: Sender<DeviceControlRequest>` | Handler field |
| 404 | `let (response_tx, response_rx) = channel()` | Per-request response channel |
| 419 | `self.device_control_tx.send(request)` | Queue message |
| 433 | `response_rx.recv_timeout(30s)` | **BLOCKING POINT** |
| 520 | `let (device_control_tx, device_control_rx)` | Internal channel creation |
| 628 | `device_control_rx.try_recv()` | Main loop drain (cannot run during block) |
| 640 | `context.hub().send(message)` | Forward to Hub |

## Related Files

- `/home/lipschitz/Documents/Code/RustCode/src/workers/rpc_worker.rs` - Full implementation
- `/home/lipschitz/Documents/Code/RustCode/src/workers/manager.rs` - DeviceManager routing
- `/home/lipschitz/Documents/Code/RustCode/src/messages.rs` - Message type definitions
