# RoboPLC Async Worker Support Research

**Research Date:** March 3, 2026  
**Researcher:** AI Research Agent  
**RoboPLC Version:** 0.6.4

---

## Executive Summary

**CONCLUSION: RoboPLC DOES NOT support async workers.**

The Worker trait requires a **synchronous** `fn run()` method, not `async fn run()`. The `blocking` attribute is a configuration hint for the task supervisor about how to manage the worker thread, not an indicator of async support.

---

## Key Findings

### 1. Worker Trait Does Not Support `async fn run`

**Evidence:** [Worker trait definition](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/src/controller.rs#L106-L111)

```rust
/// The trait which MUST be implemented by all workers
pub trait Worker<D: DataDeliveryPolicy + Clone + Send + Sync + 'static, V: Send>:
    Send + Sync
{
    /// The worker's main function, started by [`Controller::spawn_worker()`]. If the function
    /// returns an error, the process is terminated using [`critical()`].
    fn run(&mut self, context: &Context<D, V>) -> WResult;
}
```

**Explanation:** The `run` method is explicitly synchronous (`fn run`, not `async fn run`). This confirms that RoboPLC workers are designed to run in a blocking synchronous thread model, not using Rust's async/await runtime.

---

### 2. The `blocking` Attribute is NOT an Async Indicator

**Evidence:** [WorkerOpts derive macro documentation](https://docs.rs/roboplc/latest/roboplc/controller/derive.WorkerOpts.html)

The `blocking` attribute documentation states:

> **blocking** - Specifies if the worker is blocking. The value can be `true` or `false`. A hint for task supervisors that the worker blocks the thread (e.g. listens to a socket or has got a big interval in the main loop, does not return any useful result and should not be joined)

**Evidence:** [WorkerOptions trait default implementation](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/src/controller.rs#L113-L117)

```rust
impl WorkerOptions for T {
    // ... other methods ...
    fn worker_is_blocking(&self) -> bool {
        false
    }
}
```

**Evidence:** [Derive macro blocking implementation](https://github.com/roboplc/roboplc/blob/main/roboplc-derive/src/lib.rs#L64-L72)

```rust
/// * `blocking` - Specifies if the worker is blocking. The value can be `true` or `false`. A hint
///   for task supervisors that the worker blocks the thread (e.g. listens to a socket
///   or has got a big interval in the main loop, does not return any useful result
///   and should not be joined)
```

**Explanation:** The `blocking` flag is a **hint for the task supervisor** to optimize worker management:
- When `blocking = true`: The supervisor knows the worker will block indefinitely (e.g., socket listener, server loop) and should NOT wait for it to complete during shutdown
- When `blocking = false` (default): The supervisor may wait for the worker to finish gracefully

This is NOT related to Rust's async/await system. It's purely a thread management optimization hint.

---

### 3. How Workers Are Spawned

**Evidence:** [Controller::spawn_worker implementation](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/src/controller.rs#L134-L158)

```rust
pub fn spawn_worker<W: Worker<D, V> + WorkerOptions + 'static>(
    &mut self,
    mut worker: W,
) -> Result<()> {
    let context = self.context();
    let mut rt_params = RTParams::new().set_scheduling(worker.worker_scheduling());
    // ... scheduling and priority setup ...
    let mut builder = Builder::new()
        .name(worker.worker_name())
        .rt_params(rt_params)
        .blocking(worker.worker_is_blocking());  // ← This is where blocking is used
    // ... stack size setup ...
    self.supervisor.spawn(builder, move || {
        if let Err(e) = worker.run(&context) {
            error!(worker=worker.worker_name(), error=%e, "worker terminated");
            critical(&format!(
                "Worker {} terminated: {}",
                worker.worker_name(),
                e
            ));
        }
    })?;
    Ok(())
}
```

**Explanation:** Workers are spawned as **OS threads** via the Supervisor, using the Builder pattern. The `blocking` flag is passed to the thread builder to optimize shutdown behavior. The worker's `run()` method is called directly in a closure - there's no async runtime involved.

---

### 4. The `async` Feature Flag is for Locking, Not Async Workers

**Evidence:** [Cargo.toml features](https://docs.rs/roboplc/latest/crate/roboplc/latest/source/Cargo.toml.html)

```
[features]
default = ["locking-default"]
async = ["dep:parking_lot_rt"]
# ... other features ...
```

**Evidence:** [Documentation on locking safety](https://docs.rs/roboplc/latest/roboplc/index.html#locking-safety)

> Note: the asynchronous components use parking_lot_rt locking only.
> By default, the crate uses parking_lot for locking. For real-time applications, the following features are available:
> - `locking-rt` - use parking_lot_rt crate which is a spin-free fork of parking_lot.
> - `locking-rt-safe` - use RTSC priority-inheritance locking, which is not affected by priority inversion (Linux only).

**Explanation:** The `async` feature enables `parking_lot_rt` locking primitives for real-time safety in asynchronous *components* (like `hub_async`, `policy_channel_async`), but does NOT enable async workers. RoboPLC provides both synchronous and asynchronous *communication components*, but workers themselves are always synchronous.

---

### 5. Examples from Official Codebase

#### Example 1: Blocking I/O Worker

**Evidence:** [pipe.rs example](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/examples/pipe.rs#L15-L26)

```rust
#[derive(WorkerOpts)]
#[worker_opts(cpu = 0, priority = 50, scheduling = "fifo", blocking = true)]
struct Worker1 {
    reader: pipe::Reader,
}

impl Worker<Message, Variables> for Worker1 {
    fn run(&mut self, _context: &Context<Message, Variables>) -> WResult {
        loop {
            let line = self.reader.line()?;
            println!("Worker1: {}", line.trim_end());
        }
    }
}
```

**Pattern:** Workers that block on I/O (socket reads, file I/O) use `blocking = true`.

---

#### Example 2: Non-Blocking Periodic Worker

**Evidence:** [modbus-master.rs example](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/examples/modbus-master.rs#L54-L59)

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "puller", cpu = 1, scheduling = "fifo", priority = 80)]
struct ModbusPuller1 {
    sensor_mapping: ModbusMapping,
}

impl Worker<Message, Variables> for ModbusPuller1 {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let hub = context.hub();
        for _ in interval(Duration::from_millis(500)).take_while(|_| context.is_online()) {
            // Periodic work...
        }
        Ok(())
    }
}
```

**Pattern:** Workers that do periodic work use default (non-blocking) settings with `interval()`.

---

#### Example 3: Worker Without `blocking` Flag

**Evidence:** [raw-udp.rs example](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/examples/raw-udp.rs#L38-L49)

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "udp_out")]  // ← No blocking flag
struct UdpOut {}

impl Worker<Message, ()> for UdpOut {
    fn run(&mut self, context: &Context<Message, ()>) -> WResult {
        let mut tx = UdpSender::connect("localhost:25000")?;
        for _ in interval(Duration::from_secs(1)).take_while(|_| context.is_online()) {
            let data = EnvData { /* ... */ };
            if let Err(e) = tx.send(&data) {
                error!(worker=self.worker_name(), error=%e, "udp send error");
            }
        }
        Ok(())
    }
}
```

**Pattern:** Workers without `blocking` flag default to `blocking = false`.

---

## Blocking vs Non-Blocking Workers

### When to Use `blocking = true`

Use `blocking = true` for workers that:
- **Block indefinitely** on I/O operations (socket accept, file descriptors)
- **Run server loops** that never return (TCP servers, Modbus servers)
- **Listen to channels/hub messages** in an infinite loop
- **Should not be joined** during shutdown (process will terminate them)

**Example use cases:**
- Modbus/HTTP servers
- Socket listeners
- Pipe readers
- Channel consumers

### When to Use `blocking = false` (default)

Use default settings for workers that:
- **Do periodic work** with controlled intervals
- **Complete work within bounded time**
- **Can be joined** during shutdown
- **Use `interval()`** for timing control

**Example use cases:**
- Periodic data collectors (sensors)
- Periodic actuators (relays, motors)
- Data processors
- Monitor workers

---

## What Happens When `blocking` is Removed?

If you remove `blocking = true` from a worker that blocks indefinitely:

1. **The default is `blocking = false`** - Worker is marked as non-blocking
2. **During shutdown**, the supervisor **Waits** for the worker to finish
3. **Since the worker never returns** (e.g., server loop), the supervisor hangs indefinitely
4. **Result:** The application may not terminate cleanly, requiring SIGKILL

**Evidence:** [spawn_worker usage of blocking flag](https://github.com/roboplc/roboplc/blob/d79e79e06ab0499d6075ee287c50d348fba4039e/src/controller.rs#L149)

```rust
.blocking(worker.worker_is_blocking());  // ← Passed to thread builder
```

---

## Can RpcWorker Be Refactored to Use Async?

**NO.** Here's why:

1. **Worker trait requirement:** The `run()` method is synchronous and cannot be changed to async
2. **Thread-based architecture:** RoboPLC uses OS threads, not tokio async runtime
3. **No async runtime:** There is no tokio runtime or async executor in the worker spawn mechanism
4. **Design philosophy:** RoboPLC is designed for **real-time deterministic** execution, not async concurrency

### Alternative Approaches for RpcWorker

If you need to handle multiple concurrent RPC requests without blocking the entire worker:

1. **Use tokio INSIDE the worker** (non-async worker):
```rust
impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // Create a tokio runtime inside the synchronous worker
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            // Use tokio for async I/O inside the worker
            self.handle_async_requests().await;
        })
    }
}
```

2. **Use thread-safe channel + separate thread:**
   - Worker thread spawns additional threads
   - Use `std::sync::mpsc` or `crossbeam_channel` for communication

3. **Keep blocking = true** for workers that truly block:
   - If RpcWorker's main loop blocks on socket reads, keep `blocking = true`
   - The flag correctly describes the worker's behavior

---

## Configuration Examples

### Blocking Worker (Server/Socket Listener)

```rust
#[derive(WorkerOpts)]
#[worker_opts(
    name = "rpc_server",
    cpu = 0,
    priority = 50,
    scheduling = "fifo",
    blocking = true  // ← Correct: blocks on socket accept
)]
struct RpcWorker {
    listener: TcpListener,
}

impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, _context: &Context<Message, Variables>) -> WResult {
        loop {
            let (stream, addr) = self.listener.accept()?;  // ← Blocks here
            // Handle connection...
        }
    }
}
```

### Non-Blocking Worker (Periodic Task)

```rust
#[derive(WorkerOpts)]
#[worker_opts(
    name = "sensor_puller",
    cpu = 1,
    priority = 80,
    scheduling = "fifo"
    // blocking = false (default)  // ← Correct: periodic work
)]
struct SensorPuller {
    client: ModbusClient,
}

impl Worker<Message, Variables> for SensorPuller {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        for _ in interval(Duration::from_millis(500)).take_while(|_| context.is_online()) {
            // Periodic, bounded-time work
        }
        Ok(())
    }
}
```

---

## Best Practices

1. **Always set `blocking = true` for server-like workers**
   - TCP/Unix socket servers
   - Modbus servers
   - Any worker with an infinite accept() loop

2. **Use default (blocking = false) for periodic workers**
   - Sensor pollers
   - Actuator controllers
   - Data processors
   - Workers using `interval()`

3. **Don't confuse `blocking` with async**
   - `blocking = true` ≠ async worker
   - It's a shutdown optimization hint for the supervisor
   - All workers are synchronous threads

4. **If you need async I/O, run tokio runtime inside the worker**
   - Create tokio runtime inside `run()`
   - Use `rt.block_on(async { ... })`
   - The worker itself is still synchronous

5. **Test shutdown behavior**
   - Workers with `blocking = false` should complete quickly
   - Workers with `blocking = true` will be terminated (not joined)
   - Verify clean shutdown in production

---

## References

- [Worker trait documentation](https://docs.rs/roboplc/latest/roboplc/controller/trait.Worker.html)
- [WorkerOpts derive macro](https://docs.rs/roboplc/latest/roboplc/controller/derive.WorkerOpts.html)
- [Controller documentation](https://docs.rs/roboplc/latest/roboplc/controller/index.html)
- [RoboPLC examples](https://github.com/roboplc/roboplc/tree/main/examples)
- [Blocking vs non-blocking workers discussion](https://info.bma.ai/en/actual/roboplc/controller.html#blocking-vs-non-blocking-workers)
- [Locking safety documentation](https://docs.rs/roboplc/latest/roboplc/index.html#locking-safety)

---

## Conclusion

**RoboPLC does not support async workers.** The Worker trait requires a synchronous `fn run()` method. The `blocking` attribute is a supervisor optimization hint, not an async indicator. Workers that block indefinitely should use `blocking = true`, while periodic workers should use default (non-blocking) settings.

If you need to handle concurrent operations in a worker, consider running a tokio runtime **inside** the synchronous `run()` method, or spawning additional threads using Rust's threading primitives.
