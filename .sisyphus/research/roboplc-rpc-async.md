# roboplc-rpc Async Compatibility Research

## Conclusion: NO - roboplc-rpc does NOT support async operations

**Version Investigated**: roboplc-rpc 0.1.8  
**Research Date**: March 3, 2026

---

## Evidence

### 1. RpcServerHandler Trait is Synchronous

The `RpcServerHandler` trait requires a synchronous `handle_call` method:

```rust
// Source: https://docs.rs/roboplc-rpc/0.1.8/src/roboplc_rpc/server.rs.html#85-97
pub trait RpcServerHandler<'a> {
    /// Methods to handle
    type Method: Deserialize<'a>;
    /// Result of the methods
    type Result: Serialize + Deserialize<'a>;
    /// Source of the call (IP address, etc.)
    type Source;

    /// A method to handle calls
    fn handle_call(&'a self, method: Self::Method, source: Self::Source)
        -> RpcResult<Self::Result>;
}
```

**Key Finding**: The trait definition uses `fn handle_call` (synchronous), NOT `async fn handle_call`.

---

### 2. RpcServer Implementation is Synchronous

The `RpcServer::handle_request` and `RpcServer::handle_request_payload` methods call the handler synchronously:

```rust
// Source: https://docs.rs/roboplc-rpc/0.1.8/src/roboplc_rpc/server.rs.html#46-57
/// Handle a JSON RPC request
pub fn handle_request(&'a self, request: Request<M>, source: SRC) -> Option<Response<R>> {
    let result = match self.rpc.handle_call(request.method, source) {
        Ok(v) => HandlerResponse::Ok(v),
        Err(e) => HandlerResponse::Err(RpcError {
            kind: e.kind,
            message: e.message,
        }),
    };
    request
        .id
        .map(move |id| Response::from_handler_response(id, result))
}
```

**Key Finding**: The method directly calls `self.rpc.handle_call(...)` without `.await`.

---

### 3. No Async Dependencies

The Cargo.toml shows no async runtime dependencies:

```toml
# Source: https://github.com/roboplc/roboplc-rpc/blob/main/Cargo.toml
[dependencies]
heapless = { version = "0.8", features = ["serde"] }
serde = { version = "1.0", default-features = false, features = ["derive"] }

# std (synchronous)
serde_json = { version = "1.0", optional = true }
tracing = { version = "0.1", optional = true }

# msgpack (synchronous)
rmp-serde = { version = "1.3", optional = true }

# http (synchronous)
http = { version = "^1.0.0", optional = true }
url = { version = "1.6", optional = true }
thiserror = { version = "2.0", optional = true }

[features]
default = ["std"]
canonical = []

std = ["serde_json", "tracing", "serde/std"]
msgpack = ["rmp-serde"]
http = ["dep:http", "url", "serde_json", "thiserror"]
full = ["std", "msgpack", "http"]
```

**Key Finding**: No `tokio`, `async-std`, `futures`, or other async runtime dependencies.

---

### 4. Example Code is Synchronous

The official example shows synchronous usage:

```rust
// Source: https://github.com/roboplc/roboplc-rpc/blob/main/examples/client-server.rs
struct MyRpc {}

impl<'a> server::RpcServerHandler<'a> for MyRpc {
    type Method = MyMethod<'a>;
    type Result = MyResult;
    type Source = &'static str;

    fn handle_call(&self, method: MyMethod, _source: Self::Source) -> RpcResult<MyResult> {
        match method {
            MyMethod::Test {} => Ok(MyResult::General { ok: true }),
            MyMethod::Hello { name } => Ok(MyResult::String(format!("Hello, {}", name))),
            // ...
        }
    }
}

fn main() {
    let server = server::RpcServer::new(MyRpc {});
    // Synchronous call
    if let Some(v) = server.handle_request_payload::<dataformat::Json>(req.payload(), "local") {
        // ...
    }
}
```

**Key Finding**: No async/await patterns in the official examples.

---

## Integration with Tokio

### Current Limitations

1. **Cannot use async handlers**: The `RpcServerHandler` trait requires synchronous methods
2. **Cannot await in handle_call**: Any async operations would need to block threads
3. **Protocol-agnostic design**: roboplc-rpc is designed to work with any transport layer, including non-async transports

### Workaround: Blocking Async Operations

If you need to perform async operations in a roboplc-rpc handler, you must block the async runtime:

```rust
use std::time::Duration;
use tokio::runtime::Runtime;

struct MyRpc {
    runtime: Runtime,
}

impl<'a> server::RpcServerHandler<'a> for MyRpc {
    type Method = MyMethod<'a>;
    type Result = MyResult;
    type Source = &'static str;

    fn handle_call(&self, method: MyMethod, _source: Self::Source) -> RpcResult<MyResult> {
        match method {
            MyMethod::Test {} => {
                // BLOCKING: Run async code in synchronous context
                let result = self.runtime.block_on(async {
                    // Your async operation here
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    true
                });
                Ok(MyResult::General { ok: result })
            },
            // ...
        }
    }
}
```

**Drawbacks**:
- Blocks the thread handling the request
- Not scalable under high load
- Defeats the purpose of async runtime

---

## Alternatives

### 1. Use Different RPC Library

Consider async-compatible JSON-RPC libraries:

- **jsonrpsee**: Fully async JSON-RPC library with tokio support
- **tarpc**: Async RPC framework for tokio
- **warp**: Async web framework with JSON-RPC support

### 2. Implement Async Wrapper Pattern

Wrap roboplc-rpc with an async facade:

```rust
// Define async handler trait
trait AsyncRpcHandler {
    async fn handle_call(&self, method: MyMethod) -> RpcResult<MyResult>;
}

// Implement sync wrapper
struct SyncRpcWrapper<H: AsyncRpcHandler> {
    handler: H,
    runtime: Runtime,
}

impl<'a, H: AsyncRpcHandler> server::RpcServerHandler<'a> for SyncRpcWrapper<H> {
    type Method = MyMethod<'a>;
    type Result = MyResult;
    type Source = &'static str;

    fn handle_call(&self, method: MyMethod, _source: Self::Source) -> RpcResult<MyResult> {
        self.runtime.block_on(self.handler.handle_call(method))
    }
}
```

### 3. Use Message Channel Pattern

Process RPC requests through an async channel:

```rust
use tokio::sync::mpsc;

struct AsyncRpcProcessor {
    tx: mpsc::Sender<RpcRequest>,
}

impl AsyncRpcProcessor {
    async fn process_requests(mut rx: mpsc::Receiver<RpcRequest>) {
        while let Some(req) = rx.recv().await {
            let response = self.handle_async(req.method).await;
            req.response_tx.send(response).ok();
        }
    }
}
```

---

## Recommendations

### For RoboPLC Workers

Since the project already uses RoboPLC framework:

1. **Keep roboplc-rpc synchronous**: Accept that RPC handlers are synchronous
2. **Move async operations to other workers**: Use RoboPLC's message passing to offload async work
3. **Use Hub for async communication**: Leverage RoboPLC's async Hub for cross-worker communication

Example pattern:
```rust
// In rpc_worker.rs
impl<'a> server::RpcServerHandler<'a> for MyRpc {
    fn handle_call(&self, method: MyMethod, source: Self::Source) -> RpcResult<MyResult> {
        // Send async request to another worker via Hub
        context.hub().send(Message::AsyncRequest {
            method,
            reply_to: source,
        });
        Ok(MyResult::Pending)
    }
}

// In async_worker.rs
async fn handle_async_request(method: MyMethod) -> MyResult {
    // Perform async operation here
    MyResult::General { ok: true }
}
```

### For New Projects

1. **Use jsonrpsee**: If you need async JSON-RPC from the start
2. **Consider direct tokio communication**: For worker-to-worker messaging
3. **Use async-compatible transports**: Like axum or warp for HTTP

---

## Conclusion

**roboplc-rpc v0.1.8 does NOT support async operations.** The library is designed for synchronous use cases and the `RpcServerHandler` trait enforces this.

**Best path forward**:
- Accept synchronous nature of roboplc-rpc
- Use RoboPLC's async Hub for async communication between workers
- Offload async operations to dedicated workers
- Consider alternative RPC libraries if async is a hard requirement

---

## Sources

- [roboplc-rpc Documentation](https://docs.rs/roboplc-rpc/0.1.8/roboplc_rpc/)
- [roboplc-rpc GitHub Repository](https://github.com/roboplc/roboplc-rpc)
- [server.rs Source Code](https://docs.rs/roboplc-rpc/0.1.8/src/roboplc_rpc/server.rs.html)
- [client-server.rs Example](https://github.com/roboplc/roboplc-rpc/blob/main/examples/client-server.rs)
- [Cargo.toml](https://github.com/roboplc/roboplc-rpc/blob/main/Cargo.toml)
