# Modbus Worker Module

**Scope**: Modbus TCP protocol implementation and device communication.

## Overview

The `modbus/` submodule handles all Modbus TCP communication:
- Connection management with exponential backoff
- Register read/write operations
- Signal group encoding/decoding
- Transaction tracking and timeout handling

**Three-tier architecture**: Worker → Handler → State separation.

## Module Structure

 ```
 modbus/
 ├── mod.rs        # Module exports and re-exports
 ├── client.rs     # ModbusClient - TCP client, largest file (1026 lines)
 ├── worker.rs     # ModbusWorker - RoboPLC worker wrapper (~75 lines)
 ├── handler.rs    # DeviceControlHandler - message handling logic (~430 lines)
 ├── state.rs      # ModbusWorkerState - connection state management (~150 lines)
 ├── operations.rs # Register operations and address parsing (~266 lines)
 ├── parsing.rs    # Signal group encoding/decoding (~727 lines)
 └── types.rs      # Shared types: Backoff, ConnectionState, etc. (~290 lines)
 ```

## Key Types

| Type | File | Purpose |
|------|------|---------|
| `ModbusWorker` | `worker.rs` | Main RoboPLC worker (wrapper with WorkerOpts) |
| `DeviceControlHandler` | `handler.rs` | DeviceControl message handler |
| `ModbusWorkerState` | `state.rs` | Internal state management |
| `ModbusClient` | `client.rs` | TCP connection + frame handling |
| `ModbusOp` | `client.rs` | Modbus function codes |
| `Backoff` | `types.rs` | Exponential backoff for reconnection |
| `ConnectionState` | `types.rs` | Connection state enum |
| `TimeoutHandler` | `types.rs` | Operation timeout management |
| `RegisterType` | `operations.rs` | Register type enum (Coil, Discrete, Input, Holding) |

## Register Address Format

| Prefix | Type | Modbus Code |
|--------|------|-------------|
| `c` | Coil | 0x |
| `d` | Discrete Input | 1x |
| `i` | Input Register | 3x |
| `h` | Holding Register | 4x |

Example: `h100` = Holding Register at address 100

## Worker Configuration

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    handler: DeviceControlHandler,
}
```

## Connection Lifecycle

1. **Disconnected** → **Connecting** (with backoff)
2. **Connecting** → **Connected** (TCP established)
3. **Connected** → Heartbeat loop + operation processing
4. On failure → **Reconnecting** → exponential backoff → retry

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add register type | `operations.rs` | `RegisterType` enum |
| Change backoff params | `types.rs` | `Backoff::new()` constants |
| Add Modbus function | `client.rs` | `ModbusOp` enum + `dispatch_op()` |
| Signal encoding | `parsing.rs` | `encode_fields_to_registers()` |
| Message handling | `handler.rs` | `handle_device_control()` |
| State management | `state.rs` | `ModbusWorkerState` |
| Worker registration | `worker.rs` | `WorkerOpts` derive |

## Anti-Patterns

- **NEVER** share `ModbusClient` between threads - each worker has its own
- **NEVER** ignore `ConnectionState` transitions - always emit `DeviceEvent`
- **ALWAYS** use `TimeoutHandler` for operations - don't block indefinitely
- **ALWAYS** check `context.is_online()` in loops