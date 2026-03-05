# Async RpcWorker Architecture Design

## Overview

This document describes the async architecture design for converting RpcWorker from a blocking implementation to a fully asynchronous implementation using tokio. The current implementation uses `blocking = true` in worker_opts and std threads, which limits concurrency and efficiency.

## Design Goals

1. **No blocking operations** - All I/O operations must be async
2. **Concurrent request handling** - Multiple RPC connections processed simultaneously  
3. **Proper timeout handling** - Non-blocking timeouts using tokio primitives
4. **Correct response channel lifecycle** - Proper cleanup of oneshot channels

---

## Current Implementation Analysis

### Blocking Components to Replace

| Component | Current (Blocking) | Target (Async) |
|-----------|-------------------|----------------|
| Worker type | `blocking = true` | Remove blocking flag |
| TCP Listener | `std::net::TcpListener` | `tokio::net::TcpListener` |
| Channel (device control) | `std::sync::mpsc` | `tokio::sync::mpsc` |
| Channel (response) | `std::sync::mpsc` | `tokio::sync::oneshot` |
| Timeout | `recv_timeout()` | `tokio::time::timeout()` |
| Sleep | `std::thread::sleep()` | `tokio::time::sleep()` |
| Main loop | `while` + `match` | `tokio::select!` |

---

## Tokio Runtime Integration

The RpcWorker runs in a RoboPLC worker context which provides its own threading model. To integrate tokio, we follow the same pattern as HttpWorker: create a tokio runtime inside the worker and run async code within it.

### Pattern (from HttpWorker lines 231-250)

```rust
// In run() method of RpcWorker:
std::thread::spawn(move || {
    // Create multi-threaded tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("RpcWorker: failed to create Tokio runtime");
    
    // Run async main loop
    rt.block_on(async move {
        // Async code here
        async_main_loop().await;
    });
});
```

### Key Points

- Use `new_multi_thread()` for true parallelism (multiple connections)
- `enable_all()` enables both I/O and timer drivers
- `block_on()` runs the async main loop from blocking context
- Wrap in `std::thread::spawn()` to not block the worker thread

---

## Channel Replacement

### Device Control Channel (mpsc)

**Old (blocking):**
```rust
use std::sync::mpsc::{channel, Sender};

let (device_control_tx, device_control_rx) = channel::<DeviceControlRequest>();
```

**New (async):**
```rust
use tokio::sync::mpsc;

let (device_control_tx, device_control_rx) = mpsc::channel::<DeviceControlRequest>(100);
//                                                              ^^^^ buffer size
```

### Response Channel (oneshot)

**Old (blocking):**
```rust
use std::sync::mpsc::channel;

let (response_tx, response_rx) = channel();
match response_rx.recv_timeout(Duration::from_secs(30)) { ... }
```

**New (async):**
```rust
use tokio::sync::oneshot;

let (response_tx, response_rx) = oneshot::channel();
match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
    Ok(Ok((success, data, error))) => { ... },
    Ok(Err(_)) => { /* sender dropped */ },
    Err(_) => { /* timeout */ },
}
```

### Type Alias Updates

```rust
// Old
pub type ResponseSender = std::sync::mpsc::Sender<(bool, JsonValue, Option<String>)>;

// New  
pub type ResponseSender = tokio::sync::oneshot::Sender<(bool, JsonValue, Option<String>)>;

// And for device control channel
use tokio::sync::mpsc::Sender as DeviceControlSender;
```

---

## Main Loop Design

The current main loop uses blocking `listener.accept()` with manual sleep. The async version uses `tokio::select!` to concurrently handle multiple events.

### Async Main Loop Structure

```rust
async fn run_async_server(
    config: Config,
    context: &Context<Message, Variables>,
) -> WResult {
    let port = config.server.rpc_port;
    let bind_addr = format!("0.0.0.0:{}", port);
    
    // Create async TCP listener
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    
    // Create tokio channel for device control
    let (device_control_tx, mut device_control_rx) = mpsc::channel(100);
    
    // Create shared handler state
    let device_ids: Vec<String> = config.devices.iter().map(|d| d.id.clone()).collect();
    let handler = Arc::new(RpcHandler::new(
        device_ids, 
        device_control_tx, 
        context.hub().clone(),
    ));
    
    tracing::info!("RPC Server Worker started on {}", bind_addr);
    
    loop {
        tokio::select! {
            // Accept new TCP connections
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let handler = handler.clone();
                        // Spawn async task for each connection
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, handler).await {
                                tracing::warn!(%addr, %e, "connection handler error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(%e, "accept error");
                    }
                }
            }
            
            // Handle device control messages from Hub
            Some(request) = device_control_rx.recv() => {
                // Forward to device manager via Hub
                let respond_to = request.respond_to;
                let message = Message::DeviceControl {
                    device_id: request.device_id,
                    operation: request.operation,
                    params: request.params,
                    correlation_id: request.correlation_id,
                    respond_to: Some(respond_to),
                };
                context.hub().send(message);
            }
            
            // Check worker online status (periodic check)
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // This tick allows checking context.is_online() periodically
            }
        }
        
        // Exit if worker is offline
        if !context.is_online() {
            break;
        }
    }
    
    Ok(())
}
```

### tokio::select! Benefits

1. **Concurrent handling** - Multiple branches progress simultaneously
2. **Fair scheduling** - No branch can starve others
3. **Non-blocking** - Returns immediately when any branch completes
4. **Composition** - Can combine futures, streams, and channels

---

## Connection Handling

Each TCP connection is handled by a spawned async task, enabling true concurrency.

### Connection Handler

```rust
async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set up read/write timeout
    let mut buf = [0u8; 4096];
    let mut request_payload = Vec::new();
    
    // Use tokio I/O - returns WouldBlock on timeout
    loop {
        match tokio::time::timeout(
            Duration::from_millis(200),
            stream.read(&mut buf)
        ).await {
            Ok(Ok(0)) => break, // Connection closed
            Ok(Ok(n)) => {
                request_payload.extend_from_slice(&buf[..n]);
            }
            Ok(Err(e)) if e.kind() == ErrorKind::WouldBlock => {
                break; // No more data
            }
            Ok(Err(e)) => {
                tracing::warn!(%addr, %e, "read error");
                return Err(Box::new(e));
            }
            Err(_) => {
                // Timeout - no data received
                break;
            }
        }
    }
    
    if request_payload.is_empty() {
        return Ok(());
    }
    
    // Process RPC request (sync, but fast)
    let server = RpcServer::new((*handler).clone());
    if let Some(response_payload) = server.handle_request_payload::<Json>(&request_payload, addr) {
        // Write response with timeout
        tokio::time::timeout(
            Duration::from_secs(5),
            stream.write_all(&response_payload)
        ).await??;
    }
    
    Ok(())
}
```

---

## Response Handling

The RpcHandler needs to send responses back to the waiting connection using oneshot channels.

### Updated send_device_control

```rust
impl RpcHandler {
    async fn send_device_control(
        &self,
        device_id: &str,
        operation: Operation,
        params: JsonValue,
    ) -> RpcResult<RpcResultType> {
        let correlation_id = next_correlation_id();
        
        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();
        
        let request = DeviceControlRequest {
            device_id: device_id.to_string(),
            operation,
            params,
            correlation_id,
            respond_to: response_tx,
        };
        
        // Send request (async channel send)
        if self.device_control_tx.send(request).await.is_err() {
            return Ok(RpcResultType::Error {
                error: "Internal error: device control channel closed".to_string(),
            });
        }
        
        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
            Ok(Ok((success, data, error))) => {
                if success {
                    Ok(RpcResultType::Data { data })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                }
            }
            Ok(Err(_)) => {
                // Channel closed (sender dropped)
                Ok(RpcResultType::Error {
                    error: "Response channel disconnected".to_string(),
                })
            }
            Err(_) => {
                // Timeout
                tracing::warn!(correlation_id, "Request timed out, sending cleanup");
                self.hub.send(Message::TimeoutCleanup { correlation_id });
                Ok(RpcResultType::Error {
                    error: "Request timed out".to_string(),
                })
            }
        }
    }
}
```

---

## Timeout Handling

### Timeout Patterns

| Operation | Blocking (Old) | Async (New) |
|-----------|----------------|-------------|
| Channel receive | `recv_timeout(dur)` | `tokio::time::timeout(dur, rx).await` |
| TCP read | `set_read_timeout()` | `tokio::time::timeout(dur, stream.read()).await` |
| TCP write | `set_write_timeout()` | `tokio::time::timeout(dur, stream.write_all()).await` |
| Sleep | `thread::sleep(dur)` | `tokio::time::sleep(dur).await` |

### Proper Timeout Usage

```rust
use tokio::time::{timeout, Duration, Instant};

// For operations that might hang
let result = timeout(Duration::from_secs(30), async_operation).await;

match result {
    Ok(Ok(success)) => { /* operation succeeded */ }
    Ok(Err(e)) => { /* operation failed with error */ }
    Err(_) => { /* timeout - operation took too long */ }
}

// For periodic tasks
let mut interval = tokio::time::interval(Duration::from_secs(5));
loop {
    interval.tick().await;
    do_periodic_work().await;
}
```

---

## Worker Configuration Change

### Before (Blocking Worker)

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "rpc_server", blocking = true)]
pub struct RpcWorker {
    config: Config,
}
```

### After (Async Worker)

```rust
#[derive(WorkerOpts)]
#[worker_opts(name = "rpc_server", cpu = 1, scheduling = "fifo", priority = 80)]
//                                                        ^^^^^^^^^^^^^^^^
//                                                        Optional: adjust for async workload
pub struct RpcWorker {
    config: Config,
}

impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        // Spawn thread with tokio runtime
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("RpcWorker: failed to create Tokio runtime");
            
            rt.block_on(async move {
                if let Err(e) = run_async_server(self.config.clone(), context).await {
                    tracing::error!(%e, "async server error");
                }
            });
        });
        
        Ok(())
    }
}
```

---

## Summary of Changes

### 1. Worker Attribute
```rust
// Remove: blocking = true
#[worker_opts(name = "rpc_server")]
```

### 2. Imports
```rust
// Add tokio imports
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, sleep, Duration, Instant};
use tokio::select;
```

### 3. Channel Types
- `std::sync::mpsc::Sender` → `tokio::sync::mpsc::Sender`
- `std::sync::mpsc::Receiver` → `tokio::sync::mpsc::Receiver`
- `std::sync::mpsc::channel()` → `tokio::sync::oneshot::channel()`

### 4. Runtime Integration
- Wrap async code in `std::thread::spawn()` with tokio runtime
- Use `rt.block_on()` to run async main loop

### 5. Main Loop
- Replace `while` + `match` with `tokio::select!`
- Handle multiple concurrent operations

### 6. Timeout Handling
- Replace `recv_timeout()` with `tokio::time::timeout()`
- Replace `thread::sleep()` with `tokio::time::sleep()`

---

## Implementation Checklist

- [ ] Remove `blocking = true` from `#[worker_opts]`
- [ ] Add tokio imports
- [ ] Replace std channels with tokio channels
- [ ] Update type aliases for ResponseSender
- [ ] Create async main loop function
- [ ] Use `tokio::net::TcpListener` instead of `std::net::TcpListener`
- [ ] Replace main loop with `tokio::select!`
- [ ] Update `send_device_control` to async
- [ ] Replace timeout handling with `tokio::time::timeout`
- [ ] Spawn connection handlers as separate tasks
- [ ] Test concurrent request handling
- [ ] Test timeout behavior
- [ ] Test graceful shutdown

---

## References

- [tokio runtime builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html)
- [tokio::select](https://docs.rs/tokio/latest/tokio/macro.select.html)
- [tokio::sync::mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html)
- [tokio::sync::oneshot](https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html)
- [tokio::time](https://docs.rs/tokio/latest/tokio/time/index.html)
