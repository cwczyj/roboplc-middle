//! Hub send protection module
//!
//! Provides deadlock-prevention for Hub.send() operations.
//! When Hub's internal queue is full, send() blocks. This module
//! spawns a separate thread for the send operation with a timeout,
//! allowing the caller to continue if send doesn't complete quickly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use roboplc::prelude::Hub;

use crate::Message;

/// Default timeout for Hub send operations (500ms)
pub const DEFAULT_HUB_SEND_TIMEOUT: Duration = Duration::from_millis(500);

/// Send a message to Hub with timeout protection.
///
/// This function prevents indefinite blocking when Hub's internal
/// queue is full. It spawns a separate thread for the blocking send
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
    let send_completed = Arc::new(AtomicBool::new(false));
    let send_completed_clone = send_completed.clone();

    let send_thread = std::thread::spawn(move || {
        hub.send(message);
        send_completed_clone.store(true, Ordering::SeqCst);
    });

    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if send_completed.load(Ordering::SeqCst) {
            // Send completed successfully
            // Join the thread to ensure proper cleanup
            let _ = send_thread.join();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    tracing::warn!(
        timeout_ms = timeout.as_millis(),
        "Hub send timeout - system may be overloaded"
    );

    Err("Hub send timeout - system overloaded".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_reasonable() {
        assert!(DEFAULT_HUB_SEND_TIMEOUT >= Duration::from_millis(100));
        assert!(DEFAULT_HUB_SEND_TIMEOUT <= Duration::from_secs(1));
    }
}
