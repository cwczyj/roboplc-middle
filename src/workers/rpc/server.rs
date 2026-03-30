// =============================================================================
// RPC Worker - 异步服务器实现
// =============================================================================
// Wave 3 重构: 简化服务器主循环
// - 移除 device_control_rx 分支 (请求直接在 handler.rs 处理)
// - 移除 pending 超时清理 (请求在同一 blocking thread 中完成)
// - 简化参数列表

use roboplc::prelude::Hub;
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::messages::Message;

use super::handler::RpcHandler;
use super::connection::handle_connection;

pub async fn run_async_server(
    bind_addr: String,
    device_ids: Vec<String>,
    hub: Hub<Message>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("RPC Server started on {}", bind_addr);

    let handler = Arc::new(RpcHandler::new(device_ids, hub.clone()));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let handler = handler.clone();
                        tracing::info!(addr = %addr, "Accepted new RPC connection");
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, handler).await {
                                tracing::debug!(addr = %addr, error = %e, "Connection error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Accept error");
                    }
                }
            }

            _ = &mut shutdown_rx => {
                tracing::info!("Shutdown signal received, stopping RPC server");
                break;
            }
        }
    }

    Ok(())
}