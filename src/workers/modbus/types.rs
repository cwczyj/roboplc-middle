//! Type definitions for Modbus worker

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

// ==================== 常量定义 ====================

const BASE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const BACKOFF_BASE_MS: u64 = 100;
const BACKOFF_MAX_MS: u64 = 30000;

// 全局事务计数器
static TRANSACTION_COUNTER: AtomicU32 = AtomicU32::new(0);

// ==================== TransactionId ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionId {
    pub id: u32,
    pub created_at: SystemTime,
}

impl TransactionId {
    pub fn new() -> Self {
        Self {
            id: TRANSACTION_COUNTER.fetch_add(1, Ordering::SeqCst),
            created_at: SystemTime::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.created_at.elapsed().unwrap_or(Duration::ZERO)
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== ConnectionState ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

// ==================== Backoff 指数退避 ====================

#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

impl Backoff {
    pub fn new() -> Self {
        Self {
            attempts: 0,
            next_delay_ms: BACKOFF_BASE_MS,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let jitter = (self.next_delay_ms / 10) * (self.attempts as u64 % 3);
        let delay = self.next_delay_ms + jitter;

        self.attempts += 1;
        self.next_delay_ms = (self.next_delay_ms * 2).min(BACKOFF_MAX_MS);

        Duration::from_millis(delay)
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
        self.next_delay_ms = BACKOFF_BASE_MS;
    }
}

// ==================== TimeoutHandler ====================

#[derive(Debug, Clone, Copy)]
pub struct TimeoutHandler {
    current: Duration,
    base: Duration,
    max: Duration,
}

impl TimeoutHandler {
    pub fn new() -> Self {
        Self {
            current: BASE_TIMEOUT,
            base: BASE_TIMEOUT,
            max: MAX_TIMEOUT,
        }
    }

    pub fn timeout(&self) -> Duration {
        self.current
    }

    pub fn on_timeout(&mut self) {
        self.current = (self.current * 2).min(self.max);
    }

    pub fn on_success(&mut self) {
        self.current = self.base;
    }

    pub fn is_at_max(&self) -> bool {
        self.current >= self.max
    }
}

// ==================== OperationQueue ====================

pub struct OperationQueue<T> {
    pending: VecDeque<T>,
    in_flight: usize,
    max_in_flight: usize,
}

impl<T> OperationQueue<T> {
    /// Creates a new operation queue with the specified maximum concurrent operations.
    pub fn new(max_in_flight: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: 0,
            max_in_flight,
        }
    }

    /// Adds an operation to the pending queue.
    pub fn push(&mut self, op: T) {
        self.pending.push_back(op);
    }

    /// Attempts to pop an operation from the queue if capacity is available.
    /// Returns `Some(op)` if an operation was started, `None` if at capacity or queue empty.
    pub fn pop_if_ready(&mut self) -> Option<T> {
        if self.in_flight < self.max_in_flight {
            if let Some(op) = self.pending.pop_front() {
                self.in_flight += 1;
                return Some(op);
            }
        }
        None
    }

    /// Marks an operation as complete, releasing capacity for the next operation.
    /// Saturates at zero (safe to call when no operations are in flight).
    pub fn complete(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }
}

#[cfg(test)]
impl<T> OperationQueue<T> {
    fn can_start(&self) -> bool {
        self.in_flight < self.max_in_flight
    }
    fn pending_count(&self) -> usize {
        self.pending.len()
    }
    fn in_flight_count(&self) -> usize {
        self.in_flight
    }
}

// Re-export for external use (types are already public, but this makes the API cleaner)
// Note: OperationQueue needs to remain pub for external use

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_id_increments() {
        let id1 = TransactionId::new();
        let id2 = TransactionId::new();
        assert_ne!(id1.id, id2.id);
    }

    #[test]
    fn transaction_id_has_timestamp() {
        let id = TransactionId::new();
        assert!(id.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn backoff_new_starts_at_base_delay() {
        let backoff = Backoff::new();

        assert_eq!(backoff.attempts, 0);
        assert_eq!(backoff.next_delay_ms, BACKOFF_BASE_MS);
    }

    #[test]
    fn backoff_next_delay_is_exponential_and_capped() {
        let mut backoff = Backoff::new();

        let d1 = backoff.next_delay();
        let d2 = backoff.next_delay();
        let d3 = backoff.next_delay();

        assert_eq!(d1, Duration::from_millis(100));
        assert_eq!(d2, Duration::from_millis(220));
        assert_eq!(d3, Duration::from_millis(480));

        for _ in 0..20 {
            backoff.next_delay();
        }

        assert!(backoff.next_delay_ms <= BACKOFF_MAX_MS);
    }

    #[test]
    fn backoff_reset_restores_initial_state() {
        let mut backoff = Backoff::new();
        let _ = backoff.next_delay();
        let _ = backoff.next_delay();

        backoff.reset();

        assert_eq!(backoff.attempts, 0);
        assert_eq!(backoff.next_delay_ms, BACKOFF_BASE_MS);
    }

    // Simple test wrapper to test OperationQueue without ModbusOp dependency
    #[derive(Debug, Clone)]
    struct TestOperation {
        id: u32,
        data: i32,
    }

    #[test]
    fn operation_queue_limits_concurrency_and_tracks_in_flight() {
        let mut queue = OperationQueue::new(2);
        queue.push(TestOperation { id: 1, data: 100 });
        queue.push(TestOperation { id: 2, data: 200 });
        queue.push(TestOperation { id: 3, data: 300 });

        assert_eq!(queue.pending_count(), 3);
        assert_eq!(queue.in_flight_count(), 0);
        assert!(queue.can_start());

        let op1 = queue.pop_if_ready();
        let op2 = queue.pop_if_ready();
        let op3 = queue.pop_if_ready();

        assert!(op1.is_some());
        assert!(op2.is_some());
        assert!(op3.is_none());
        assert_eq!(queue.in_flight_count(), 2);
        assert_eq!(queue.pending_count(), 1);
        assert!(!queue.can_start());
    }

    #[test]
    fn operation_queue_complete_allows_next_queued_operation() {
        let mut queue = OperationQueue::new(1);
        queue.push(TestOperation { id: 1, data: 100 });
        queue.push(TestOperation { id: 2, data: 200 });

        let first = queue.pop_if_ready();
        let blocked = queue.pop_if_ready();

        assert!(first.is_some());
        assert!(blocked.is_none());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 1);

        queue.complete();
        let second = queue.pop_if_ready();

        assert!(second.is_some());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 0);
    }

    #[test]
    fn operation_queue_complete_is_saturating_at_zero() {
        let mut queue: OperationQueue<TestOperation> = OperationQueue::new(1);

        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);

        queue.push(TestOperation { id: 1, data: 100 });
        let _ = queue.pop_if_ready();
        assert_eq!(queue.in_flight_count(), 1);

        queue.complete();
        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);
    }

    #[test]
    fn operation_queue_pop_if_ready_returns_none_on_empty_queue() {
        let mut queue: OperationQueue<TestOperation> = OperationQueue::new(2);
        let result = queue.pop_if_ready();
        assert!(result.is_none());
        assert_eq!(queue.in_flight_count(), 0);
    }

    #[test]
    fn operation_queue_maintains_fifo_order() {
        let mut queue = OperationQueue::new(3);
        queue.push(TestOperation { id: 1, data: 100 });
        queue.push(TestOperation { id: 2, data: 200 });
        queue.push(TestOperation { id: 3, data: 300 });

        let op1 = queue.pop_if_ready();
        let op2 = queue.pop_if_ready();
        let op3 = queue.pop_if_ready();

        assert_eq!(op1.unwrap().id, 1);
        assert_eq!(op2.unwrap().id, 2);
        assert_eq!(op3.unwrap().id, 3);
    }

    #[test]
    fn operation_queue_new_with_zero_capacity() {
        let mut queue: OperationQueue<TestOperation> = OperationQueue::new(0);
        queue.push(TestOperation { id: 1, data: 100 });

        let result = queue.pop_if_ready();
        assert!(result.is_none());
        assert_eq!(queue.pending_count(), 1);
    }

    #[test]
    fn operation_queue_full_cycle() {
        let mut queue = OperationQueue::new(2);
        queue.push(TestOperation { id: 1, data: 100 });
        queue.push(TestOperation { id: 2, data: 200 });
        queue.push(TestOperation { id: 3, data: 300 });
        queue.push(TestOperation { id: 4, data: 400 });

        let op1 = queue.pop_if_ready();
        let op2 = queue.pop_if_ready();
        assert_eq!(op1.unwrap().id, 1);
        assert_eq!(op2.unwrap().id, 2);

        queue.complete();
        let op3 = queue.pop_if_ready();
        assert_eq!(op3.unwrap().id, 3);

        queue.complete();
        queue.complete();
        let op4 = queue.pop_if_ready();
        assert_eq!(op4.unwrap().id, 4);

        queue.complete();
        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);
        assert_eq!(queue.pending_count(), 0);
    }
}
