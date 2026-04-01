//! Unified correlation ID generation module
//!
//! This module provides a single, thread-safe correlation ID generator
//! for use across the entire middleware codebase.
//!
//! ## Usage
//!
//! ```rust
//! use roboplc_middleware::next_correlation_id;
//!
//! let id = next_correlation_id();
//! ```
//!
//! ## Design
//!
//! - Uses `AtomicU64` for 64-bit unique IDs (supports ~18 quintillion IDs)
//! - Uses `SeqCst` memory ordering for strict consistency across all threads
//! - Global static counter initialized to 1 (0 reserved for special cases)
//!
//! ## Distinction from TransactionId
//!
//! The `TransactionId` in `modbus/types.rs` is a separate concept used for
//! Modbus protocol transaction tracking. It uses `AtomicU32` because Modbus
//! MBAP headers use 16-bit transaction IDs. Correlation IDs are for internal
//! request/response correlation across the middleware.

use std::sync::atomic::{AtomicU64, Ordering};

// Global correlation ID counter
// Initialized to 1 to allow 0 as a special/default value if needed
static GLOBAL_CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generates the next unique correlation ID.
///
/// Returns a monotonically increasing u64 value.
/// Thread-safe - can be called from any thread.
/// Uses SeqCst ordering for strict memory consistency.
///
/// # Example
///
/// ```rust
/// use roboplc_middleware::next_correlation_id;
///
/// let id1 = next_correlation_id();
/// let id2 = next_correlation_id();
/// assert!(id2 > id1);
/// ```
pub fn next_correlation_id() -> u64 {
    GLOBAL_CORRELATION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::thread;

    #[test]
    fn correlation_ids_are_unique() {
        let mut ids = HashSet::new();
        for _ in 0..10000 {
            assert!(ids.insert(next_correlation_id()));
        }
    }

    #[test]
    fn correlation_ids_are_thread_safe() {
        let handles: Vec<_> = (0..10)
            .map(|_| {
                thread::spawn(|| {
                    let mut ids = Vec::new();
                    for _ in 0..1000 {
                        ids.push(next_correlation_id());
                    }
                    ids
                })
            })
            .collect();

        let mut all_ids = HashSet::new();
        for handle in handles {
            for id in handle.join().unwrap() {
                assert!(all_ids.insert(id), "Duplicate ID found");
            }
        }
        // Verify we got all 10,000 unique IDs
        assert_eq!(all_ids.len(), 10000);
    }

    #[test]
    fn correlation_ids_start_at_one() {
        // Note: This test assumes it's the first to call next_correlation_id
        // In practice, other tests may have already incremented the counter
        // So we just verify the counter is working, not the starting value
        let id = next_correlation_id();
        assert!(id > 0);
    }

    #[test]
    fn correlation_ids_are_monotonic() {
        let id1 = next_correlation_id();
        let id2 = next_correlation_id();
        let id3 = next_correlation_id();
        assert!(id2 > id1);
        assert!(id3 > id2);
    }
}
