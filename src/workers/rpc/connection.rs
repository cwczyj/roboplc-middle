use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};

use roboplc_rpc::{dataformat::Json, server::RpcServer};

use super::handler::RpcHandler;

const MAX_REQUEST_SIZE: usize = 1024 * 1024;
const INITIAL_TIMEOUT_MS: u64 = 3000;
const SUBSEQUENT_TIMEOUT_MS: u64 = 50;

pub async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), std::io::Error> {
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];
    let mut first_read = true;

    // Incremental JSON completeness tracking — scan only new bytes per read
    let mut brace_depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut json_started = false;

    loop {
        let timeout_ms = if first_read { INITIAL_TIMEOUT_MS } else { SUBSEQUENT_TIMEOUT_MS };
        first_read = false;

        match timeout(Duration::from_millis(timeout_ms), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                request_payload.extend_from_slice(&buf[..n]);

                if request_payload.len() > MAX_REQUEST_SIZE {
                    tracing::warn!(addr = %addr, size = request_payload.len(), max = MAX_REQUEST_SIZE, "Request too large");
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Request too large"));
                }

                for &b in &buf[..n] {
                    if escape {
                        escape = false;
                        continue;
                    }
                    if b == b'\\' && in_string {
                        escape = true;
                        continue;
                    }
                    if b == b'"' {
                        in_string = !in_string;
                        continue;
                    }
                    if in_string {
                        continue;
                    }
                    match b {
                        b'{' | b'[' => {
                            brace_depth += 1;
                            json_started = true;
                        }
                        b'}' | b']' => brace_depth -= 1,
                        _ => {}
                    }
                }

                if json_started && brace_depth == 0 {
                    tracing::debug!(addr = %addr, size = request_payload.len(), "Complete JSON detected");
                    break;
                }
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, error = %e, "Read error");
                return Err(e);
            }
            Err(_) => break,
        }
    }

    if request_payload.is_empty() {
        return Ok(());
    }

    // RpcServer::new is a zero-cost abstraction - it only stores the handler reference
    // and zero-sized PhantomData markers for type tracking. No heap allocations or
    // initialization overhead, so creating a new instance per request is optimal and
    // simpler than caching (which would require state management without any benefit).
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
