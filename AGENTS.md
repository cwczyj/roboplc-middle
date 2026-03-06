# AGENTS.md

Guidelines for AI agents working in this Rust codebase.

## Build/Test Commands

### Essential Commands
```bash
# Build the project (debug)
cargo build

# Build release binary
cargo build --release

# Run the project
cargo run

# Run with simulated mode (skips RT scheduling)
ROBOPLC_SIMULATED=1 cargo run

# Run all tests
cargo test

# Run a single test
cargo test test_name

# Run tests in a specific module
cargo test module_name

# Run tests with output
cargo test -- --nocapture

# Check for errors without building
cargo check

# Run linter (clippy)
cargo clippy

# Format code
cargo fmt

# Format and check (dry-run)
cargo fmt --check
```

### Running Specific Tests

```bash
# Unit test by exact name
cargo test transaction_id_increments

# Test in specific module
cargo test modbus_worker::tests

# Integration test file
cargo test --test integration_test_name

# Run tests matching pattern
cargo test backoff
```

## Code Style Guidelines

### Import Organization

Imports follow this order:
1. Crate-local modules (crate::)
2. External dependencies
3. Standard library (std::)

**Example from `src/workers/modbus_worker.rs`:**
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

### Constants and Statics

Define constants at module level after imports. Use `const` for compile-time values and `static` for thread-safe globals.

**Example:**
```rust
const BASE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const BACKOFF_BASE_MS: u64 = 100;
static TRANSACTION_COUNTER: AtomicU16 = AtomicU16::new(0);
```

### Type Conventions

- Use explicit types in struct fields and function signatures
- Derive common traits: `Debug`, `Clone`, `Copy` (when appropriate)
- Use `#[serde(...)]` attributes for serialization configuration
- Provide default values via functions or `#[serde(default = "func")]`

**Example from `src/config.rs`:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    #[serde(default = "default_tcp_nodelay")]
    pub tcp_nodelay: bool,
}

fn default_tcp_nodelay() -> bool {
    true
}
```

### Naming Conventions

- **Variables/Functions/Modules**: `snake_case`
- **Types/Structs/Enums**: `PascalCase`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Lifetimes**: Short single letters (`'a`, `'b`)

**Examples:**
```rust
struct TransactionId { pub id: u16 }
impl TransactionId { pub fn new() -> Self { ... } }
const BACKOFF_MAX_MS: u64 = 30000;
```

### Error Handling

- Create custom error enums using `thiserror` crate
- Use `Result<T, Box<dyn std::error::Error>>` for fallible functions
- Use `?` for error propagation
- Use `.unwrap_or_default()` for safe default values

**Example from `src/config.rs`:**
```rust
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Duplicate device ID: {0}")]
    DuplicateDeviceId(String),
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }
}
```

### Serde Patterns

- Use `#[serde(rename = "...")]` for conflicting Rust keywords (e.g., `type`)
- Use `#[serde(rename_all = "snake_case")]` or `"lowercase"` for enum variants
- Use `#[serde(default)]` for optional fields
- Use `#[serde(deny_unknown_fields)]` for strict parsing

**Example:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    #[default]
    Plc,
    RobotArm,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "m", content = "p", rename_all = "lowercase", deny_unknown_fields)]
enum RpcMethod<'a> {
    Ping {},
    GetStatus { device_id: &'a str },
}
```

### Worker Pattern (RoboPLC)

Workers use the RoboPLC framework with specific patterns:

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "worker_name", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct MyWorker {
    // Worker state fields
}

impl Worker<Message, Variables> for MyWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // Worker logic
        Ok(())
    }
}
```

- Use `interval()` for periodic tasks
- Check `context.is_online()` in loops
- Access shared state via `context.variables()`
- Send messages via `context.hub().send()`

### Testing Conventions

- Unit tests in `#[cfg(test)]` modules at bottom of files
- Use descriptive test names: `fn backoff_reset_restores_initial_state()`
- Arrange-Act-Assert pattern preferred
- Use helper functions for test fixtures

**Example from `src/workers/modbus_worker.rs`:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_id_increments() {
        let id1 = TransactionId::new();
        let id2 = TransactionId::new();
        assert_ne!(id1.id, id2.id);
    }

    fn test_device() -> Device {
        Device {
            id: "test-device".to_string(),
            // ...
        }
    }
}
```

### Documentation

- Use `///` for public items
- Use `//!` for module-level docs
- Include `TODO:` markers for future work
- Add inline comments for complex logic sections

**Example:**
```rust
/// Tracks connection backoff with exponential delay
///
/// Implements exponential backoff with jitter to prevent thundering herd.
struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

// TODO: Implement HTTP API
```

## Project Structure

```
src/
├── lib.rs           # Main library exports, shared state (Variables)
├── main.rs          # Entry point
├── config.rs        # Configuration parsing and validation
├── messages.rs      # Message enums for worker communication
├── api.rs          # HTTP API endpoints
├── workers/         # Worker implementations
│   ├── mod.rs
│   ├── manager.rs
│   ├── rpc_worker.rs
│   ├── modbus_worker.rs
│   ├── http_worker.rs
│   ├── config_loader.rs
│   └── latency_monitor.rs
└── profiles/        # Device profiles
    ├── mod.rs
    └── device_profile.rs
```

## Key Dependencies

- `roboplc`: Real-time PLC framework (workers, Hub, comm)
- `serde`/`serde_json`: Serialization
- `tokio`: Async runtime
- `thiserror`: Error handling
- `anyhow`: Error context
- `tracing`: Structured logging

## Where to Look

| Task | Location | See Also |
|------|----------|----------|
| Add a new worker | `src/workers/<name>.rs` | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Worker registration | `src/main.rs` lines 102-119 | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Modbus protocol changes | `src/workers/modbus/` | [modbus/AGENTS.md](src/workers/modbus/AGENTS.md) |
| Message routing logic | `src/workers/manager.rs` | [workers/AGENTS.md](src/workers/AGENTS.md) |
| Add new test | `tests/<type>_tests.rs` | [tests/AGENTS.md](tests/AGENTS.md) |
| Config parsing | `src/config.rs` | - |
| Shared state types | `src/lib.rs` `Variables` | - |
| Message types | `src/messages.rs` | - |

## Module Guides

- **[workers/AGENTS.md](src/workers/AGENTS.md)** - Worker patterns, Hub communication, RT scheduling
- **[modbus/AGENTS.md](src/workers/modbus/AGENTS.md)** - Modbus TCP protocol, connection management
- **[tests/AGENTS.md](tests/AGENTS.md)** - Testing patterns, mock servers, test organization
## Configuration

Configuration is loaded from `config.toml` in the working directory. See `config.sample.toml` for schema.
