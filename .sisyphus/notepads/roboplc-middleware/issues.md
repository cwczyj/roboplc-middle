# Issues

## 2026-02-26 System Linker Issue

### Issue
- Error: `linker 'cc' not found`
- Dependencies resolve correctly but compilation fails due to missing C compiler

### Impact
- Cannot compile Rust code on this system
- Need to install build-essential or equivalent

### Workaround
- Document that Cargo.toml is correct
- Dependencies successfully downloaded and locked
- System needs `apt install build-essential` or equivalent

### Status
- Blocked: Requires system-level package installation
- Cargo.toml is verified correct

## 2026-02-26 Workspace Build State (Task 12)

### Issue
- `cargo check` fails due to multiple pre-existing compilation errors in unrelated files (`config_loader.rs`, `http_worker.rs`, `latency_monitor.rs`, `rpc_worker.rs`, `device_profile.rs`).
- New `modbus_worker.rs` file itself is clean under `lsp_diagnostics`.

### Impact
- Task-local scaffolding compiles at file level, but repository-level `cargo check` is currently red.

### Status
- Blocked by unrelated existing code errors outside Task 12 scope.
