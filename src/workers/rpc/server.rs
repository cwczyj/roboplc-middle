// =============================================================================
// RPC Worker - 异步服务器实现
// =============================================================================
// 这个模块实现了使用 tokio::select! 的异步服务器主循环

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use roboplc::prelude::Hub;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;

use crate::messages::Message;

use super::types::{DeviceControlRequest, PendingRequest};
use super::handler::RpcHandler;
use super::connection::handle_connection;
use super::request::handle_device_control_request;
use super::cleanup::cleanup_timed_out_requests;

/// Main async server loop using tokio::select! for concurrent handling
pub async fn run_async_server(
    bind_addr: String,
    device_ids: Vec<String>,
    // Note: device_control_tx is moved to RpcHandler, not used in select
    #[allow(unused_variables)] device_control_tx: mpsc::Sender<DeviceControlRequest>,
    mut device_control_rx: mpsc::Receiver<DeviceControlRequest>,
    hub: Hub<Message>,
    mut shutdown_rx: oneshot::Receiver<()>,
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Use tokio::net::TcpListener for async accept
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("RPC Server started on {}", bind_addr);

    // Create handler with device_control_tx
    let handler = Arc::new(RpcHandler::new(
        device_ids,
        device_control_tx.clone(),
        hub.clone(),
    ));

    // Main select loop
    loop {
        tokio::select! {
            // Handle incoming TCP connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let handler = handler.clone();
                        tracing::info!(stream = ?stream, addr = %addr, "Accepted new RPC connection");
                        // Spawn connection handler
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

            // Handle device control requests from RpcHandler
            // These need to be forwarded to the Hub
            Some(request) = async { device_control_rx.recv().await } => {
                handle_device_control_request(request, hub.clone(), pending.clone());
            }

            // Handle shutdown signal
            _ = &mut shutdown_rx => {
                tracing::info!("Shutdown signal received, stopping RPC server");
                break;
            }

            // Periodic cleanup of timed-out requests
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                cleanup_timed_out_requests(pending.clone(), hub.clone());
            }
        }
    }

    Ok(())
}