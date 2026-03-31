use parking_lot_rt::RwLock;
use roboplc::prelude::Hub;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::messages::Message;

use super::connection::handle_connection;
use super::handler::RpcHandler;

struct ActiveConnections {
    tasks: RwLock<HashSet<tokio::task::Id>>,
}

impl ActiveConnections {
    fn new() -> Self {
        Self {
            tasks: RwLock::new(HashSet::new()),
        }
    }

    fn add(&self, id: tokio::task::Id) {
        self.tasks.write().insert(id);
    }

    fn remove(&self, id: tokio::task::Id) {
        self.tasks.write().remove(&id);
    }

    fn count(&self) -> usize {
        self.tasks.read().len()
    }
}

pub async fn run_async_server(
    bind_addr: String,
    device_ids: Vec<String>,
    hub: Hub<Message>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("RPC Server started on {}", bind_addr);

    let handler = Arc::new(RpcHandler::new(device_ids, hub.clone()));
    let connections = Arc::new(ActiveConnections::new());

    let mut task_set: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let handler = handler.clone();
                        let connections = connections.clone();
                        tracing::info!(addr = %addr, "Accepted new RPC connection");

                        let task = tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, handler).await {
                                tracing::debug!(addr = %addr, error = %e, "Connection error");
                            }
                        });

                        let task_id = task.id();
                        connections.add(task_id);

                        let connections_clone = connections.clone();
                        task_set.spawn(async move {
                            let _ = task.await;
                            connections_clone.remove(task_id);
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Accept error");
                    }
                }
            }

            _ = &mut shutdown_rx => {
                tracing::info!(
                    active_connections = connections.count(),
                    "Shutdown signal received, draining connections..."
                );

                break;
            }
        }
    }

    let drain_timeout = tokio::time::Duration::from_secs(5);
    let drain_start = std::time::Instant::now();

    while !task_set.is_empty() && drain_start.elapsed() < drain_timeout {
        tokio::select! {
            _ = task_set.join_next() => {
                tracing::debug!(
                    remaining = task_set.len(),
                    "Connection completed during drain"
                );
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
    }

    if !task_set.is_empty() {
        tracing::warn!(
            remaining = task_set.len(),
            "Force-closing remaining connections after drain timeout"
        );
        task_set.shutdown().await;
    }

    tracing::info!("RPC Server stopped");
    Ok(())
}
