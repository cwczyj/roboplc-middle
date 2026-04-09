//! # SSE Worker
//!
//! Server-Sent Events (SSE) streaming worker for real-time data updates.
//!
//! ## 功能
//!
//! - 接收 DataStreamUpdate 消息
//! - 管理 SSE 连接注册表
//! - 路由数据更新到连接的客户端
//!
//! ## 架构
//!
//! ```text
//! DataStreamWorker --DataStreamUpdate--> Hub --> SseWorker --> SSE Clients
//!                                                |
//!                                                v
//!                                         Connection Registry (DashMap)
//! ```
//!
//! ## Worker配置
//!
//! - 优先级: 60 (低于 DataStreamWorker 的 75，非 RT)
//! - 调度: other (非实时调度)
//! - CPU亲和性: 不绑定特定 CPU

use crate::{Message, Variables};
use dashmap::DashMap;
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{Receiver, Sender};

/// SSE事件数据
#[derive(Debug, Clone)]
pub enum SseEventData {
    JsonData(serde_json::Value),
    Heartbeat,
}

/// SSE连接唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SseConnectionId(u64);

impl SseConnectionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

/// SSE 连接信息
pub struct SseConnection {
    pub device_id: String,
    pub signal_groups: Vec<String>,
    pub connected_at: u64,
    pub sender: Sender<SseEventData>,
}

impl SseConnection {
    pub fn new(
        device_id: String,
        signal_groups: Vec<String>,
        connected_at: u64,
        sender: Sender<SseEventData>,
    ) -> Self {
        Self {
            device_id,
            signal_groups,
            connected_at,
            sender,
        }
    }

    pub fn is_subscribed_to(&self, signal_group: &str) -> bool {
        self.signal_groups.iter().any(|g| g == signal_group)
    }
}

impl std::fmt::Debug for SseConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SseConnection")
            .field("device_id", &self.device_id)
            .field("signal_groups", &self.signal_groups)
            .field("connected_at", &self.connected_at)
            .finish()
    }
}

/// SSE 连接注册表
pub struct SseConnectionRegistry {
    connections: DashMap<SseConnectionId, SseConnection>,
    next_id: AtomicU64,
}

impl SseConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
            next_id: AtomicU64::new(0),
        }
    }

    pub fn register(&self, connection: SseConnection) -> Result<SseConnectionId, String> {
        if self.connections.len() >= MAX_SSE_CONNECTIONS {
            return Err(format!(
                "Maximum SSE connections limit reached ({}). Rejecting new connection.",
                MAX_SSE_CONNECTIONS
            ));
        }
        let id = SseConnectionId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.connections.insert(id, connection);
        Ok(id)
    }

    pub fn unregister(&self, id: SseConnectionId) -> Option<Sender<SseEventData>> {
        self.connections.remove(&id).map(|(_, conn)| conn.sender)
    }

    pub fn count(&self) -> usize {
        self.connections.len()
    }

    pub fn send_to_subscribers(
        &self,
        device_id: &str,
        signal_group: &str,
        data: SseEventData,
    ) -> usize {
        let mut sent_count = 0;
        let mut dead_connections: Vec<SseConnectionId> = Vec::new();

        for entry in self.connections.iter() {
            let conn = entry.value();
            let device_match = conn.device_id == device_id;
            let signal_match = conn.is_subscribed_to(signal_group);

            if device_match && signal_match {
                match conn.sender.try_send(data.clone()) {
                    Ok(()) => {
                        sent_count += 1;
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        tracing::warn!(
                            device_id = %device_id,
                            signal_group = %signal_group,
                            connection_id = entry.key().value(),
                            "SSE channel full, skipping message"
                        );
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        dead_connections.push(*entry.key());
                    }
                }
            }
        }

        for conn_id in dead_connections {
            if let Some(sender) = self.unregister(conn_id) {
                drop(sender);
            }
            tracing::info!(
                connection_id = conn_id.value(),
                "Cleaned up dead SSE connection"
            );
        }

        sent_count
    }

    pub fn send_heartbeat_to_all(&self) -> usize {
        let mut sent_count = 0;
        let mut dead_connections: Vec<SseConnectionId> = Vec::new();

        for entry in self.connections.iter() {
            match entry.value().sender.try_send(SseEventData::Heartbeat) {
                Ok(()) => sent_count += 1,
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    dead_connections.push(*entry.key());
                }
            }
        }

        for conn_id in dead_connections {
            if let Some(sender) = self.unregister(conn_id) {
                drop(sender);
            }
            tracing::info!(
                connection_id = conn_id.value(),
                "Cleaned up dead SSE connection (heartbeat)"
            );
        }

        sent_count
    }

    pub fn all_connection_ids(&self) -> Vec<SseConnectionId> {
        self.connections.iter().map(|entry| *entry.key()).collect()
    }

    pub fn clear(&self) {
        self.connections.clear();
    }
}

impl Default for SseConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SseConnectionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SseConnectionRegistry")
            .field("connections_count", &self.connections.len())
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .finish()
    }
}

const SSE_HEARTBEAT_INTERVAL_SECS: u64 = 15;

const MAX_SSE_CONNECTIONS: usize = 100;

/// SSE Worker
#[derive(WorkerOpts)]
#[worker_opts(name = "sse_worker", scheduling = "other", priority = 60)]
pub struct SseWorker {
    messages_received: AtomicU64,
    messages_routed: AtomicU64,
    last_heartbeat_check: parking_lot_rt::RwLock<Instant>,
}

impl SseWorker {
    pub fn new() -> Self {
        Self {
            messages_received: AtomicU64::new(0),
            messages_routed: AtomicU64::new(0),
            last_heartbeat_check: parking_lot_rt::RwLock::new(Instant::now()),
        }
    }

    fn handle_data_stream_update(&self, msg: &Message, registry: &SseConnectionRegistry) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);

        if let Message::DataStreamUpdate {
            device_id,
            signal_group,
            values,
            timestamp_ms,
            latency_us,
            sequence,
        } = msg
        {
            let sse_data = serde_json::json!({
                "device_id": device_id,
                "signal_group": signal_group,
                "values": values,
                "timestamp_ms": timestamp_ms,
                "latency_us": latency_us,
                "sequence": sequence,
            });

            let sent_count = registry.send_to_subscribers(
                device_id,
                signal_group,
                SseEventData::JsonData(sse_data),
            );

            if sent_count > 0 {
                self.messages_routed
                    .fetch_add(sent_count as u64, Ordering::Relaxed);
            }
        }
    }

    fn send_heartbeat_if_needed(&self, registry: &SseConnectionRegistry) {
        let elapsed = self.last_heartbeat_check.read().elapsed();
        if elapsed >= Duration::from_secs(SSE_HEARTBEAT_INTERVAL_SECS) {
            *self.last_heartbeat_check.write() = Instant::now();

            let heartbeat_count = registry.send_heartbeat_to_all();
            if heartbeat_count > 0 {
                tracing::trace!(connection_count = heartbeat_count, "Sending SSE heartbeats");
            }
        }
    }

    pub fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    pub fn messages_routed(&self) -> u64 {
        self.messages_routed.load(Ordering::Relaxed)
    }
}

impl Default for SseWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl Worker<Message, Variables> for SseWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let client = context.hub().register(
            "sse_worker",
            event_matches!(Message::DataStreamUpdate { .. }),
        )?;

        tracing::info!("SseWorker started, listening for DataStreamUpdate messages");

        let registry = &context.variables().sse_registry;
        // NOTE: Do NOT use interval.tick() here - it blocks for the full interval duration,
        // causing message backlog. The send_heartbeat_if_needed() already uses non-blocking
        // elapsed() time check. We just call it on every message iteration for efficiency.
        for msg in client {
            if !context.is_online() {
                tracing::info!("SseWorker shutting down");
                break;
            }

            self.handle_data_stream_update(&msg, registry);
            self.send_heartbeat_if_needed(registry);
        }

        registry.clear();

        tracing::info!(
            messages_received = self.messages_received.load(Ordering::Relaxed),
            messages_routed = self.messages_routed.load(Ordering::Relaxed),
            "SseWorker stopped"
        );

        Ok(())
    }
}

const SSE_CHANNEL_CAPACITY: usize = 100;

pub fn create_sse_channel() -> (Sender<SseEventData>, Receiver<SseEventData>) {
    tokio::sync::mpsc::channel(SSE_CHANNEL_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::mpsc::channel;

    #[test]
    fn sse_connection_id_creation() {
        let id1 = SseConnectionId::new(1);
        let id2 = SseConnectionId::new(2);

        assert_eq!(id1.value(), 1);
        assert_eq!(id2.value(), 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn sse_event_data_creation() {
        let json_data = SseEventData::JsonData(serde_json::json!({"test": 1}));
        let heartbeat = SseEventData::Heartbeat;

        assert!(matches!(json_data, SseEventData::JsonData(_)));
        assert!(matches!(heartbeat, SseEventData::Heartbeat));
    }

    #[test]
    fn sse_connection_with_channel() {
        let (tx, _rx) = channel(10);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string(), "pressure".to_string()],
            1234567890,
            tx,
        );

        assert_eq!(conn.device_id, "plc-1");
        assert_eq!(conn.signal_groups, vec!["temperature", "pressure"]);
        assert_eq!(conn.connected_at, 1234567890);
        assert!(conn.is_subscribed_to("temperature"));
        assert!(conn.is_subscribed_to("pressure"));
        assert!(!conn.is_subscribed_to("humidity"));
    }

    #[test]
    fn sse_connection_registry_thread_safe_operations() {
        let registry = SseConnectionRegistry::new();

        assert_eq!(registry.count(), 0);

        let (tx1, _rx1) = channel(10);
        let conn1 = SseConnection::new("plc-1".to_string(), vec!["temp".to_string()], 1000, tx1);
        let id1 = registry.register(conn1).unwrap();
        assert_eq!(registry.count(), 1);

        let (tx2, _rx2) = channel(10);
        let conn2 =
            SseConnection::new("plc-2".to_string(), vec!["pressure".to_string()], 2000, tx2);
        let id2 = registry.register(conn2).unwrap();
        assert_eq!(registry.count(), 2);

        assert_ne!(id1, id2);

        assert!(registry.unregister(id1).is_some());
        assert_eq!(registry.count(), 1);

        assert!(registry.unregister(id1).is_none());
    }

    #[test]
    fn sse_connection_registry_send_to_subscribers() {
        let registry = SseConnectionRegistry::new();

        let (tx1, mut rx1) = channel(10);
        let conn1 = SseConnection::new(
            "plc-1".to_string(),
            vec!["temp".to_string(), "pressure".to_string()],
            1000,
            tx1,
        );
        registry.register(conn1).unwrap();

        let (tx2, mut rx2) = channel(10);
        let conn2 = SseConnection::new("plc-1".to_string(), vec!["temp".to_string()], 2000, tx2);
        registry.register(conn2).unwrap();

        let (tx3, _rx3) = channel(10);
        let conn3 = SseConnection::new("plc-2".to_string(), vec!["temp".to_string()], 3000, tx3);
        registry.register(conn3).unwrap();

        let sent_count = registry.send_to_subscribers(
            "plc-1",
            "temp",
            SseEventData::JsonData(serde_json::json!({"temp": 25.5})),
        );
        assert_eq!(sent_count, 2);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn sse_connection_registry_send_heartbeat() {
        let registry = SseConnectionRegistry::new();

        let (tx1, mut rx1) = channel(10);
        let conn1 = SseConnection::new("plc-1".to_string(), vec!["temp".to_string()], 1000, tx1);
        registry.register(conn1).unwrap();

        let (tx2, mut rx2) = channel(10);
        let conn2 =
            SseConnection::new("plc-2".to_string(), vec!["pressure".to_string()], 2000, tx2);
        registry.register(conn2).unwrap();

        let sent_count = registry.send_heartbeat_to_all();
        assert_eq!(sent_count, 2);

        assert!(matches!(rx1.try_recv().unwrap(), SseEventData::Heartbeat));
        assert!(matches!(rx2.try_recv().unwrap(), SseEventData::Heartbeat));
    }

    #[test]
    fn sse_worker_creation() {
        let worker = SseWorker::new();

        assert_eq!(worker.messages_received(), 0);
        assert_eq!(worker.messages_routed(), 0);
    }

    #[tokio::test]
    async fn sse_worker_routes_messages_to_subscribers() {
        let worker = SseWorker::new();
        let registry = SseConnectionRegistry::new();

        let (tx, mut rx) = channel(10);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1000,
            tx,
        );
        registry.register(conn).unwrap();

        let msg = Message::DataStreamUpdate {
            device_id: "plc-1".to_string(),
            signal_group: "temperature".to_string(),
            values: serde_json::json!({"temp": 25.5}),
            timestamp_ms: 1234567890,
            latency_us: 150,
            sequence: 1,
        };

        worker.handle_data_stream_update(&msg, &registry);

        assert_eq!(worker.messages_received(), 1);
        assert_eq!(worker.messages_routed(), 1);

        let received = rx.try_recv().unwrap();
        assert!(matches!(received, SseEventData::JsonData(_)));
    }

    #[tokio::test]
    async fn sse_worker_filters_non_matching_subscriptions() {
        let worker = SseWorker::new();
        let registry = SseConnectionRegistry::new();

        let (tx, mut rx) = channel(10);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1000,
            tx,
        );
        registry.register(conn).unwrap();

        let msg = Message::DataStreamUpdate {
            device_id: "plc-2".to_string(),
            signal_group: "temperature".to_string(),
            values: serde_json::json!({"temp": 30.0}),
            timestamp_ms: 1234567890,
            latency_us: 150,
            sequence: 1,
        };

        worker.handle_data_stream_update(&msg, &registry);

        assert_eq!(worker.messages_received(), 1);
        assert_eq!(worker.messages_routed(), 0);

        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sse_connection_registry_concurrent_access() {
        use std::thread;

        let registry = Arc::new(SseConnectionRegistry::new());
        let num_threads = 10;
        let mut handles = vec![];

        for i in 0..num_threads {
            let registry_clone = registry.clone();
            let handle = thread::spawn(move || {
                let (tx, _rx) = channel(10);
                let conn = SseConnection::new(
                    format!("device-{}", i),
                    vec![format!("group-{}", i)],
                    i as u64 * 1000,
                    tx,
                );
                registry_clone.register(conn);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.count(), num_threads);
    }

    #[test]
    fn sse_connection_registry_unique_ids_concurrent() {
        use std::thread;

        let registry = Arc::new(SseConnectionRegistry::new());
        let num_threads = 20;
        let mut handles: Vec<std::thread::JoinHandle<Result<SseConnectionId, String>>> = vec![];

        for i in 0..num_threads {
            let registry_clone = registry.clone();
            let handle = thread::spawn(move || {
                let (tx, _rx) = channel(10);
                let conn = SseConnection::new(
                    format!("device-{}", i),
                    vec![format!("group-{}", i)],
                    i as u64 * 1000,
                    tx,
                );
                registry_clone.register(conn)
            });
            handles.push(handle);
        }

        let ids: Vec<SseConnectionId> = handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();

        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "Duplicate ID detected: {}", id.value());
        }

        assert_eq!(registry.count(), num_threads);
    }

    #[test]
    fn sse_connection_registry_large_scale() {
        let registry = SseConnectionRegistry::new();
        let mut _receivers = Vec::new();

        // Test with connections under the limit (MAX_SSE_CONNECTIONS = 100)
        for i in 0..99 {
            let (tx, rx) = channel(10);
            _receivers.push(rx);
            let conn = SseConnection::new(
                format!("device-{}", i % 10),
                vec![format!("group-{}", i % 5)],
                i as u64,
                tx,
            );
            registry.register(conn).unwrap();
        }

        assert_eq!(registry.count(), 99);

        let sent_count =
            registry.send_to_subscribers("device-0", "group-0", SseEventData::Heartbeat);
        assert!(sent_count >= 9);

        let all_ids = registry.all_connection_ids();
        for id in all_ids.iter().take(49) {
            registry.unregister(*id);
        }

        assert_eq!(registry.count(), 50);
    }

    #[test]
    fn create_sse_channel_works() {
        let (tx, mut rx) = create_sse_channel();

        assert_eq!(tx.capacity(), SSE_CHANNEL_CAPACITY);

        tx.try_send(SseEventData::Heartbeat).unwrap();
        let received = rx.try_recv().unwrap();
        assert!(matches!(received, SseEventData::Heartbeat));
    }

    #[tokio::test]
    async fn sse_worker_sends_heartbeat() {
        let worker = SseWorker::new();
        let registry = SseConnectionRegistry::new();

        let (tx, mut rx) = channel(10);
        let conn = SseConnection::new("plc-1".to_string(), vec!["temp".to_string()], 1000, tx);
        registry.register(conn).unwrap();

        *worker.last_heartbeat_check.write() = Instant::now() - Duration::from_secs(20);
        worker.send_heartbeat_if_needed(&registry);

        let received = rx.try_recv().unwrap();
        assert!(matches!(received, SseEventData::Heartbeat));
    }

    #[tokio::test]
    async fn sse_worker_heartbeat_interval() {
        let worker = SseWorker::new();
        let registry = SseConnectionRegistry::new();

        let (tx, mut rx) = channel(10);
        let conn = SseConnection::new("plc-1".to_string(), vec!["temp".to_string()], 1000, tx);
        registry.register(conn).unwrap();

        worker.send_heartbeat_if_needed(&registry);
        assert!(rx.try_recv().is_err());

        *worker.last_heartbeat_check.write() = Instant::now() - Duration::from_secs(10);
        worker.send_heartbeat_if_needed(&registry);
        assert!(rx.try_recv().is_err());

        *worker.last_heartbeat_check.write() = Instant::now() - Duration::from_secs(15);
        worker.send_heartbeat_if_needed(&registry);
        assert!(rx.try_recv().is_ok());
    }
}
