use crate::config::Config;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Correlation ID for request/response matching
static CORRELATION_ID: AtomicU64 = AtomicU64::new(0);

/// Pending request tracking for async response routing
#[derive(Debug)]
struct PendingRequest {
    correlation_id: u64,
    device_id: String,
    operation: crate::Operation,
    response_tx: Sender<DeviceResponseData>,
    timestamp_ms: u64,
}

/// Device response data structure
#[derive(Debug, Clone)]
struct DeviceResponseData {
    success: bool,
    data: serde_json::Value,
    error: Option<String>,
}

#[derive(WorkerOpts)]
#[worker_opts(name = "device_manager")]
pub struct DeviceManager {
    config: Config,
    pending_requests: HashMap<u64, PendingRequest>,
    request_timeout_ms: u64,
}

impl DeviceManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            pending_requests: HashMap::new(),
            request_timeout_ms: 5000, // 5 second timeout
        }
    }

    fn next_correlation_id() -> u64 {
        CORRELATION_ID.fetch_add(1, Ordering::SeqCst)
    }

    fn cleanup_expired_requests(&mut self, now_ms: u64) {
        self.pending_requests
            .retain(|_, req| now_ms.saturating_sub(req.timestamp_ms) < self.request_timeout_ms);
    }
}

impl Worker<Message, Variables> for DeviceManager {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let client = context.hub().register(
            "device_manager",
            event_matches!(Message::DeviceControl { .. } | Message::DeviceResponse { .. }),
        )?;

        tracing::info!(
            "Device Manager started, routing {} devices",
            self.config.devices.len()
        );

        for msg in client {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;


            if now_ms % 1000 == 0 {
                self.cleanup_expired_requests(now_ms);
            }

            match msg {
                Message::DeviceControl {
                    device_id,
                    operation,
                    params,
                } => {
                    // TODO: Forward to appropriate Modbus worker

                    tracing::debug!(
                        device_id = %device_id,
                        operation = ?operation,
                        "Received DeviceControl request"
                    );
                }
                Message::DeviceResponse {
                    device_id,
                    success,
                    data,
                    error,
                } => {
                    // TODO: Route response back to requester
                    tracing::debug!(
                        device_id = %device_id,
                        success = success,
                        "Received DeviceResponse"
                    );
                }
                _ => {}
            }
        }

        tracing::info!("Device Manager stopped");
        Ok(())
    }
}
