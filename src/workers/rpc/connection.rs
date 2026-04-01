use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};

use roboplc_rpc::{dataformat::Json, server::RpcServer};

use super::handler::RpcHandler;

const MAX_REQUEST_SIZE: usize = 1024 * 1024;
/// Short timeout for detecting if more data is coming after initial read
const READ_MORE_TIMEOUT_MS: u64 = 50;

/// Check if the payload appears to be a complete JSON document.
/// JSON-RPC requests end with '}' (possibly followed by whitespace).
/// This is a lightweight check without full parsing for performance.
fn is_json_complete(payload: &[u8]) -> bool {
    if payload.is_empty() {
        return false;
    }
    
    // Trim trailing whitespace (including \r, \n, spaces)
    let end_pos = payload.iter().rposition(|&b| !b.is_ascii_whitespace());
    if let Some(pos) = end_pos {
        // JSON object ends with '}'
        if payload[pos] == b'}' {
            // Also verify it starts with '{' for object (JSON-RPC uses objects)
            let start_pos = payload.iter().position(|&b| !b.is_ascii_whitespace());
            if let Some(start) = start_pos {
                return payload[start] == b'{';
            }
        }
        // JSON array ends with ']' - also accept for batch requests
        if payload[pos] == b']' {
            let start_pos = payload.iter().position(|&b| !b.is_ascii_whitespace());
            if let Some(start) = start_pos {
                return payload[start] == b'[';
            }
        }
    }
    false
}

pub async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), std::io::Error> {
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];

    // First read: wait for initial data with longer timeout
    match timeout(Duration::from_millis(3000), stream.read(&mut buf)).await {
        Ok(Ok(0)) => return Ok(()), // Connection closed immediately
        Ok(Ok(n)) => {
            request_payload.extend_from_slice(&buf[..n]);
            
            if request_payload.len() > MAX_REQUEST_SIZE {
                tracing::warn!(
                    addr = %addr,
                    size = request_payload.len(),
                    max = MAX_REQUEST_SIZE,
                    "Request exceeds maximum size, rejecting"
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Request too large",
                ));
            }
        }
        Ok(Err(e)) => {
            tracing::debug!(addr = %addr, error = %e, "Read error");
            return Err(e);
        }
        Err(_) => return Ok(()), // Timeout with no data
    }

    // Check if JSON is complete after first read
    if is_json_complete(&request_payload) {
        tracing::debug!(addr = %addr, size = request_payload.len(), "Complete JSON detected, processing immediately");
    } else {
        // Continue reading with short timeout to catch fragmented data
        loop {
            match timeout(Duration::from_millis(READ_MORE_TIMEOUT_MS), stream.read(&mut buf)).await {
                Ok(Ok(0)) => break, // Connection closed by client
                Ok(Ok(n)) => {
                    request_payload.extend_from_slice(&buf[..n]);

                    if request_payload.len() > MAX_REQUEST_SIZE {
                        tracing::warn!(
                            addr = %addr,
                            size = request_payload.len(),
                            max = MAX_REQUEST_SIZE,
                            "Request exceeds maximum size, rejecting"
                        );
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Request too large",
                        ));
                    }

                    // Check completeness after each additional read
                    if is_json_complete(&request_payload) {
                        tracing::debug!(addr = %addr, size = request_payload.len(), "Complete JSON detected after additional read");
                        break;
                    }
                }
                Ok(Err(e)) => {
                    tracing::debug!(addr = %addr, error = %e, "Read error");
                    return Err(e);
                }
                Err(_) => {
                    // Short timeout - assume what we have is the complete request
                    // (or client is slow/using keep-alive without shutdown)
                    tracing::debug!(addr = %addr, size = request_payload.len(), "Short timeout, processing received data");
                    break;
                }
            }
        }
    }

    if request_payload.is_empty() {
        return Ok(());
    }

    let handler = (*handler).clone();
    let response_payload = tokio::task::spawn_blocking(move || {
        let server = RpcServer::new(handler);
        server.handle_request_payload::<Json>(&request_payload, addr)
    })
    .await
    .map_err(std::io::Error::other)?;

    if let Some(response_payload) = response_payload {
        timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await??;
    }

    Ok(())
}
