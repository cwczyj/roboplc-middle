# RPC Worker Module

**Scope**: JSON-RPC 2.0 server implementation for the middleware.

## Overview

The `rpc/` submodule handles all JSON-RPC TCP server functionality:
- Async TCP connection handling with tokio
- JSON-RPC 2.0 protocol parsing and response generation
- Device control request routing to the Hub
- Response correlation and timeout handling

## Module Structure

```
rpc/
├── mod.rs        # Module exports and re-exports
├── worker.rs     # RpcWorker - RoboPLC worker wrapper
├── handler.rs    # RpcHandler - RpcServerHandler trait implementation
├── server.rs     # run_async_server - main async server loop
├── connection.rs # TCP connection handling
├── request.rs    # Device control request processing
├── cleanup.rs    # Timeout request cleanup logic
└── types.rs      # Shared types: RpcMethod, RpcResultType, DeviceControlRequest, etc.
```

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

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add RPC method | `types.rs` | `RpcMethod` enum |
| Change request timeout | `request.rs` | `recv_timeout()` duration |
| Modify cleanup interval | `server.rs` | `sleep(Duration::from_secs(10))` |
| Connection handling | `connection.rs` | TCP read/write logic |
| Worker registration | `worker.rs` | `WorkerOpts` derive |

## Anti-Patterns

- **NEVER** share RpcHandler between threads without Arc
- **NEVER** block in async server loop - use tokio::select!
- **ALWAYS** use TimeoutHandler for operations - don't block indefinitely
- **ALWAYS** track pending requests for cleanup