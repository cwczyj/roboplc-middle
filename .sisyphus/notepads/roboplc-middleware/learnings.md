## Learnings from roboplc-middleware Implementation

### RoboPLC API Usage (CRITICAL)
- **DataPolicy import**: Use `use roboplc::prelude::*;` which re-exports DataPolicy from rtsc. DO NOT use `use roboplc::derive::DataPolicy;` - the derive module doesn't exist.
- **data_delivery variants**: Valid values are `single`, `single_optional`, `optional`, `always`, `latest`. NOT `broadcast` (that's invalid).
- **DataBuffer**: `roboplc::buf::DataBuffer` is a type alias with NO generic params. Use `rtsc::buf::DataBuffer<T>` directly instead.
- **DataBuffer constructor**: Use `DataBuffer::bounded(capacity)` NOT `DataBuffer::new(capacity)`.
- **rtsc crate**: Must add `rtsc = "0.4"` to Cargo.toml for direct DataBuffer access.
- **DataBuffer Debug**: DataBuffer<T> doesn't implement Debug. Implement manual Debug for structs containing DataBuffer fields.

### Worker Implementation Patterns
- **WorkerOpts derive**: Must be on struct, NOT on impl block: `#[derive(WorkerOpts)] #[worker_opts(name = "worker_name")] pub struct MyWorker { ... }`
- **Worker trait**: `impl Worker<Message, Variables> for MyWorker`
- **Hub subscription**: `context.hub().register("name", event_matches!(Message::Xxx { .. }))`
- **Hub send**: `context.hub().send(Message::Xxx { ... })`
- **Variables access**: `context.variables().field_name`
- **Blocking workers**: Add `blocking = true` in worker_opts for I/O-bound workers

### File Watching (notify 6.x)
- Use `notify::recommended_watcher(tx)` NOT `notify::watcher(tx, duration)`
- The new API uses `Watcher` trait with `watch(&Path, RecursiveMode)`
- Use `std::sync::mpsc::channel()` for the watcher channel

### Tokio Runtime in Blocking Workers
- Use `tokio::runtime::Builder::new_multi_thread().enable_all().build()` for creating runtime
- The `Runtime::new()` requires `rt-multi-thread` feature in Cargo.toml

### Import Paths in Workers
- Use `crate::{Message, Variables, ...}` NOT `roboplc_middleware::...`
- Workers are in the same crate as lib.rs

### Device Profile Patterns
- Prefer `crate::config::{AddressingMode, ByteOrder, DataType}` for internal modules
- `AddressMapping::parse` normalizes prefixed Modbus notation (`c`, `d`, `i`, `h`)
- Keep byte-order handling isolated in `convert_byte_order` utility

### Pattern References
- RoboPLC modbus-master example: `/home/lipschitz/.cargo/registry/src/.../roboplc-0.6.4/examples/modbus-master.rs`
- Context7 docs: `/roboplc/roboplc` for Hub, Controller, Worker patterns
- Implemented Task 20: Added address validation in config.rs (format and range checks) with new ConfigError variants.
- Validation checks cover: address format (prefix h/d/c/i + number) and range 0-65535 for each register mapping.
- Added tests scaffolding plan (unit tests for invalid addresses) and ensured cargo test can be run (note: full test suite may require project dependencies).
