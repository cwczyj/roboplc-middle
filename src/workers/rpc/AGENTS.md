# RPC Worker Module

**Scope**: JSON-RPC 2.0 server for the middleware.

## Overview

Async TCP server with JSON-RPC 2.0 protocol, device control routing, response correlation, timeout handling.

## Module Structure

```
rpc/
├── mod.rs        # Module exports
├── worker.rs     # RpcWorker with dedicated tokio runtime (4 worker, 128 blocking threads)
├── handler.rs    # RpcHandler implementation (consolidated request/cleanup logic)
├── server.rs     # Main async server loop
├── connection.rs # TCP connection handling
├── request.rs    # Deprecated - kept for backward compatibility
├── cleanup.rs    # Deprecated - kept for backward compatibility
└── types.rs      # Shared types: RpcMethod, RpcResultType, DeviceControlRequest, etc.
```

## Wave 3 Refactoring

`request.rs` and `cleanup.rs` deprecated but retained for backward compatibility. spawn_blocking logic consolidated into `handler.rs`.

## Key Types

| Type | File | Purpose |
|------|------|---------|
| `RpcWorker` | `worker.rs` | Main RoboPLC worker (wrapper with WorkerOpts) |
| `RpcHandler` | `handler.rs` | RpcServerHandler trait implementation |
| `DeviceControlRequest` | `types.rs` | Device control request structure |
| `PendingRequest` | `types.rs` | Pending request tracking for cleanup |
| `RpcMethod` | `types.rs` | RPC method enum (Ping, GetStatus, ReadSignalGroup, etc.) |
| `RpcResultType` | `types.rs` | RPC result enum for responses |

## RPC Methods

| Method | Description |
|--------|-------------|
| `ping` | Health check |
| `get_version` | Get server version |
| `get_device_list` | List configured devices |
| `get_status` | Get device status |
| `read_signal_group` | Read a signal group from device |
| `write_signal_group` | Write a signal group to device |

## Connection Lifecycle

1. **TCP Accept** → New connection spawned as async task
2. **JSON-RPC Parse** → Request payload parsed
3. **Handler Call** → RpcHandler.handle_call() processes method
4. **Device Control** → Request sent to Hub via mpsc channel
5. **Response** → Response routed back via oneshot channel
6. **Cleanup** → Timed-out requests cleaned up periodically

## Tokio Runtime Pattern

RpcWorker spawns dedicated tokio runtime with 4 worker threads and 128 blocking threads. Provides isolation from RoboPLC RT scheduler, handles concurrent JSON-RPC requests.

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add RPC method | `types.rs` | `RpcMethod` enum |
| Change timeout | `handler.rs` | Consolidated request/cleanup |
| Connection handling | `connection.rs` | TCP read/write logic |

## Anti-Patterns

- **NEVER** share RpcHandler between threads without Arc
- **NEVER** block in async server loop - use tokio::select!
- **ALWAYS** use TimeoutHandler for operations - don't block indefinitely
- **ALWAYS** track pending requests for cleanup