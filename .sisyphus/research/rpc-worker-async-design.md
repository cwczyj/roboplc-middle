# RpcWorker Async Architecture Design

**Date:** March 3, 2026  
**Task:** Task 5 - Design async architecture for RpcWorker refactoring

---

## 1. Executive Summary

This document describes the async architecture design for refactoring `RpcWorker` to handle concurrent RPC requests without blocking. The design integrates a tokio runtime inside a synchronous RoboPLC worker while maintaining full compatibility with RoboPLC's blocking worker model.

**Key Design Decision:** Use the HttpWorker pattern - spawn a dedicated thread with a tokio runtime inside, using `rt.block_on()` for async operations. Keep `blocking = true` in worker_opts for RoboPLC supervisor compatibility.

---

## 2. Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        RpcWorker (RoboPLC Worker)                       │
│                     fn run() - SYNCHRONOUS (blocking=true)              │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  std::thread::spawn(move || {  // Dedicated thread for async   │    │
│  │                                                                    │    │
│  │      let rt = tokio::runtime::Builder::new_multi_thread()      │    │
│  │          .enable_all().build().expect("tokio runtime");         │    │
│  │                                                                    │    │
│  │      rt.block_on(async_server_loop(context, config))           │    │
│  │                                                                    │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  while context.is_online() { std::thread::sleep(1sec); }               │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    async_server_loop (tokio context)                     │
│                                                                          │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐                 │
│  │  TCP Accept  │   │  Device      │   │  Shutdown   │                 │
│  │  (tokio::    │   │  Control     │   │  Signal     │                 │
│  │   net::)     │   │  (mpsc)      │   │  (oneshot)  │                 │
│  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘                 │
│         │                  │                  │                          │
│         ▼                  ▼                  ▼                          │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │               tokio::select! (concurrent handling)             │     │
│  │                                                                │     │
│  │   tokio::select! {                                             │     │
│  │       _ = accept_loop(&listener, &handler, &pending) => {}    │     │
│  │       Some(request) = device_control_rx.recv() => {            │     │
│  │           forward_to_hub(request, context);                     │     │
│  │       }                                                         │     │
│  │       _ = shutdown_rx.recv() => {                               │     │
│  │           break; // Graceful shutdown                           │     │
│  │       }                                                         │     │
│  │   }                                                             │     │
│  └────────────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    Connection Handler (per-connection)                   │
│                                                                          │
│  spawn(async move {                                                      │
│      // Handle TCP stream with tokio::net::TcpStream                    │
│      // Use timeout() for read/write operations                          │
│      // Call server.handle_request_payload()                            │
│      // Manage response channel with oneshot                             │
│  })                                                                      │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────┐     │
│  │  timeout(Duration::from_secs(5), stream.read_exact(&mut buf))  │     │
│  │  timeout(Duration::from_secs(30), response_rx.recv())          │     │
│  └────────────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Channel Architecture

### 3.1 Channel Types

| Channel | Type | Purpose | Direction |
|---------|------|---------|-----------|
| `device_control_tx/rx` | `tokio::sync::mpsc::Sender/Receiver<DeviceControlRequest>` | Forward device control requests to Hub | RpcWorker → Hub |
| `shutdown_tx/rx` | `tokio::sync::oneshot::Sender/Receiver<()>` | Signal graceful shutdown | Main loop → Async loop |
| `response_tx/rx` | `tokio::sync::oneshot::Sender/Receiver<DeviceResponseData>` | Return RPC response to handler | Hub → RpcWorker |

### 3.2 Channel Creation

```rust
// Inside async context
let (device_control_tx, device_control_rx) = tokio::sync::mpsc::channel(100);
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
```

### 3.3 Bridging std::sync::mpsc to tokio

The `Message` type expects `std::sync::mpsc::Sender<DeviceResponseData>`. We must bridge:

```rust
// In async handler when receiving from Hub
let (std_tx, std_rx) = std::sync::mpsc::channel::<DeviceResponseData>();

// Send Message with std_tx via context.hub().send()

// Use spawn_blocking to wait on std channel
let respond_to = request.respond_to.clone();
tokio::task::spawn_blocking(move || {
    match std_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(response) => {
            let _ = respond_to.send(response);
        }
        Err(_) => {
            tracing::warn!(correlation_id = request.correlation_id, "Response timeout");
        }
    }
});
```

---

## 4. Main Loop Design (tokio::select!)

### 4.1 Select Loop Structure

```rust
async fn async_server_loop(
    context: Context<Message, Variables>,
    config: Config,
    device_control_tx: tokio::sync::mpsc::Sender<DeviceControlRequest>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let bind_addr = format!("0.0.0.0:{}", config.server.rpc_port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind TCP listener");

    tracing::info!("Async RPC Server started on {}", bind_addr);

    // Pending requests tracking for cleanup
    let pending: Arc<tokio::sync::Mutex<HashMap<u64, PendingRequest>>> = 
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    loop {
        tokio::select! {
            // Handle incoming TCP connections
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let handler = create_handler(
                            device_control_tx.clone(),
                            pending.clone(),
                        );
                        // Spawn connection handler
                        tokio::spawn(handle_connection(stream, addr, handler));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Accept error");
                    }
                }
            }

            // Handle device control messages from our own channel
            Some(request) = device_control_rx.recv() => {
                // Forward to Hub
                let message = Message::DeviceControl {
                    device_id: request.device_id,
                    operation: request.operation,
                    params: request.params,
                    correlation_id: request.correlation_id,
                    respond_to: Some(create_std_sender(request.respond_to)),
                };
                context.hub().send(message);
            }

            // Handle shutdown signal
            _ = &mut shutdown_rx => {
                tracing::info!("Shutdown signal received");
                break;
            }

            // Periodic cleanup of timed-out requests
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                cleanup_timed_out_requests(pending.clone()).await;
            }
        }
    }
}
```

### 4.2 Connection Handler

```rust
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: RpcHandler,
) {
    // Set timeout for entire connection
    let timeout_duration = Duration::from_secs(30);
    
    // Read request with timeout
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];
    
    loop {
        match timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // Connection closed
            Ok(Ok(n)) => {
                request_payload.extend_from_slice(&buf[..n]);
            }
            Ok(Err(e)) => {
                tracing::warn!(addr = %addr, error = %e, "Read error");
                return;
            }
            Err(_) => {
                // Timeout - no more data
                break;
            }
        }
    }

    if request_payload.is_empty() {
        return;
    }

    // Create RpcServer for this connection
    let server = RpcServer::new(handler);
    
    // Process request
    if let Some(response_payload) = server.handle_request_payload::<Json>(&request_payload, addr) {
        // Write response with timeout
        if let Err(e) = timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await {
            tracing::warn!(addr = %addr, error = %e, "Write error");
        }
    }
}
```

---

## 5. Timeout Handling

### 5.1 Request Timeout Pattern

Instead of `std::sync::mpsc::recv_timeout(30 seconds)`:

```rust
// Create oneshot channel for response
let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();

// Use tokio::time::timeout for async waiting
match tokio::time::timeout(Duration::from_secs(30), &mut response_rx).await {
    Ok(Ok(response)) => {
        // Handle successful response
    }
    Ok(Err(_)) => {
        // Channel closed (disconnected)
        tracing::error!("Response channel disconnected");
    }
    Err(_) => {
        // Timeout - request took too long
        tracing::warn!(correlation_id, "Request timed out after 30s");
        // Send cleanup message
        context.hub().send(Message::TimeoutCleanup { correlation_id });
    }
}
```

### 5.2 Read/Write Timeouts

```rust
// Read with timeout
let read_result = tokio::time::timeout(
    Duration::from_millis(500),
    stream.read_exact(&mut buf)
).await;

// Write with timeout
let write_result = tokio::time::timeout(
    Duration::from_secs(5),
    stream.write_all(&response_payload)
).await;
```

---

## 6. Cleanup Logic for Timed-Out Requests

### 6.1 Pending Request Tracking

```rust
#[derive(Clone)]
struct PendingRequest {
    correlation_id: u64,
    created_at: Instant,
    respond_to: tokio::sync::oneshot::Sender<DeviceResponseData>,
}

async fn cleanup_timed_out_requests(
    pending: Arc<tokio::sync::Mutex<HashMap<u64, PendingRequest>>>,
) {
    let timeout_duration = Duration::from_secs(35); // Slightly longer than request timeout
    
    let mut pending_lock = pending.lock().await;
    let now = Instant::now();
    
    let timed_out: Vec<u64> = pending_lock
        .iter()
        .filter(|(_, req)| now.duration_since(req.created_at) > timeout_duration)
        .map(|(&id, _)| id)
        .collect();
    
    for id in timed_out {
        if let Some(req) = pending_lock.remove(&id) {
            // Send error response
            let _ = req.respond_to.send(DeviceResponseData {
                success: false,
                data: serde_json::json!({}),
                error: Some("Request timed out during cleanup".to_string()),
            });
            
            // Notify Hub about timeout
            tracing::warn!(correlation_id = id, "Cleaned up timed-out request");
        }
    }
}
```

---

## 7. Implementation Plan

### Phase 1: Setup Tokio Runtime (1 step)

1. Modify `RpcWorker::run()` to spawn a dedicated thread with tokio runtime
   - Keep `blocking = true` in worker_opts
   - Use HttpWorker pattern: `std::thread::spawn(move || { ... rt.block_on(...) })`

### Phase 2: Convert to Async Listeners (1 step)

1. Replace `std::net::TcpListener` with `tokio::net::TcpListener`
   - Change from `listener.set_nonblocking(true)` to `listener.accept()` in select

### Phase 3: Implement tokio::select! Main Loop (1 step)

1. Create `async_server_loop` function
   - Implement tokio::select! for concurrent accept/recv/shutdown handling
   - Use `tokio::sync::mpsc` for device_control channel

### Phase 4: Convert Timeout Handling (1 step)

1. Replace `recv_timeout(30s)` with `tokio::time::timeout`
   - Use oneshot channels for response waiting

### Phase 5: Add Cleanup Logic (1 step)

1. Implement pending request tracking with HashMap + Mutex
   - Add periodic cleanup in select loop

---

## 8. Key Code Changes Summary

### 8.1 Type Changes

| Before | After |
|--------|-------|
| `std::sync::mpsc::Sender<DeviceControlRequest>` | `tokio::sync::mpsc::Sender<DeviceControlRequest>` |
| `std::sync::mpsc::Sender<(bool, JsonValue, Option<String>)>` | `tokio::sync::oneshot::Sender<DeviceResponseData>` |
| `std::net::TcpListener` | `tokio::net::TcpListener` |
| `std::net::TcpStream` | `tokio::net::TcpStream` |
| `recv_timeout(30s)` | `tokio::time::timeout(30s, receiver).await` |

### 8.2 Blocking to Async Conversions

```rust
// BEFORE (sync):
let response_rx = channel();
match response_rx.recv_timeout(Duration::from_secs(30)) { ... }

// AFTER (async):
let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
match tokio::time::timeout(Duration::from_secs(30), response_rx).await { ... }
```

```rust
// BEFORE (sync - try_recv in loop):
while let Ok(request) = device_control_rx.try_recv() { ... }

// AFTER (async):
tokio::select! {
    Some(request) = device_control_rx.recv() => { ... }
    ...
}
```

---

## 9. Risk Mitigation

### 9.1 Compatibility Risks

| Risk | Mitigation |
|------|------------|
| RoboPLC doesn't support async workers | Use blocking=true, spawn tokio in dedicated thread |
| Message type uses std::mpsc | Bridge with spawn_blocking |
| RpcServerHandler is sync | Use blocking_send/blocking_recv |

### 9.2 Performance Risks

| Risk | Mitigation |
|------|------------|
| Thread overhead | Single tokio runtime, spawn connection handlers as tasks |
| Channel bottlenecks | Use channel size 100 for device_control |
| Memory from pending requests | Cleanup every 10 seconds |

---

## 10. References

- HttpWorker pattern: `src/workers/http_worker.rs` lines 230-276
- RoboPLC async constraints: `.sisyphus/research/roboplc-async-support.md`
- Learnings from previous work: `.sisyphus/notepads/rpc-worker-deadlock-fix/learnings.md`