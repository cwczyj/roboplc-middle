// =============================================================================
// RPC Worker - TCP 连接处理
// =============================================================================
// 这个模块处理单个 TCP 连接的读取和写入

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};

use roboplc_rpc::{dataformat::Json, server::RpcServer};

use super::handler::RpcHandler;

/// Handle a single TCP connection
pub async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), std::io::Error> {
    // Read request with timeout
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // Connection closed
            Ok(Ok(n)) => {
                request_payload.extend_from_slice(&buf[..n]);
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, error = %e, "Read error");
                return Err(e);
            }
            Err(_) => {
                // Timeout - no more data coming
                break;
            }
        }
    }

    if request_payload.is_empty() {
        return Ok(());
    }

    // Create RpcServer for this connection (fresh each time to avoid generic complexity)
    // Use spawn_blocking because RpcServer::handle_request_payload calls blocking_send/blocking_recv
    let handler = (*handler).clone();
    let response_payload = tokio::task::spawn_blocking(move || {
        let server = RpcServer::new(handler);
        server.handle_request_payload::<Json>(&request_payload, addr)
    })
    .await
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Write response with timeout if there is one
    if let Some(response_payload) = response_payload {
        timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await??;
    }

    Ok(())
}