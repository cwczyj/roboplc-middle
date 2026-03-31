# AGENTS.md

Guidelines for AI agents working in this Rust codebase.

## Build/Test Commands

```bash
cargo build              # Debug build
cargo build --release    # Release binary
cargo run                # Run project
ROBOPLC_SIMULATED=1 cargo run  # Simulated mode (no RT scheduling)
cargo test               # All tests
cargo test test_name     # Single test
cargo test -- --nocapture  # Test output
cargo check              # Check errors
cargo clippy             # Linter
cargo fmt                # Format code
```
## Code Style Guidelines

### Import Organization

1. Crate-local (crate::)
2. External dependencies
3. Standard library (std::)

**Example:** `src/workers/modbus/worker.rs`
```rust
use crate::config::Device;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::comm::Client;
use roboplc::controller::prelude::*;
use roboplc::io::modbus::prelude::*;
use roboplc::{comm::tcp, time::interval};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
```
### Constants/Statics

Module-level. `const` for compile-time, `static` for thread-safe.

### Type Conventions

- Explicit types in struct fields/functions
- Derive: `Debug`, `Clone`, `Copy`
- `#[serde(...)]` for serialization
- Defaults via functions or `#[serde(default = "func")]`

### Naming

- **Variables/Functions/Modules**: `snake_case`
- **Types/Structs/Enums**: `PascalCase`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Lifetimes**: Short single letters

### Error Handling

- Custom error enums with `thiserror`
- `Result<T, Box<dyn std::error::Error>>`
- `?` for propagation
- `.unwrap_or_default()` for safe defaults

### Serde Patterns

- `#[serde(rename = "...")]` for Rust keywords
- `#[serde(rename_all = "snake_case")]` or `"lowercase"`
- `#[serde(default)]` for optional
- `#[serde(deny_unknown_fields)]` for strict parsing

### Worker Pattern

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "worker_name", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct MyWorker;

impl Worker<Message, Variables> for MyWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult { Ok(()) }
}
```
- `interval()` for periodic tasks
- `context.is_online()` in loops
- `context.variables()` for shared state
- `context.hub().send()` for messages

### Testing

- Unit tests in `#[cfg(test)]` modules
- Descriptive names: `fn backoff_reset_restores_initial_state()`
- Arrange-Act-Assert pattern
- Helper functions for fixtures

### Documentation

- `///` for public items
- `//!` for module-level
- `TODO:` markers
- Inline comments for complex logic

## Project Structure
```
src/
├── lib.rs              # Library exports, shared state (Variables)
├── main.rs             # Entry point
├── config.rs           # Config parsing/validation
├── messages.rs         # Worker communication messages
├── data_conversion.rs  # Data type conversion
├── workers/            # Worker implementations
│   ├── mod.rs
│   ├── manager.rs      # Device manager (router)
│   ├── rpc_worker.rs   # JSON-RPC 2.0 server
│   ├── http_worker.rs  # HTTP API server
│   ├── heartbeat_worker.rs   # Heartbeat detection
│   ├── latency_monitor.rs    # Latency anomaly detection
│   ├── config_loader.rs      # Hot config reload
│   ├── config_updater.rs     # Config update handler
│   └── modbus/         # Modbus implementation
│       ├── mod.rs
│       ├── worker.rs   # ModbusWorker implementation
│       ├── client.rs   # Modbus TCP client
│       ├── operations.rs # Register operations
│       ├── parsing.rs  # Signal group encoding/decoding
│       └── types.rs    # Shared types (Backoff, ConnectionState, etc.)
demo/                   # Demo binaries (mock_server, jsonrpc_client)
├── AGENTS.md           # AI agent guidance
└── .sisyphus/          # Planning directory
```

## Key Dependencies

- `roboplc`: Real-time PLC framework (RT scheduling, Hub messaging, workers, comm)
- `serde`/`serde_json`: Serialization
- `tokio`: Async runtime
- `thiserror`: Error handling
- `anyhow`: Error context
- `tracing`: Structured logging

## Where to Look

| Task | Location | See Also |
|------|----------|----------|
| Add worker | `src/workers/<name>.rs` | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Worker registration | `src/main.rs` lines 108-130 | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Modbus changes | `src/workers/modbus/` | [modbus/AGENTS.md](src/workers/modbus/AGENTS.md) |
| Message routing | `src/workers/manager.rs` | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Add test | `tests/<type>_tests.rs` | [tests/AGENTS.md](tests/AGENTS.md) |
| Config | `src/config.rs` | - |
| Variables | `src/lib.rs` | - |
| Messages | `src/messages.rs` | - |

## Non-Standard Elements

- **AGENTS.md hierarchy**: Multiple AGENTS.md files provide AI agent guidance (root, workers/, modbus/, tests/)
- **.sisyphus/**: Planning directory for AI-assisted development
- **demo/**: Demo binaries instead of examples/ (mock_server, jsonrpc_client)

## Configuration

Config files: `config.toml` (runtime), `config.example.toml`, `config.sample.toml` (schema), `config_mock.toml` (demo)

## Module Guides

- **[workers/AGENTS.md](src/workers/AGENTS.md)** - Worker patterns, Hub communication, RT scheduling
- **[modbus/AGENTS.md](src/workers/modbus/AGENTS.md)** - Modbus TCP protocol, connection management
- **[tests/AGENTS.md](tests/AGENTS.md)** - Testing patterns, mock servers, test organization
