//! # roboplc-middleware
//!
//! 通信中间件，用于将 JSON-RPC 2.0 请求转换为 Modbus TCP 操作。
//! 该中间件作为 PLC 和机械臂设备的通信桥梁，提供统一的 API 接口。
//!
//! ## 架构概述
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
//! │   JSON-RPC      │────▶│  Device Manager  │────▶│   Modbus    │
//! │   Server        │     │  (Hub Router)    │     │   Workers   │
//! └─────────────────┘     └──────────────────┘     └─────────────┘
//!        │                        │                       │
//!        │                        ▼                       │
//!        │              ┌──────────────────┐             │
//!        └─────────────▶│  HTTP API       │◀────────────┘
//!                       │  /api/devices   │
//!                       │  /api/health    │
//!                       └──────────────────┘
//! ```
//!
//! ## 核心组件
//!
//! - **Workers**: 基于 RoboPLC 框架的实时工作线程
//!   - `RpcWorker`: 处理 JSON-RPC 请求 (端口 8080)
//!   - `ModbusWorker`: 管理 Modbus TCP 连接
//!   - `HttpWorker`: 提供 HTTP 管理接口 (端口 8081)
//!   - `ConfigLoader`: 热重载配置文件
//!   - `LatencyMonitor`: 监控设备延迟和异常检测
//!
//! - **Hub**: RoboPLC 消息路由系统，在 workers 之间传递消息
//!
//! - **Variables**: 跨 workers 的共享状态
//!   - 设备状态跟踪
//!   - 延迟样本数据
//!   - 事务日志
//!   - 事件流
//!
//! ## 使用方式
//!
//! 1. 创建 `config.toml` 配置文件
//! 2. 运行 `cargo run --release` 启动中间件
//! 3. 通过 JSON-RPC 或 HTTP API 与设备交互
//!
//! 详见 [USAGE.md](USAGE.md) 获取完整使用说明。

pub mod config;
pub mod correlation;
pub mod data_conversion;
pub mod hub_protection;
pub mod messages;

pub mod workers;

pub use hub_protection::{send_to_hub_with_protection, DEFAULT_HUB_SEND_TIMEOUT};

pub use correlation::next_correlation_id;

pub use workers::modbus::{
    parse_register_address, parse_signal_group_fields, Backoff, ConnectionState, ModbusClient,
    ModbusOp, OperationQueue, OperationResult, ParsedField, RegisterType, TimeoutHandler,
    TransactionId,
};

pub use messages::{DeviceResponseData, Message, Operation, SystemStatusResponse};

pub use workers::sse_worker::{
    create_sse_channel, SseConnection, SseConnectionId, SseConnectionRegistry, SseEventData,
};

use dashmap::DashMap;
use rtsc::buf::DataBuffer;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// =============================================================================
// Data Cache Structures
// =============================================================================

/// Data cache entry for streaming data mode
///
/// Stores the latest polled values from a signal group along with metadata
/// about the poll operation (timestamp, latency, error state).
///
/// # Fields
///
/// - `values`: The JSON values from the signal group read operation
/// - `timestamp_ms`: Unix timestamp when the data was cached (milliseconds)
/// - `latency_us`: Operation latency in microseconds
/// - `error`: Whether the read operation encountered an error
#[derive(Debug, Clone)]
pub struct DataCacheEntry {
    /// The JSON values from the signal group read operation
    pub values: JsonValue,

    /// Unix timestamp when the data was cached (milliseconds)
    pub timestamp_ms: u64,

    /// Operation latency in microseconds
    pub latency_us: u64,

    /// Whether the read operation encountered an error
    pub error: bool,
}

impl DataCacheEntry {
    /// Create a new cache entry
    pub fn new(values: JsonValue, timestamp_ms: u64, latency_us: u64, error: bool) -> Self {
        Self {
            values,
            timestamp_ms,
            latency_us,
            error,
        }
    }
}

/// Thread-safe in-memory data cache for streaming data mode
///
/// Stores the latest values for each signal group, keyed by device_id + signal_group.
/// Uses DashMap for lock-free concurrent access.
///
/// Cache key format: "{device_id}_{signal_group}"
#[derive(Debug, Clone)]
pub struct DataCache {
    /// Internal DashMap storing cache entries
    inner: Arc<DashMap<String, DataCacheEntry>>,
}

impl DataCache {
    /// Create a new empty DataCache
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Get a cache entry by key
    ///
    /// Returns `Some(DataCacheEntry)` if the key exists, `None` otherwise.
    pub fn get(&self, key: &str) -> Option<DataCacheEntry> {
        self.inner.get(key).map(|entry| entry.clone())
    }

    /// Set a cache entry by key
    ///
    /// Inserts or updates the cache entry for the given key.
    pub fn set(&self, key: &str, entry: DataCacheEntry) {
        self.inner.insert(key.to_string(), entry);
    }

    /// Get the age of a cache entry in milliseconds
    ///
    /// Returns `Some(age_ms)` if the key exists, `None` otherwise.
    /// Age is calculated from the entry's timestamp to the current time.
    pub fn get_age(&self, key: &str) -> Option<u64> {
        self.inner.get(key).map(|entry| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now.saturating_sub(entry.timestamp_ms)
        })
    }

    /// Check if a cache entry is fresh (not older than ttl_ms)
    ///
    /// Returns `true` if the entry exists and its age is <= ttl_ms.
    /// Returns `false` if the entry doesn't exist or is stale.
    pub fn is_fresh(&self, key: &str, ttl_ms: u64) -> bool {
        match self.get_age(key) {
            Some(age) => age <= ttl_ms,
            None => false,
        }
    }
}

impl Default for DataCache {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Channel Capacity Constants
// =============================================================================
// These constants define bounded channel capacities to prevent memory
// exhaustion under high load or slow consumer scenarios.
// Using sync_channel instead of unbounded channel provides backpressure.

/// Maximum pending heartbeat responses channel capacity.
/// Prevents memory exhaustion when heartbeat checks pile up due to
/// slow device responses or network issues.
pub const MAX_PENDING_HEARTBEATS: usize = 50;

/// Maximum pending RPC response channel capacity.
/// Prevents memory exhaustion when RPC responses pile up due to slow consumers.
pub const MAX_PENDING_RESPONSES: usize = 1000;

// =============================================================================
// Timeout Constants
// =============================================================================
// These constants define default timeout values used across the middleware.
// Centralizing them ensures consistency and makes tuning easier.

/// Default TCP connect timeout for Modbus connections (milliseconds)
pub const DEFAULT_CONNECT_TIMEOUT_MS: u16 = 200;

/// Default operation timeout for Modbus operations (milliseconds)
pub const DEFAULT_OPERATION_TIMEOUT_MS: u16 = 1000;

/// Maximum operation timeout for Modbus operations (milliseconds)
pub const MAX_OPERATION_TIMEOUT_MS: u16 = 30000;

/// Default timeout for Hub send operations (milliseconds)
pub const DEFAULT_HUB_SEND_TIMEOUT_MS: u16 = 500;

/// Default heartbeat timeout for device health checks (milliseconds)
pub const DEFAULT_HEARTBEAT_TIMEOUT_MS: u16 = 1000;

/// 设备状态跟踪
///
/// 记录每个设备的连接状态和通信指标，用于监控设备健康状态。
/// 该状态由 `ModbusWorker` 更新，并通过共享状态供其他 workers 访问。
///
/// # 字段说明
///
/// - `connected`: 设备当前是否已连接
/// - `last_communication`: 上一次成功通信的时间戳
/// - `error_count`: 累计的错误次数
/// - `reconnect_count`: 累计的重连次数
#[derive(Debug)]
pub struct DeviceStatus {
    /// 设备连接状态
    pub connected: bool,

    /// 上一次成功通信的时间
    pub last_communication: Instant,

    /// 累计错误计数
    pub error_count: u32,

    /// 累计重连计数
    pub reconnect_count: u32,
}

/// Modbus 事务日志条目
///
/// 记录每次 Modbus 操作的详细信息，用于调试和审计。
/// 该日志存储在循环缓冲区中，保留最近的操作记录。
///
/// # 字段说明
///
/// - `device_id`: 设备标识符
/// - `timestamp_ms`: 操作发生的 Unix 时间戳（毫秒）
/// - `operation`: 操作类型（如 "ReadHolding", "WriteSingle"）
/// - `address`: Modbus 地址
/// - `success`: 操作是否成功
/// - `latency_us`: 操作耗时（微秒）
#[derive(Clone, Debug)]
pub struct ModbusLogEntry {
    /// 设备标识符
    pub device_id: String,

    /// 时间戳（毫秒）
    pub timestamp_ms: u64,

    /// 操作类型描述
    pub operation: String,

    /// Modbus 地址
    pub address: String,

    /// 操作是否成功
    pub success: bool,

    /// 操作延迟（微秒）
    pub latency_us: u64,
}

/// 设备事件
///
/// 记录设备状态变化事件，如连接、断开、重连等。
/// 这些事件用于监控和日志记录，帮助诊断连接问题。
///
/// # 字段说明
///
/// - `device_id`: 设备标识符
/// - `event_type`: 事件类型（连接、断开、重连等）
/// - `timestamp_ms`: 事件发生的 Unix 时间戳（毫秒）
/// - `details`: 事件详情描述
#[derive(Clone, Debug)]
pub struct DeviceEvent {
    /// 设备标识符
    pub device_id: String,

    /// 事件类型
    pub event_type: DeviceEventType,

    /// 时间戳（毫秒）
    pub timestamp_ms: u64,

    /// 事件详情
    pub details: String,
}

/// 设备事件类型
///
/// 定义设备可能发生的各种状态变化事件。
#[derive(Clone, Debug)]
pub enum DeviceEventType {
    /// 设备成功连接
    Connected,

    /// 设备连接断开
    Disconnected,

    /// 设备正在重新连接
    Reconnecting,

    /// 发生错误
    Error,

    /// 心跳超时未收到
    HeartbeatMissed,
}

/// 延迟样本
///
/// 记录单次设备通信的延迟数据，用于延迟监控和异常检测。
/// 使用 3-sigma 算法检测延迟异常。
///
/// # 字段说明
///
/// - `device_id`: 设备 ID（u32，与 String 的 device_id 不同）
/// - `latency_us`: 通信延迟（微秒）
/// - `timestamp_ms`: 采样时间戳（毫秒）
#[derive(Clone, Debug)]
pub struct LatencySample {
    /// 设备 ID
    pub device_id: String,

    /// 延迟时间（微秒）
    pub latency_us: u64,

    /// 采样时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 所有 workers 的共享状态
///
/// 这是 RoboPLC Hub 中的全局变量结构，所有 workers 都可以访问这些状态。
/// 使用锁来保证线程安全。
///
/// # 数据结构说明
///
/// - `device_states`: 使用 `Arc<DashMap>` 实现并发安全的随机访问
///   - DashMap 提供无锁读取和分片写入，避免 RwLock 写者饥饿问题
/// - 其他字段: 使用 `DataBuffer` 实现高效的循环缓冲区
///
/// # 使用场景
///
/// - `ModbusWorker`: 更新设备状态、记录延迟和事件
/// - `HttpWorker`: 读取设备状态以提供 API 响应
/// - `LatencyMonitor`: 从延迟样本中读取数据进行分析
pub struct Variables {
    /// 每个设备的状态（随机访问，并发读取）
    pub device_states: Arc<DashMap<String, DeviceStatus>>,

    /// 最近的 Modbus 事务（环形缓冲区，用于日志）
    pub modbus_transactions: DataBuffer<ModbusLogEntry>,

    /// 设备事件流（事件流）
    pub device_events: DataBuffer<DeviceEvent>,

    /// 数据缓存，用于流式数据模式
    pub data_cache: DataCache,

    /// SSE 连接注册表（用于 SSE 流式推送）
    pub sse_registry: Arc<SseConnectionRegistry>,
}

/// 为 Variables 实现 Debug trait
///
/// 手动实现而不是 derive，因为 DataBuffer 没有实现 Debug
/// 我们只打印缓冲区的长度，而不是内容
impl std::fmt::Debug for Variables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Variables")
            .field("device_states", &self.device_states)
            .field("modbus_transactions_len", &self.modbus_transactions.len())
            .field("device_events_len", &self.device_events.len())
            .field("data_cache", &self.data_cache)
            .field("sse_registry_count", &self.sse_registry.count())
            .finish()
    }
}

/// 为 Variables 实现 Default trait
impl Default for Variables {
    fn default() -> Self {
        Self {
            device_states: Arc::new(DashMap::new()),
            modbus_transactions: DataBuffer::bounded(500),
            device_events: DataBuffer::bounded(500),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_data_cache_entry_new() {
        let values = serde_json::json!({"temperature": 25.5});
        let entry = DataCacheEntry::new(values.clone(), 1234567890, 100, false);

        assert_eq!(entry.values, values);
        assert_eq!(entry.timestamp_ms, 1234567890);
        assert_eq!(entry.latency_us, 100);
        assert!(!entry.error);
    }

    #[test]
    fn test_data_cache_basic_operations() {
        let cache = DataCache::new();

        // Test get on empty cache
        assert!(cache.get("nonexistent").is_none());

        // Test set and get
        let values = serde_json::json!({"temperature": 25.5, "humidity": 60});
        let entry = DataCacheEntry::new(values.clone(), 1234567890, 100, false);
        cache.set("device1_temp", entry.clone());

        let retrieved = cache.get("device1_temp").unwrap();
        assert_eq!(retrieved.values, values);
        assert_eq!(retrieved.timestamp_ms, 1234567890);
        assert_eq!(retrieved.latency_us, 100);
        assert!(!retrieved.error);
    }

    #[test]
    fn test_data_cache_update_existing() {
        let cache = DataCache::new();

        // Set initial value
        let values1 = serde_json::json!({"value": 10});
        let entry1 = DataCacheEntry::new(values1, 1000, 50, false);
        cache.set("key1", entry1);

        // Update with new value
        let values2 = serde_json::json!({"value": 20});
        let entry2 = DataCacheEntry::new(values2.clone(), 2000, 60, false);
        cache.set("key1", entry2);

        // Verify update
        let retrieved = cache.get("key1").unwrap();
        assert_eq!(retrieved.values, values2);
        assert_eq!(retrieved.timestamp_ms, 2000);
    }

    #[test]
    fn test_data_cache_is_fresh() {
        let cache = DataCache::new();

        // Get current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Set entry with current timestamp
        let values = serde_json::json!({"value": 42});
        let entry = DataCacheEntry::new(values, now, 100, false);
        cache.set("fresh_key", entry);

        // Should be fresh with 1 second TTL
        assert!(cache.is_fresh("fresh_key", 1000));

        // Set entry with old timestamp (10 seconds ago)
        let old_values = serde_json::json!({"value": 0});
        let old_entry = DataCacheEntry::new(old_values, now - 10000, 100, false);
        cache.set("stale_key", old_entry);

        // Should NOT be fresh with 1 second TTL
        assert!(!cache.is_fresh("stale_key", 1000));

        // Non-existent key should not be fresh
        assert!(!cache.is_fresh("nonexistent", 1000));
    }

    #[test]
    fn test_data_cache_get_age() {
        let cache = DataCache::new();

        // Get current timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Set entry with 5 second old timestamp
        let values = serde_json::json!({"value": 42});
        let entry = DataCacheEntry::new(values, now - 5000, 100, false);
        cache.set("age_key", entry);

        // Get age - should be approximately 5000ms
        let age = cache.get_age("age_key").unwrap();
        assert!(age >= 4990 && age <= 5100, "Age should be ~5000ms, got {}", age);

        // Non-existent key should return None
        assert!(cache.get_age("nonexistent").is_none());
    }

    #[test]
    fn test_data_cache_concurrent_access() {
        let cache = DataCache::new();
        let num_threads = 10;
        let num_operations = 100;

        // Spawn threads that write to the cache
        let mut handles = vec![];
        for i in 0..num_threads {
            let cache_clone = cache.clone();
            let handle = thread::spawn(move || {
                for j in 0..num_operations {
                    let key = format!("thread_{}_op_{}", i, j);
                    let values = serde_json::json!({"thread": i, "op": j});
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    let entry = DataCacheEntry::new(values, now, 10, false);
                    cache_clone.set(&key, entry);
                }
            });
            handles.push(handle);
        }

        // Spawn threads that read from the cache concurrently
        for _ in 0..num_threads {
            let cache_clone = cache.clone();
            let handle = thread::spawn(move || {
                for _ in 0..num_operations * 2 {
                    // Try to read any key
                    for i in 0..num_threads {
                        for j in 0..num_operations {
                            let key = format!("thread_{}_op_{}", i, j);
                            let _ = cache_clone.get(&key);
                        }
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all writes succeeded
        for i in 0..num_threads {
            for j in 0..num_operations {
                let key = format!("thread_{}_op_{}", i, j);
                let entry = cache.get(&key).expect(&format!("Key {} should exist", key));
                assert_eq!(entry.values["thread"], i);
                assert_eq!(entry.values["op"], j);
            }
        }
    }

    #[test]
    fn test_data_cache_concurrent_same_key() {
        let cache = DataCache::new();
        let num_threads = 20;
        let key = "concurrent_key";

        // Spawn threads that all write to the same key
        let mut handles = vec![];
        for i in 0..num_threads {
            let cache_clone = cache.clone();
            let handle = thread::spawn(move || {
                let values = serde_json::json!({"writer": i});
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let entry = DataCacheEntry::new(values, now, 10, false);
                cache_clone.set(key, entry);
            });
            handles.push(handle);
        }

        // Spawn threads that read from the same key concurrently
        for _ in 0..num_threads {
            let cache_clone = cache.clone();
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let _ = cache_clone.get(key);
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Key should exist (value is from one of the writers)
        assert!(cache.get(key).is_some());
    }

    #[test]
    fn test_data_cache_error_entry() {
        let cache = DataCache::new();

        let values = serde_json::json!({"error": "timeout"});
        let entry = DataCacheEntry::new(values, 1234567890, 5000, true);
        cache.set("error_key", entry);

        let retrieved = cache.get("error_key").unwrap();
        assert!(retrieved.error);
        assert_eq!(retrieved.latency_us, 5000);
    }

    #[test]
    fn test_variables_default_with_data_cache() {
        let vars = Variables::default();

        // Verify data_cache is initialized
        let entry = DataCacheEntry::new(
            serde_json::json!({"test": 1}),
            1234567890,
            100,
            false,
        );
        vars.data_cache.set("test", entry);

        assert!(vars.data_cache.get("test").is_some());
    }
}
