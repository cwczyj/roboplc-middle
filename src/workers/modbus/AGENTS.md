# Modbus Worker Module

**Scope**: Modbus TCP protocol implementation and device communication.

## Overview

The `modbus/` submodule handles all Modbus TCP communication:
- Connection management with exponential backoff
- Register read/write operations
- Signal group encoding/decoding
- Transaction tracking and timeout handling

## Module Structure

```
modbus/
├── mod.rs       # Module exports and re-exports
├── client.rs    # ModbusClient - low-level TCP client (786 lines)
├── worker.rs    # ModbusWorker - RoboPLC worker implementation (736 lines)
├── operations.rs # Register operations and address parsing
├── parsing.rs   # Signal group field encoding/decoding (537 lines)
└── types.rs     # Shared types: Backoff, ConnectionState, etc.
```

## Key Types

| Type | File | Purpose |
|------|------|---------|
| `ModbusWorker` | `worker.rs` | Main RoboPLC worker |
| `ModbusClient` | `client.rs` | TCP connection + frame handling |
| `ModbusOp` | `client.rs` | Modbus function codes (ReadHolding, WriteSingle, etc.) |
| `OperationQueue` | `types.rs` | Concurrent operation queue with max_in_flight |
| `Backoff` | `types.rs` | Exponential backoff for reconnection |
| `TransactionId` | `types.rs` | Auto-incrementing transaction tracker |
| `TimeoutHandler` | `types.rs` | Operation timeout management |

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
| Add Modbus function | `client.rs` | `ModbusOp` enum + `execute()` |
| Signal encoding | `parsing.rs` | `encode_fields_to_registers()` |
| Connection timeout | `worker.rs` | `connect()` method |

## Anti-Patterns

- **NEVER** share `ModbusClient` between threads - each worker has its own
- **NEVER** ignore `ConnectionState` transitions - always emit `DeviceEvent`
- **ALWAYS** use `TimeoutHandler` for operations - don't block indefinitely
- **ALWAYS** check `context.is_online()` in loops
