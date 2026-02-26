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

### Config Loader Reload Pattern (Task 21)
- Keep config loader state as `current_config` and compare old/new via `serde_json::Value` to detect changed paths.
- Use recursive object-key diff collection (dot notation like `server.rpc_port`) and treat array inequality as a path-level change.
- Emit `Message::ConfigUpdate { config: <serialized-new-config> }` only when diff is non-empty; log changed fields with tracing.

### Modbus TCP Client Setup Pattern (Task 22)
- `roboplc::comm::tcp::connect` returns a lazy `roboplc::comm::Client`; call `client.connect()?` to force immediate connectivity validation.
- Keep per-device worker state as `Option<ModbusClient>` plus `last_heartbeat` timestamp to gate reconnection and heartbeat emission.
- Encapsulate reconnection in client wrapper: `reconnect()` should drop stale connection state, call `client.reconnect()` when present, then establish a fresh connection.
- In worker loop, retry connection with bounded sleep (`RECONNECT_DELAY`) before continuing periodic work.
- Add unit tests for constructor invariants (client starts disconnected; worker starts with no client) to guard state scaffolding before register I/O is implemented.

### Application-layer Heartbeat Pattern (Task 24)
- Add explicit `ConnectionState` (`Disconnected` / `Connecting` / `Connected`) on worker state and emit `DeviceEvent` only when transitions occur to avoid event spam.
- Map `Connecting` to `DeviceEventType::Reconnecting` so downstream monitors can treat retry loops distinctly from hard disconnects.
- Keep event emission testable by routing through closure-based helpers (`update_connection_state_with`, `record_communication_with`), while production wrappers still push into `context.variables()` buffers.
- Record successful heartbeat as communication (`last_communication = Some(SystemTime::now())`) and push a `LatencySample` in the same path for consistent observability updates.

### Latency Anomaly Monitoring Pattern (Task 25)
- Maintain per-device rolling latency stats with `HashMap<String, LatencyStats>` to avoid cross-device contamination of baselines.
- Use fixed-size `VecDeque<u64>` window (`LATENCY_WINDOW = 100`) and recompute mean/variance on each insertion for predictable behavior and simpler correctness.
- Apply anomaly detection only after a minimum baseline (`MIN_ANOMALY_SAMPLES = 10`) and flag samples above `mean + 3 * std_dev`.
- Evaluate anomaly status against pre-insert statistics, then insert current sample so each event reflects deviation from historical trend.

### Connection Pool Limits Scaffolding (Task 27)
- Implement queue state with `OperationQueue<T> { pending: VecDeque<T>, in_flight, max_in_flight }` in `modbus_worker.rs`; `start_next` increments in-flight only when capacity exists and `complete` decrements safely.
- Keep queue generic and operation enum explicit (`ModbusOp::{ReadHolding, WriteSingle, WriteMultiple}`) so later register-I/O wiring can reuse the same queue primitives without API churn.
- Initialize worker queue from config (`device.max_concurrent_ops as usize`) during `ModbusWorker::new` so per-device concurrency policy is enforced at construction time.
- For placeholder task stages where queue is not consumed yet, use targeted `#[allow(dead_code)]` on queue scaffolding to keep `lsp_diagnostics` clean without changing behavior.
