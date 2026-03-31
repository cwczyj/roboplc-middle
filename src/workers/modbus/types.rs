//! Type definitions for Modbus worker

use crate::{DEFAULT_OPERATION_TIMEOUT_MS, MAX_OPERATION_TIMEOUT_MS};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

// ==================== 常量定义 ====================

const BASE_TIMEOUT: Duration = Duration::from_millis(DEFAULT_OPERATION_TIMEOUT_MS as u64);
const MAX_TIMEOUT: Duration = Duration::from_millis(MAX_OPERATION_TIMEOUT_MS as u64);
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

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
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

impl Default for TimeoutHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== OperationQueue ====================

/// Thread-safe capacity limiter for concurrent operations.
///
/// Uses atomic counter for thread-safe capacity management without locks.
pub struct OperationQueue<T> {
    pending: VecDeque<T>,
    in_flight: AtomicUsize,
    max_in_flight: usize,
}

impl<T> OperationQueue<T> {
    const MAX_CAS_RETRIES: usize = 100;

    pub fn new(max_in_flight: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: AtomicUsize::new(0),
            max_in_flight,
        }
    }

    pub fn push(&mut self, op: T) {
        self.pending.push_back(op);
    }

    pub fn pop_if_ready(&mut self) -> Option<T> {
        for attempt in 0..Self::MAX_CAS_RETRIES {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.max_in_flight {
                return None;
            }
            if self
                .in_flight
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                if let Some(op) = self.pending.pop_front() {
                    return Some(op);
                }
                self.in_flight.fetch_sub(1, Ordering::Release);
                return None;
            }
            if attempt > 10 {
                std::hint::spin_loop();
            }
        }
        tracing::debug!("OperationQueue::pop_if_ready CAS retried");
        None
    }

    pub fn complete(&self) {
        for attempt in 0..Self::MAX_CAS_RETRIES {
            let current = self.in_flight.load(Ordering::Acquire);
            if current == 0 {
                break;
            }
            if self
                .in_flight
                .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
            if attempt > 10 {
                std::hint::spin_loop();
            }
        }
        tracing::debug!("OperationQueue::complete CAS retried");
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn can_start(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) < self.max_in_flight
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Try to atomically acquire capacity without modifying pending queue.
    /// Returns true if capacity was acquired, false if at max capacity.
    pub fn try_acquire_atomic(&self) -> bool {
        for attempt in 0..Self::MAX_CAS_RETRIES {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.max_in_flight {
                return false;
            }
            if self
                .in_flight
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
            if attempt > 10 {
                std::hint::spin_loop();
            }
        }
        tracing::debug!(
            "OperationQueue::try_acquire_atomic CAS failed after {} attempts",
            Self::MAX_CAS_RETRIES
        );
        false
    }
}

// ==================== OperationGuard ====================

/// RAII guard that ensures operation capacity is released
/// even if the operation panics.
pub struct OperationGuard {
    on_drop: Option<Box<dyn FnOnce()>>,
}

impl OperationGuard {
    /// Create a new guard with a callback that runs on drop.
    pub fn new<F>(on_drop: F) -> Self
    where
        F: FnOnce() + 'static,
    {
        Self {
            on_drop: Some(Box::new(on_drop)),
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Some(on_drop) = self.on_drop.take() {
            on_drop();
        }
    }
}

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

    #[test]
    fn operation_guard_releases_on_drop() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(1));
        let counter_clone = counter.clone();

        {
            let _guard = OperationGuard::new(move || {
                counter_clone.fetch_sub(1, Ordering::SeqCst);
            });
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        }

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "Guard should release capacity on drop"
        );
    }

    #[test]
    fn operation_guard_releases_on_panic() {
        use std::panic::catch_unwind;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(1));
        let counter_clone = counter.clone();

        let result = catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = OperationGuard::new(move || {
                counter_clone.fetch_sub(1, Ordering::SeqCst);
            });
            panic!("Simulated panic");
        }));

        assert!(result.is_err(), "Panic should propagate");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "Guard should release capacity even on panic"
        );
    }

    #[test]
    fn test_cas_retry_limit() {
        use std::sync::Arc;

        let queue = Arc::new(OperationQueue::<i32>::new(1));
        let mut success_count = 0;

        for _ in 0..1000 {
            if queue.try_acquire_atomic() {
                success_count += 1;
            }
        }

        assert_eq!(success_count, 1);
    }

    #[test]
    fn test_concurrent_push_with_limit() {
        use std::sync::Arc;
        use std::thread;

        let queue = Arc::new(std::sync::Mutex::new(OperationQueue::<TestOperation>::new(
            3,
        )));
        let mut handles = vec![];

        for i in 0..100 {
            let q = queue.clone();
            handles.push(thread::spawn(move || {
                let mut q = q.lock().unwrap();
                q.push(TestOperation {
                    id: i as u32,
                    data: i,
                });
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let q = queue.lock().unwrap();
        assert_eq!(q.pending_count(), 100);
    }
}
