# Workers Module

**Scope**: RoboPLC worker implementations for the middleware.

## Overview

Workers are independent execution units in the RoboPLC framework. Each worker:
- Runs in its own thread with configurable CPU affinity
- Uses real-time scheduling (FIFO) with configurable priority
- Communicates via the Hub message-passing system
- Accesses shared state through `Variables`

## Worker Architecture

```
RpcWorker ──DeviceControl──▶ Manager ──DeviceControl──▶ ModbusWorker (per device)
     ▲                        │                              │
     └────────DeviceResponse────────── Manager ◀──────────────┘

HttpWorker ──SystemStatus──▶ Manager ──查询 Variables ──▶ Response
```

## Worker Types

| Worker | File | Port | Purpose |
|--------|------|------|---------|
| **RpcWorker** | `rpc_worker.rs` | 8080 | JSON-RPC 2.0 server |
| **HttpWorker** | `http_worker.rs` | 8081 | HTTP management API |
| **DeviceManager** | `manager.rs` | - | Message router between workers |
| **ConfigLoader** | `config_loader.rs` | - | Hot config reload via file watching |
| **ConfigUpdater** | `config_updater.rs` | - | Config update processing |
| **LatencyMonitor** | `latency_monitor.rs` | - | 3-sigma latency anomaly detection |
| **ModbusWorker** | `modbus/worker.rs` | - | Modbus TCP client (one per device) |

## Worker Pattern

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "worker_name", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct MyWorker {
    // Worker state fields
}

impl Worker<Message, Variables> for MyWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        while context.is_online() {
            // Worker logic
            context.hub().send(Message::...)?;
        }
        Ok(())
    }
}
```

## Key Conventions

- **CPU Affinity**: Workers pin to specific CPUs (e.g., `cpu = 1`)
- **Scheduling**: Use `"fifo"` (real-time FIFO) for time-critical workers
- **Priority**: Range 1-99 (higher = more priority). ModbusWorker uses 80.
- **Message Types**: See `crate::messages::Message` enum
- **Hub Communication**: Use `context.hub().send()` and `event_matches!` macro

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add new worker | Create `src/workers/<name>.rs` | Follow existing pattern |
| Worker registration | `src/main.rs` lines 102-119 | `controller.spawn_worker()` |
| Message routing | `manager.rs` | Correlation ID mapping |
| Shared state | `lib.rs` `Variables` struct | Arc<RwLock<>> for thread safety |
| Message types | `messages.rs` | All Message enum variants |

## Anti-Patterns

- **NEVER** block in worker run loops indefinitely - check `context.is_online()`
- **NEVER** spawn threads directly - use RoboPLC's worker system
- **NEVER** use `std::sync::Mutex` - use `parking_lot_rt::RwLock` for RT safety
- **ALWAYS** handle Hub send errors - don't unwrap blindly
