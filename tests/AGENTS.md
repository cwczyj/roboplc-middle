# Tests

**Scope**: Integration, E2E, and functional tests for roboplc-middleware.

## Overview

Test suite organized by test type. All tests use the `roboplc_middleware` crate.

## Test Structure

| File | Type | Lines | Purpose |
|------|------|-------|---------|
| `integration_tests.rs` | Integration | 262 | Worker integration, startup tests |
| `e2e_tests.rs` | E2E | 650 | End-to-end scenarios with mock devices |
| `async_rpc_tests.rs` | Async | 400 | Async RPC handler tests |
| `functional_worker_tests.rs` | Functional | 70 | Worker lifecycle tests |
| `functional_config_tests.rs` | Functional | 140 | Config loading/parsing tests |
| `functional_http_tests.rs` | Functional | 120 | HTTP API endpoint tests |
| `mock_modbus.rs` | Mock | 700 | Mock Modbus TCP server for tests |

## Test Conventions

### Test Organization
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptive_test_name() {
        // Arrange
        let fixture = create_test_fixture();
        
        // Act
        let result = function_under_test(&fixture);
        
        // Assert
        assert_eq!(result, expected);
    }
}
```

### Config Helper Pattern
```rust
fn create_test_config(custom_ports: Option<(u16, u16)>) -> tempfile::NamedTempFile {
    let (rpc_port, http_port) = custom_ports.unwrap_or((8888, 8889));
    // Write TOML config to temp file
    // Return NamedTempFile (auto-deleted on drop)
}
```

### Port Selection
- Tests use ephemeral ports (8888, 8889, etc.)
- Use `portpicker` crate for OS-assigned ports in E2E tests
- Always use `portpicker::pick_unused_port()` to avoid conflicts

### Mock Server Pattern
```rust
// In mock_modbus.rs
pub fn start_mock_modbus(port: u16) -> MockServer {
    // Bind to localhost:port
    // Spawn async handler
    // Return handle for cleanup
}
```

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Add integration test | `integration_tests.rs` | Test worker interactions |
| Add E2E scenario | `e2e_tests.rs` | Full stack with mock device |
| Mock Modbus device | `mock_modbus.rs` | TCP server mocking |
| Test helpers | Top of any test file | `create_test_*` functions |
| Test config | `config.sample.toml` | Reference configuration |

## Running Tests

```bash
# All tests
cargo test

# Specific test file
cargo test --test integration_tests

# Specific test by name
cargo test test_name_here

# With output
cargo test -- --nocapture

# E2E tests only
cargo test --test e2e_tests
```

## Anti-Patterns

- **NEVER** use hardcoded ports in E2E tests - use `portpicker`
- **NEVER** leave test files behind - use `tempfile::NamedTempFile`
- **NEVER** rely on test order - tests must be independent
- **ALWAYS** clean up resources - use `Drop` or explicit cleanup
