//! Hub send protection module
//!
//! Provides deadlock-prevention for Hub.send() operations using a dedicated thread pool.
//! When Hub's internal queue is full, send() blocks. This module uses a thread pool
//! to process send operations with timeout protection, preventing thread starvation
//! under high load.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, Sender};
use once_cell::sync::Lazy;
use roboplc::prelude::Hub;

use crate::{Message, DEFAULT_HUB_SEND_TIMEOUT_MS};

/// Default timeout for Hub send operations
pub const DEFAULT_HUB_SEND_TIMEOUT: Duration =
    Duration::from_millis(DEFAULT_HUB_SEND_TIMEOUT_MS as u64);

/// Number of worker threads in the Hub send pool
const HUB_SEND_POOL_SIZE: usize = 4;

/// Maximum capacity of the Hub send task queue
const HUB_SEND_QUEUE_CAPACITY: usize = 128;

/// Task for Hub send operations
type HubSendTask = (Hub<Message>, Message, mpsc::Sender<()>);

/// Global Hub send thread pool
static HUB_SEND_POOL: Lazy<HubSendPool> = Lazy::new(HubSendPool::new);

/// Thread pool for Hub.send() operations
struct HubSendPool {
    sender: Sender<HubSendTask>,
}

impl HubSendPool {
    /// Create a new Hub send thread pool
    fn new() -> Self {
        let (sender, receiver) = bounded::<HubSendTask>(HUB_SEND_QUEUE_CAPACITY);

        // Spawn worker threads
        for worker_id in 0..HUB_SEND_POOL_SIZE {
            let rx = receiver.clone();
            thread::spawn(move || {
                tracing::debug!(worker_id = worker_id, "Hub send worker thread started");
                while let Ok((hub, message, completed_tx)) = rx.recv() {
                    hub.send(message);
                    // Send completion signal, ignore errors (receiver may have timed out)
                    let _ = completed_tx.send(());
                }
                tracing::debug!(worker_id = worker_id, "Hub send worker thread exiting");
            });
        }

        tracing::info!(
            pool_size = HUB_SEND_POOL_SIZE,
            queue_capacity = HUB_SEND_QUEUE_CAPACITY,
            "Hub send thread pool initialized"
        );

        Self { sender }
    }

    /// Submit a Hub send task to the pool
    fn submit(
        &self,
        hub: Hub<Message>,
        message: Message,
        completed_tx: mpsc::Sender<()>,
    ) -> Result<(), String> {
        self.sender
            .try_send((hub, message, completed_tx))
            .map_err(|e| format!("Failed to submit Hub send task: {:?}", e))?;
        Ok(())
    }
}

/// Send a message to Hub with timeout protection using a thread pool.
///
/// This function prevents indefinite blocking when Hub's internal
/// queue is full. It submits the send operation to a dedicated thread pool
/// and times out if the send doesn't complete within the specified duration.
///
/// # Arguments
/// * `hub` - Reference to the Hub
/// * `message` - Message to send
/// * `timeout` - Maximum time to wait for send to complete
///
/// # Returns
/// * `Ok(())` - Message was sent successfully
/// * `Err(String)` - Timeout or other error
///
/// # Example
/// ```ignore
/// use crate::hub_protection::send_to_hub_with_protection;
/// use std::time::Duration;
///
/// if let Err(e) = send_to_hub_with_protection(&hub, message, Duration::from_millis(500)) {
///     tracing::warn!("Hub send failed: {}", e);
/// }
/// ```
pub fn send_to_hub_with_protection(
    hub: &Hub<Message>,
    message: Message,
    timeout: Duration,
) -> Result<(), String> {
    let hub = hub.clone();
    let (completed_tx, completed_rx) = mpsc::channel();

    // Submit task to thread pool
    HUB_SEND_POOL
        .submit(hub, message, completed_tx)
        .map_err(|e| format!("Hub send queue full: {}", e))?;

    completed_rx.recv_timeout(timeout).map_err(|e| {
        tracing::warn!(
            timeout_ms = timeout.as_millis(),
            error = ?e,
            "Hub send timeout - system may be overloaded"
        );
        "Hub send timeout - system overloaded".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_reasonable() {
        assert!(DEFAULT_HUB_SEND_TIMEOUT >= Duration::from_millis(100));
        assert!(DEFAULT_HUB_SEND_TIMEOUT <= Duration::from_secs(1));
    }

    #[test]
    fn hub_send_pool_size_is_valid() {
        assert!(HUB_SEND_POOL_SIZE >= 1);
        assert!(HUB_SEND_POOL_SIZE <= 16);
    }

    #[test]
    fn hub_send_queue_capacity_is_valid() {
        assert!(HUB_SEND_QUEUE_CAPACITY >= 64);
        assert!(HUB_SEND_QUEUE_CAPACITY <= 1024);
    }
}
