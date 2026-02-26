use crate::config::Device;
use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::comm::Client;
use roboplc::controller::prelude::*;
use roboplc::io::modbus::prelude::*;
use roboplc::{comm::tcp, time::interval};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MODBUS_TIMEOUT: Duration = Duration::from_secs(1);
const BACKOFF_BASE_MS: u64 = 100;
const BACKOFF_MAX_MS: u64 = 30000;

static TRANSACTION_COUNTER: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionId {
    pub id: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy)]
struct Backoff {
    attempts: u32,
    next_delay_ms: u64,
}

impl Backoff {
    fn new() -> Self {
        Self {
            attempts: 0,
            next_delay_ms: BACKOFF_BASE_MS,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let jitter = (self.next_delay_ms / 10) * (self.attempts as u64 % 3);
        let delay = self.next_delay_ms + jitter;

        self.attempts += 1;
        self.next_delay_ms = (self.next_delay_ms * 2).min(BACKOFF_MAX_MS);

        Duration::from_millis(delay)
    }

    fn reset(&mut self) {
        self.attempts = 0;
        self.next_delay_ms = BACKOFF_BASE_MS;
    }
}

#[allow(dead_code)]
struct OperationQueue<T> {
    pending: VecDeque<T>,
    in_flight: usize,
    max_in_flight: usize,
}

#[allow(dead_code)]
impl<T> OperationQueue<T> {
    fn new(max_in_flight: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: 0,
            max_in_flight,
        }
    }

    fn push(&mut self, op: T) {
        self.pending.push_back(op);
    }

    fn can_start(&self) -> bool {
        self.in_flight < self.max_in_flight
    }

    fn start_next(&mut self) -> Option<T> {
        if self.can_start() {
            if let Some(op) = self.pending.pop_front() {
                self.in_flight += 1;
                return Some(op);
            }
        }
        None
    }

    fn complete(&mut self) {
        if self.in_flight > 0 {
            self.in_flight -= 1;
        }
    }

    fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn in_flight_count(&self) -> usize {
        self.in_flight
    }
}

#[derive(Debug, Clone)]
pub enum ModbusOp {
    ReadHolding { address: u16, count: u16 },
    WriteSingle { address: u16, value: u16 },
    WriteMultiple { address: u16, values: Vec<u16> },
}

struct ModbusClient {
    endpoint: String,
    connection: Option<Client>,
}

impl ModbusClient {
    fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            connection: None,
        }
    }

    fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let client = tcp::connect(&self.endpoint, MODBUS_TIMEOUT)?;
        client.connect()?;
        self.connection = Some(client);
        Ok(())
    }

    fn reconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = &self.connection {
            client.reconnect();
        }
        self.connection = None;
        self.connect()
    }

    fn ensure_connected(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.connection {
            Some(client) => {
                if client.connect().is_err() {
                    self.reconnect()?;
                }
            }
            None => {
                self.connect()?;
            }
        }
        Ok(())
    }
}

#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    device: Device,
    client: Option<ModbusClient>,
    connection_state: ConnectionState,
    last_communication: Option<SystemTime>,
    last_heartbeat: SystemTime,
    pending_transactions: HashMap<u16, TransactionId>,
    #[allow(dead_code)]
    operation_queue: OperationQueue<ModbusOp>,
    backoff: Backoff,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        let max_in_flight = device.max_concurrent_ops as usize;

        Self {
            device,
            client: None,
            connection_state: ConnectionState::Disconnected,
            last_communication: None,
            last_heartbeat: SystemTime::UNIX_EPOCH,
            pending_transactions: HashMap::new(),
            operation_queue: OperationQueue::new(max_in_flight),
            backoff: Backoff::new(),
        }
    }

    fn track_transaction(&mut self) -> TransactionId {
        let tx = TransactionId::new();
        self.pending_transactions.insert(tx.id, tx);
        tx
    }

    fn prune_stale_transactions(&mut self, max_age: Duration) {
        self.pending_transactions
            .retain(|_, tx| tx.elapsed() <= max_age);
    }

    fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = format!("{}:{}", self.device.address, self.device.port);
        let mut client = ModbusClient::new(endpoint);
        client.connect()?;
        self.client = Some(client);
        tracing::info!(device_id = %self.device.id, "Connected to Modbus device");
        Ok(())
    }

    fn update_connection_state_with<F>(&mut self, new_state: ConnectionState, mut emit: F)
    where
        F: FnMut(DeviceEvent),
    {
        if self.connection_state != new_state {
            let event_type = match new_state {
                ConnectionState::Connected => DeviceEventType::Connected,
                ConnectionState::Disconnected => DeviceEventType::Disconnected,
                ConnectionState::Connecting => DeviceEventType::Reconnecting,
            };

            emit(DeviceEvent {
                device_id: self.device.id.clone(),
                event_type,
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                details: format!("Connection state: {:?}", new_state),
            });

            self.connection_state = new_state;
        }
    }

    fn update_connection_state(
        &mut self,
        new_state: ConnectionState,
        context: &Context<Message, Variables>,
    ) {
        self.update_connection_state_with(new_state, |event| {
            let _ = context.variables().device_events.force_push(event);
        });
    }

    fn record_communication_with<F>(&mut self, latency_us: u64, mut emit: F)
    where
        F: FnMut(LatencySample),
    {
        let now = SystemTime::now();
        self.last_communication = Some(now);

        let sample = LatencySample {
            device_id: 0,
            latency_us,
            timestamp_ms: now
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        emit(sample);
    }

    fn record_communication(&mut self, context: &Context<Message, Variables>, latency_us: u64) {
        self.record_communication_with(latency_us, |sample| {
            let _ = context.variables().latency_samples.force_push(sample);
        });
    }

    fn ensure_connected(&mut self, context: &Context<Message, Variables>) -> bool {
        if self.client.is_none() {
            self.update_connection_state(ConnectionState::Connecting, context);
            if let Err(e) = self.connect() {
                tracing::warn!(device_id = %self.device.id, error = %e, "Connection failed");
                self.update_connection_state(ConnectionState::Disconnected, context);
                return false;
            }
        }

        let reconnect_failed = if let Some(client) = &mut self.client {
            client.ensure_connected().is_err()
        } else {
            false
        };

        if reconnect_failed {
            self.client = None;
            self.update_connection_state(ConnectionState::Connecting, context);
            if let Err(e) = self.connect() {
                tracing::warn!(device_id = %self.device.id, error = %e, "Reconnection failed");
                self.update_connection_state(ConnectionState::Disconnected, context);
                return false;
            }
        }

        self.update_connection_state(ConnectionState::Connected, context);
        self.backoff.reset();
        true
    }
}

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let _mapping_placeholder: Option<ModbusMapping> = None;

        for _ in interval(Duration::from_millis(100)).take_while(|_| context.is_online()) {
            self.prune_stale_transactions(Duration::from_secs(5));

            if !self.ensure_connected(context) {
                std::thread::sleep(self.backoff.next_delay());
                continue;
            }

            let now = SystemTime::now();
            if now
                .duration_since(self.last_heartbeat)
                .unwrap_or(Duration::ZERO)
                >= Duration::from_secs(self.device.heartbeat_interval_sec as u64)
            {
                let _tx_id = self.track_transaction();
                let timestamp_ms = now
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                context.hub().send(Message::DeviceHeartbeat {
                    device_id: self.device.id.clone(),
                    timestamp_ms,
                    latency_us: 0,
                });
                self.record_communication(context, 0);
                self.last_heartbeat = now;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceType, RegisterMapping};
    use crate::{DeviceEventType, LatencySample};

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

    fn test_device() -> Device {
        Device {
            id: "test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: Default::default(),
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            heartbeat_interval_sec: 30,
            register_mappings: Vec::<RegisterMapping>::new(),
        }
    }

    #[test]
    fn modbus_client_new_starts_disconnected() {
        let client = ModbusClient::new("127.0.0.1:502".to_string());

        assert!(client.connection.is_none());
        assert_eq!(client.endpoint, "127.0.0.1:502");
    }

    #[test]
    fn worker_new_initializes_without_client() {
        let worker = ModbusWorker::new(test_device());

        assert!(worker.client.is_none());
        assert_eq!(worker.connection_state, ConnectionState::Disconnected);
        assert!(worker.last_communication.is_none());
        assert!(worker.pending_transactions.is_empty());
    }

    #[test]
    fn update_connection_state_emits_event_on_transition_only() {
        let mut worker = ModbusWorker::new(test_device());
        let mut emitted = Vec::new();

        worker
            .update_connection_state_with(ConnectionState::Connected, |event| emitted.push(event));
        worker
            .update_connection_state_with(ConnectionState::Connected, |event| emitted.push(event));
        worker
            .update_connection_state_with(ConnectionState::Connecting, |event| emitted.push(event));

        assert_eq!(emitted.len(), 2);
        assert!(matches!(emitted[0].event_type, DeviceEventType::Connected));
        assert!(matches!(
            emitted[1].event_type,
            DeviceEventType::Reconnecting
        ));
        assert_eq!(worker.connection_state, ConnectionState::Connecting);
    }

    #[test]
    fn record_communication_updates_timestamp_and_latency_sample() {
        let mut worker = ModbusWorker::new(test_device());
        let before = SystemTime::now();
        let mut emitted_sample: Option<LatencySample> = None;

        worker.record_communication_with(250, |sample| emitted_sample = Some(sample));

        assert!(worker.last_communication.is_some());
        assert!(worker.last_communication.unwrap() >= before);

        let sample = emitted_sample.expect("latency sample should be emitted");
        assert_eq!(sample.latency_us, 250);
        assert_eq!(sample.device_id, 0);
        assert!(sample.timestamp_ms > 0);
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

    #[test]
    fn operation_queue_limits_concurrency_and_tracks_in_flight() {
        let mut queue = OperationQueue::new(2);
        queue.push(ModbusOp::ReadHolding {
            address: 100,
            count: 2,
        });
        queue.push(ModbusOp::WriteSingle {
            address: 101,
            value: 42,
        });
        queue.push(ModbusOp::WriteMultiple {
            address: 102,
            values: vec![1, 2, 3],
        });

        assert_eq!(queue.pending_count(), 3);
        assert_eq!(queue.in_flight_count(), 0);
        assert!(queue.can_start());

        let op1 = queue.start_next();
        let op2 = queue.start_next();
        let op3 = queue.start_next();

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
        queue.push(ModbusOp::ReadHolding {
            address: 200,
            count: 1,
        });
        queue.push(ModbusOp::WriteSingle {
            address: 201,
            value: 7,
        });

        let first = queue.start_next();
        let blocked = queue.start_next();

        assert!(first.is_some());
        assert!(blocked.is_none());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 1);

        queue.complete();
        let second = queue.start_next();

        assert!(second.is_some());
        assert_eq!(queue.in_flight_count(), 1);
        assert_eq!(queue.pending_count(), 0);
    }

    #[test]
    fn operation_queue_complete_is_saturating_at_zero() {
        let mut queue: OperationQueue<ModbusOp> = OperationQueue::new(1);

        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);

        queue.push(ModbusOp::ReadHolding {
            address: 300,
            count: 1,
        });
        let _ = queue.start_next();
        assert_eq!(queue.in_flight_count(), 1);

        queue.complete();
        queue.complete();
        assert_eq!(queue.in_flight_count(), 0);
    }
}
