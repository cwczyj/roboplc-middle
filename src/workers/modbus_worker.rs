use crate::config::Device;
use crate::{Message, Variables};
use roboplc::comm::Client;
use roboplc::controller::prelude::*;
use roboplc::io::modbus::prelude::*;
use roboplc::{comm::tcp, time::interval};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MODBUS_TIMEOUT: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

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
    last_heartbeat: SystemTime,
    pending_transactions: HashMap<u16, TransactionId>,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        Self {
            device,
            client: None,
            last_heartbeat: SystemTime::UNIX_EPOCH,
            pending_transactions: HashMap::new(),
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

    fn ensure_connected(&mut self) -> bool {
        if self.client.is_none() {
            if let Err(e) = self.connect() {
                tracing::warn!(device_id = %self.device.id, error = %e, "Connection failed");
                return false;
            }
        }

        if let Some(client) = &mut self.client {
            if let Err(e) = client.ensure_connected() {
                tracing::warn!(device_id = %self.device.id, error = %e, "Reconnection failed");
                self.client = None;
                return false;
            }
        }

        self.client.is_some()
    }
}

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let _mapping_placeholder: Option<ModbusMapping> = None;

        for _ in interval(Duration::from_millis(100)).take_while(|_| context.is_online()) {
            self.prune_stale_transactions(Duration::from_secs(5));

            if !self.ensure_connected() {
                std::thread::sleep(RECONNECT_DELAY);
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
        assert!(worker.pending_transactions.is_empty());
    }
}
