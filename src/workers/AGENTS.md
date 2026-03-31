# Workers Module

RoboPLC worker implementations for the middleware.

## Overview

Workers are independent execution units:
- Thread with CPU affinity, RT scheduling, priority
- Hub message-passing, shared `Variables` access

## Architecture

```
RpcWorker ──DeviceControl──▶ Manager ──DeviceControl──▶ ModbusWorker (per device)
     ▲                        │                              │
     └────────DeviceResponse────────── Manager ◀──────────────┘

HttpWorker ──SystemStatus──▶ Manager ──查询 Variables ──▶ Response
```

## Worker Types

| Worker | File | Port | Purpose |
|--------|------|------|---------|
| RpcWorker | `rpc_worker.rs` | 8080 | JSON-RPC 2.0 |
| HttpWorker | `http_worker.rs` | 8081 | HTTP API |
| DeviceManager | `manager.rs` | - | Hub router |
| ConfigLoader | `config_loader.rs` | - | Hot reload |
| ConfigUpdater | `config_updater.rs` | - | Updates |
| LatencyMonitor | `latency_monitor.rs` | - | 3-sigma |
| HeartbeatWorker | `heartbeat_worker.rs` | - | Heartbeat |
| ModbusWorker | `modbus/worker.rs` | - | Modbus TCP |

## Pattern

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "name", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct MyWorker;
impl Worker<Message, Variables> for MyWorker {
    fn run(&mut self, ctx: &Context<Message, Variables>) -> WResult {
        while ctx.is_online() { ctx.hub().send(Message::...)?; } Ok(())
    }
}
```

## Key Conventions

- CPU Affinity: Workers pin to CPUs
- Scheduling: `"fifo"` for time-critical
- Priority: 1-99, higher is priority
- Message Types: `crate::messages::Message`
- Hub: `context.hub().send()` + `event_matches!`

## Cross-Cutting Utilities

| Pattern | Locations | Notes |
|---------|-----------|-------|
| Correlation ID | `rpc/handler.rs`, `heartbeat_worker.rs`, `modbus/types.rs` | Consolidate? |
| TimeoutHandler | `manager.rs` | Timeout cleanup |
| Backoff | `modbus/types.rs` | Exponential backoff |
| Time/Duration | All workers | `interval()`, `SystemTime`, `UNIX_EPOCH` |
| Device State | `manager.rs`, `lib.rs` | Hub broadcasts `DeviceEvent` |

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add worker | `src/workers/<name>.rs` | Follow pattern |
| Register | `src/main.rs` 108-130 | `spawn_worker()` |
| Routing | `manager.rs` 70-115 | Device cleanup |
| State | `lib.rs` Variables | Arc<RwLock<>> |
| Messages | `messages.rs` | Message enum |

## Anti-Patterns

- NEVER block run loops - check `context.is_online()`
- NEVER spawn threads - use RoboPLC workers
- NEVER use `std::sync::Mutex` - use `parking_lot_rt::RwLock`
- ALWAYS handle Hub send errors
