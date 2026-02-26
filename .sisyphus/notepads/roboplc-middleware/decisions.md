## Decisions from roboplc-middleware Implementation

### Task 25 - Latency Trend Monitoring
- Chosen approach: replace static high-latency threshold with per-device 3-sigma anomaly detection.
- Rationale: absolute thresholds do not adapt to device-specific baseline latency, while rolling distribution-based thresholding captures abnormal spikes relative to normal behavior.
- Scope boundary: implementation and tests kept entirely in `src/workers/latency_monitor.rs`; no changes to shared structs or dependencies.

### Task 27 - Connection Pool Limits Scaffold
- Chosen approach: add queue and operation types directly in `src/workers/modbus_worker.rs` with no cross-file refactor.
- Rationale: task scope explicitly forbids modifying other files and defers real Modbus I/O integration; local scaffolding with unit tests provides safe incremental delivery.
- Concurrency policy source: `OperationQueue::new(device.max_concurrent_ops as usize)` in `ModbusWorker::new` to bind runtime limits to per-device config defaults/overrides.

### Task 29 - JSON-RPC Server Implementation
- Chosen approach: keep `roboplc_rpc::server::RpcServer` as protocol engine and add TCP transport loop directly inside `RpcWorker::run`.
- Rationale: upstream crate is transport-agnostic and does not expose bind/listen APIs; implementing `TcpListener` in worker is required to satisfy runtime server behavior.
- Shutdown policy: use non-blocking accept loop guarded by `context.is_online()` for graceful stop without extra worker threads or new dependencies.
