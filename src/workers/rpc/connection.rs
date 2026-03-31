use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};

use roboplc_rpc::{dataformat::Json, server::RpcServer};

use super::handler::RpcHandler;

const MAX_REQUEST_SIZE: usize = 1024 * 1024;

pub async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    addr: SocketAddr,
    handler: Arc<RpcHandler>,
) -> Result<(), std::io::Error> {
    let mut request_payload = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break,
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
            Err(_) => {
                break;
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
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    if let Some(response_payload) = response_payload {
        timeout(Duration::from_secs(5), stream.write_all(&response_payload)).await??;
    }

    Ok(())
}
