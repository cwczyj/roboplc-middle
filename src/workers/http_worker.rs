//! # HTTP Worker
//!
//! HTTP REST API服务器worker，提供设备管理接口。
//!
//! ## 功能
//!
//! - 监听TCP端口（默认8081）提供REST API
//! - 查询设备状态
//! - 获取系统健康状态
//! - 触发配置重载
//! - 支持优雅关闭（3秒超时）

use actix_sse::Sse;
use actix_web::{web, App, Either, HttpResponse, HttpServer, Result};
use dashmap::DashMap;
use futures_util::StreamExt;
use roboplc::controller::prelude::*;
use serde_json::json;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    config::Config, create_sse_channel, DataCache, DeviceStatus, Message, SseConnection,
    SseConnectionRegistry, SseEventData, Variables,
};

#[derive(Clone)]
pub struct AppState {
    pub device_states: Arc<DashMap<String, DeviceStatus>>,
    pub config: Arc<Config>,
    pub data_cache: DataCache,
    pub sse_registry: Arc<SseConnectionRegistry>,
}

async fn get_devices(data: web::Data<AppState>) -> Result<HttpResponse> {
    let devices: Vec<serde_json::Value> = data
        .device_states
        .iter()
        .map(|entry| {
            let id = entry.key();
            let status = entry.value();
            json!({
                "id": id,
                "connected": status.connected,
                "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
                "error_count": status.error_count,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(json!({"devices": devices})))
}

async fn get_device_by_id(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();

    if let Some(status_ref) = data.device_states.get(&device_id) {
        let status = status_ref.value();
        let body = json!({
            "id": device_id,
            "connected": status.connected,
            "last_communication_ms": status.last_communication.elapsed().as_millis() as u64,
            "error_count": status.error_count,
            "reconnect_count": status.reconnect_count,
        });
        Ok(HttpResponse::Ok().json(body))
    } else {
        Ok(HttpResponse::NotFound().json(json!({"error": "Device not found"})))
    }
}

/// 这个函数处理 GET /api/health 请求
/// 用于健康检查，监控系统是否正常运行
/// 返回系统健康状态，包括设备连接统计
async fn get_health(data: web::Data<AppState>) -> Result<HttpResponse> {
    let total = data.device_states.len();
    let connected = data
        .device_states
        .iter()
        .filter(|entry| entry.value().connected)
        .count();
    let disconnected = total - connected;

    let status = if total == 0 {
        "unhealthy"
    } else if connected == total {
        "healthy"
    } else if connected == 0 {
        "unhealthy"
    } else {
        "degraded"
    };

    Ok(HttpResponse::Ok().json(json!({
        "status": status,
        "devices": {
            "total": total,
            "connected": connected,
            "disconnected": disconnected
        }
    })))
}

/// 这个函数处理 GET /api/config 请求
/// 用于获取当前配置信息
async fn get_config(data: web::Data<AppState>) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "config": &*data.config
    })))
}

/// 配置重载端点
///
/// 注意：此端点仅返回成功响应，不会实际触发配置重载。
///
/// 实际的配置重载由 ConfigLoader 的文件监控机制触发：
/// - ConfigLoader 持续监控 config.toml 文件的变化
/// - 当文件被修改时，ConfigLoader 自动重新加载配置
/// - 如需触发重载，请直接修改 config.toml 文件
async fn reload_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"reload": "ok"})))
}

const CACHE_TTL_MS: u64 = 50;

async fn get_cached_data(
    path: web::Path<(String, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    let (device_id, signal_group) = path.into_inner();
    let cache_key = format!("{}_{}", device_id, signal_group);

    match data.data_cache.get(&cache_key) {
        Some(entry) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cache_age_ms = now.saturating_sub(entry.timestamp_ms);
            let fresh = cache_age_ms < CACHE_TTL_MS;

            let body = json!({
                "device_id": device_id,
                "signal_group": signal_group,
                "values": entry.values,
                "timestamp_ms": entry.timestamp_ms,
                "cache_age_ms": cache_age_ms,
                "fresh": fresh,
            });
            Ok(HttpResponse::Ok().json(body))
        }
        None => {
            let body = json!({
                "error": format!(
                    "Cache miss: no data found for device '{}' and signal group '{}'",
                    device_id, signal_group
                ),
                "device_id": device_id,
                "signal_group": signal_group,
            });
            Ok(HttpResponse::NotFound().json(body))
        }
    }
}

/// SSE query parameters
#[derive(Debug, serde::Deserialize)]
struct SseQuery {
    device: String,
    groups: String,
}

/// SSE endpoint: GET /api/stream?device={device_id}&groups={group1,group2}
///
/// Establishes a Server-Sent Events connection that streams DataStreamUpdate messages
/// filtered by device_id and signal_groups.
async fn sse_stream(
    query: web::Query<SseQuery>,
    data: web::Data<AppState>,
) -> Either<HttpResponse, Sse<impl futures_util::Stream<Item = Result<actix_sse::Event, actix_web::Error>>>> {
    let device_id = query.device.clone();
    let signal_groups: Vec<String> = query
        .groups
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if signal_groups.is_empty() {
        return Either::Left(HttpResponse::BadRequest().json(json!({
            "error": "No signal groups specified"
        })));
    }

    let connected_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let (tx, rx) = create_sse_channel();

    let conn = SseConnection::new(device_id.clone(), signal_groups.clone(), connected_at, tx);
    let conn_id = match data.sse_registry.register(conn) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(
                device_id = %device_id,
                error = %e,
                "SSE connection rejected"
            );
            return Either::Left(HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": e
            })));
        }
    };

    tracing::info!(
        device_id = %device_id,
        signal_groups = ?signal_groups,
        connection_id = conn_id.value(),
        "SSE client connected"
    );

    let stream = ReceiverStream::new(rx);

    let event_stream = stream.map(move |event| {
        match event {
            SseEventData::JsonData(data) => {
                let json_str = serde_json::to_string(&data).unwrap_or_default();
                Ok(actix_sse::Event::Data(actix_sse::Data::new(json_str)))
            }
            SseEventData::Heartbeat => {
                Ok(actix_sse::Event::Comment("heartbeat".into()))
            }
        }
    });

    let sse = Sse::from_stream(event_stream)
        .with_keep_alive(std::time::Duration::from_secs(15))
        .with_retry_duration(std::time::Duration::from_secs(5));

    tracing::info!(
        device_id = %device_id,
        connection_id = conn_id.value(),
        "SSE stream started"
    );

    let _ = conn_id;

    Either::Right(sse)
}

fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .route("/devices", web::get().to(get_devices))
            .route("/devices/{id}/status", web::get().to(get_device_by_id))
            .route("/health", web::get().to(get_health))
            .route("/config", web::get().to(get_config))
            .route("/config/reload", web::post().to(reload_config))
            .route("/cache/{device}/{group}", web::get().to(get_cached_data))
            .route("/stream", web::get().to(sse_stream)),
    );
}

#[derive(WorkerOpts)]
#[worker_opts(name = "http_server", blocking = true)]
pub struct HttpWorker {
    config: Config,
}

impl HttpWorker {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Worker<Message, Variables> for HttpWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let http_port = self.config.server.http_port;
        let addr = format!("0.0.0.0:{}", http_port);

        let device_states = context.variables().device_states.clone();
        let config = Arc::new(self.config.clone());

        let app_state = web::Data::new(AppState {
            device_states,
            config,
            data_cache: context.variables().data_cache.clone(),
            sse_registry: context.variables().sse_registry.clone(),
        });

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let addr_clone = addr.clone();
        let runtime_handle: JoinHandle<()> = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("HttpWorker: failed to create Tokio runtime");

            rt.block_on(async move {
                let server = HttpServer::new(move || {
                    App::new()
                        .app_data(app_state.clone())
                        .configure(configure_routes)
                })
                .bind(&addr_clone)
                .expect("HttpWorker: failed to bind address");

                tracing::info!("HttpWorker: listening on http://{}", addr_clone);

                let running_server = server.run();
                let server_handle = running_server.handle();

                tokio::spawn(async move {
                    shutdown_rx.await.ok();
                    server_handle.stop(true).await;
                });

                running_server.await.expect("HttpWorker: server run failed");
            });

            rt.shutdown_timeout(std::time::Duration::from_secs(3));
        });

        tracing::info!("HttpWorker started on http://{}", addr);

        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        tracing::info!("HttpWorker shutting down...");

        let _ = shutdown_tx.send(());

        match runtime_handle.join() {
            Ok(()) => tracing::info!("HttpWorker stopped gracefully"),
            Err(_) => tracing::warn!("HttpWorker thread panicked"),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Logging, Server};
    use crate::{DataCacheEntry, DeviceStatus, Message};
    use std::time::Instant;

    fn make_app_state() -> AppState {
        AppState {
            device_states: Arc::new(DashMap::new()),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                    ..Default::default()
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                timeouts: Default::default(),
                devices: vec![],
                streams: vec![],
                stream_settings: Default::default(),
            }),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        }
    }

    fn make_app_state_with_device(id: &str, connected: bool) -> AppState {
        let states = DashMap::new();
        states.insert(
            id.to_string(),
            DeviceStatus {
                connected,
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        AppState {
            device_states: Arc::new(states),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                    ..Default::default()
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                timeouts: Default::default(),
                devices: vec![],
                streams: vec![],
                stream_settings: Default::default(),
            }),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        }
    }

    #[actix_rt::test]
    async fn test_get_devices_empty() {
        let app_state = make_app_state();
        let result = get_devices(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_devices_with_device() {
        let app_state = make_app_state_with_device("device-1", true);
        let result = get_devices(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_device_by_id_found() {
        let app_state = make_app_state_with_device("device-1", true);
        let result = get_device_by_id(
            web::Path::from("device-1".to_string()),
            web::Data::new(app_state),
        )
        .await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_device_by_id_not_found() {
        let app_state = make_app_state();
        let result = get_device_by_id(
            web::Path::from("nonexistent".to_string()),
            web::Data::new(app_state),
        )
        .await;
        assert_eq!(
            result.unwrap().status(),
            actix_web::http::StatusCode::NOT_FOUND
        );
    }

    #[actix_rt::test]
    async fn test_get_health() {
        let app_state = make_app_state();
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_get_health_connected_devices() {
        let app_state = make_app_state_with_device("device-1", true);
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_get_health_mixed_devices() {
        let states = DashMap::new();
        states.insert(
            "device-1".to_string(),
            DeviceStatus {
                connected: true,
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        states.insert(
            "device-2".to_string(),
            DeviceStatus {
                connected: false,
                last_communication: Instant::now(),
                error_count: 0,
                reconnect_count: 0,
            },
        );
        let app_state = AppState {
            device_states: Arc::new(states),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                    ..Default::default()
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                timeouts: Default::default(),
                devices: vec![],
                streams: vec![],
                stream_settings: Default::default(),
            }),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        };
        let result = get_health(web::Data::new(app_state)).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_get_config() {
        use crate::config::{Logging, Server};

        let app_state = AppState {
            device_states: Arc::new(DashMap::new()),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                    ..Default::default()
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                timeouts: Default::default(),
                devices: vec![],
                streams: vec![],
                stream_settings: Default::default(),
            }),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        };
        let result = get_config(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_reload_config() {
        let result = reload_config().await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_cache_endpoint_hit() {
        let app_state = make_app_state();
        let cache_key = "robot-arm-1_position".to_string();
        let entry = DataCacheEntry::new(
            json!({"x": 100.5, "y": 200.0, "z": 50.0}),
            1712345678901,
            150,
            false,
        );
        app_state.data_cache.set(&cache_key, entry);

        let result = get_cached_data(
            web::Path::from(("robot-arm-1".to_string(), "position".to_string())),
            web::Data::new(app_state),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_cache_endpoint_miss() {
        let app_state = make_app_state();

        let result = get_cached_data(
            web::Path::from(("unknown-device".to_string(), "unknown-group".to_string())),
            web::Data::new(app_state),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), actix_web::http::StatusCode::NOT_FOUND);
    }

    #[actix_rt::test]
    async fn test_cache_endpoint_concurrent_requests() {
        use futures::future::join_all;

        let app_state = make_app_state();
        let cache_key = "plc-1_temperature".to_string();
        let entry = DataCacheEntry::new(
            json!({"temp": 25.5, "humidity": 60.0}),
            1712345678901,
            100,
            false,
        );
        app_state.data_cache.set(&cache_key, entry);

        let mut handles = vec![];
        for _i in 0..10 {
            let state = app_state.clone();
            handles.push(async move {
                get_cached_data(
                    web::Path::from(("plc-1".to_string(), "temperature".to_string())),
                    web::Data::new(state),
                )
                .await
            });
        }

        let results = join_all(handles).await;
        for result in results {
            assert!(result.is_ok());
            let response = result.unwrap();
            assert_eq!(response.status(), actix_web::http::StatusCode::OK);
        }
    }

    // ====================================================================================
    // SSE Integration Tests
    // ====================================================================================

    fn make_app_state_with_sse() -> AppState {
        AppState {
            device_states: Arc::new(DashMap::new()),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                    ..Default::default()
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                timeouts: Default::default(),
                devices: vec![],
                streams: vec![],
                stream_settings: Default::default(),
            }),
            data_cache: DataCache::new(),
            sse_registry: Arc::new(SseConnectionRegistry::new()),
        }
    }

    #[actix_rt::test]
    async fn test_sse_stream_empty_groups_error() {
        let app_state = make_app_state_with_sse();

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-1".to_string(),
                groups: "".to_string(),
            }),
            web::Data::new(app_state),
        )
        .await;

        assert!(
            matches!(result, Either::Left(_)),
            "Should return error response for empty groups"
        );

        if let Either::Left(response) = result {
            assert_eq!(response.status(), actix_web::http::StatusCode::BAD_REQUEST);
        }
    }

    #[actix_rt::test]
    async fn test_sse_stream_valid_query_params() {
        let app_state = make_app_state_with_sse();
        let initial_count = app_state.sse_registry.count();

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-1".to_string(),
                groups: "temperature,pressure".to_string(),
            }),
            web::Data::new(app_state.clone()),
        )
        .await;

        assert!(
            matches!(result, Either::Right(_)),
            "Should return SSE stream for valid params"
        );

        assert_eq!(app_state.sse_registry.count(), initial_count + 1);
    }

    #[actix_rt::test]
    async fn test_sse_stream_single_group() {
        let app_state = make_app_state_with_sse();

        let result = sse_stream(
            web::Query(SseQuery {
                device: "robot-arm-1".to_string(),
                groups: "position".to_string(),
            }),
            web::Data::new(app_state),
        )
        .await;

        assert!(
            matches!(result, Either::Right(_)),
            "Should accept single group"
        );
    }

    #[actix_rt::test]
    async fn test_sse_stream_groups_with_whitespace() {
        let app_state = make_app_state_with_sse();
        let initial_count = app_state.sse_registry.count();

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-1".to_string(),
                groups: "  temperature , pressure , humidity ".to_string(),
            }),
            web::Data::new(app_state.clone()),
        )
        .await;

        assert!(matches!(result, Either::Right(_)));
        assert_eq!(app_state.sse_registry.count(), initial_count + 1);
    }

    #[actix_rt::test]
    async fn test_sse_stream_special_device_id() {
        let app_state = make_app_state_with_sse();

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-device_1.2-3".to_string(),
                groups: "temp".to_string(),
            }),
            web::Data::new(app_state),
        )
        .await;

        assert!(matches!(result, Either::Right(_)));
    }

    #[actix_rt::test]
    async fn test_sse_multiple_connections_independent() {
        let app_state = make_app_state_with_sse();
        let initial_count = app_state.sse_registry.count();

        for i in 0..5 {
            let result = sse_stream(
                web::Query(SseQuery {
                    device: format!("plc-{}", i),
                    groups: format!("group{}", i),
                }),
                web::Data::new(app_state.clone()),
            )
            .await;
            assert!(matches!(result, Either::Right(_)));
        }

        assert_eq!(app_state.sse_registry.count(), initial_count + 5);

        let ids = app_state.sse_registry.all_connection_ids();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique_ids.len(), ids.len());
    }

    #[actix_rt::test]
    async fn test_sse_connection_stores_filter_info() {
        let app_state = make_app_state_with_sse();

        let device_id = "test-device-123";
        let groups = vec!["temperature", "pressure", "humidity"];
        let groups_str = groups.join(",");

        let result = sse_stream(
            web::Query(SseQuery {
                device: device_id.to_string(),
                groups: groups_str,
            }),
            web::Data::new(app_state.clone()),
        )
        .await;

        assert!(matches!(result, Either::Right(_)));

        let ids = app_state.sse_registry.all_connection_ids();
        assert!(!ids.is_empty());
    }

    #[actix_rt::test]
    async fn test_sse_registry_connection_counting() {
        let app_state = make_app_state_with_sse();

        assert_eq!(app_state.sse_registry.count(), 0);

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-1".to_string(),
                groups: "temp".to_string(),
            }),
            web::Data::new(app_state.clone()),
        )
        .await;
        assert!(matches!(result, Either::Right(_)));
        assert_eq!(app_state.sse_registry.count(), 1);

        for i in 2..=10 {
            let _ = sse_stream(
                web::Query(SseQuery {
                    device: format!("plc-{}", i),
                    groups: "temp".to_string(),
                }),
                web::Data::new(app_state.clone()),
            )
            .await;
        }
        assert_eq!(app_state.sse_registry.count(), 10);
    }

    #[actix_rt::test]
    async fn test_sse_worker_routes_to_subscribers() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx, mut rx) = channel(100);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567890,
            tx,
        );
        let conn_id = registry.register(conn).unwrap();

        let test_data = SseEventData::JsonData(json!({"temp": 25.5}));
        let sent = registry.send_to_subscribers("plc-1", "temperature", test_data.clone());
        assert_eq!(sent, 1);

        let received = rx.try_recv().unwrap();
        assert!(matches!(received, SseEventData::JsonData(_)));

        registry.unregister(conn_id);
    }

    #[actix_rt::test]
    async fn test_sse_filtering_excludes_wrong_device() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx, mut rx) = channel(100);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567890,
            tx,
        );
        registry.register(conn).unwrap();

        let test_data = SseEventData::JsonData(json!({"temp": 30.0}));
        let sent = registry.send_to_subscribers("plc-2", "temperature", test_data);
        assert_eq!(sent, 0);

        assert!(rx.try_recv().is_err());
    }

    #[actix_rt::test]
    async fn test_sse_filtering_excludes_wrong_group() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx, mut rx) = channel(100);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567890,
            tx,
        );
        registry.register(conn).unwrap();

        let test_data = SseEventData::JsonData(json!({"pressure": 101.3}));
        let sent = registry.send_to_subscribers("plc-1", "pressure", test_data);
        assert_eq!(sent, 0);

        assert!(rx.try_recv().is_err());
    }

    #[actix_rt::test]
    async fn test_sse_multiple_clients_receive_same_data() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx1, mut rx1) = channel(100);
        let conn1 = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567890,
            tx1,
        );
        registry.register(conn1);

        let (tx2, mut rx2) = channel(100);
        let conn2 = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567891,
            tx2,
        );
        registry.register(conn2);

        let (tx3, mut rx3) = channel(100);
        let conn3 = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567892,
            tx3,
        );
        registry.register(conn3);

        let test_data = SseEventData::JsonData(json!({"temp": 26.5}));
        let sent = registry.send_to_subscribers("plc-1", "temperature", test_data);
        assert_eq!(sent, 3);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
        assert!(rx3.try_recv().is_ok());
    }

    #[actix_rt::test]
    async fn test_sse_heartbeat_to_all_connections() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let mut receivers = vec![];
        for i in 0..5 {
            let (tx, rx) = channel(100);
            receivers.push(rx);
            let conn = SseConnection::new(
                format!("plc-{}", i),
                vec!["temperature".to_string()],
                1234567890 + i as u64,
                tx,
            );
            registry.register(conn).unwrap();
        }

        let sent = registry.send_heartbeat_to_all();
        assert_eq!(sent, 5);

        for mut rx in receivers {
            let received = rx.try_recv().unwrap();
            assert!(matches!(received, SseEventData::Heartbeat));
        }
    }

    #[actix_rt::test]
    async fn test_sse_connection_cleanup() {
        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let conn = crate::SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string()],
            1234567890,
            tx,
        );
        let conn_id = registry.register(conn).unwrap();
        assert_eq!(registry.count(), 1);

        registry.unregister(conn_id);
        assert_eq!(registry.count(), 0);
    }

    #[actix_rt::test]
    async fn test_sse_data_stream_message_structure() {
        let msg = Message::DataStreamUpdate {
            device_id: "plc-1".to_string(),
            signal_group: "temperature_sensor".to_string(),
            values: json!({"temp": 25.5, "humidity": 60}),
            timestamp_ms: 1712345678901,
            latency_us: 150,
            sequence: 42,
        };

        if let Message::DataStreamUpdate {
            device_id,
            signal_group,
            values,
            timestamp_ms,
            latency_us,
            sequence,
        } = &msg
        {
            assert_eq!(device_id, "plc-1");
            assert_eq!(signal_group, "temperature_sensor");
            assert_eq!(values["temp"], 25.5);
            assert_eq!(timestamp_ms, &1712345678901);
            assert_eq!(latency_us, &150);
            assert_eq!(sequence, &42);
        } else {
            panic!("Expected DataStreamUpdate message");
        }
    }

    #[actix_rt::test]
    async fn test_sse_concurrent_endpoint_access() {
        use futures::future::join_all;

        let app_state = make_app_state_with_sse();
        let initial_count = app_state.sse_registry.count();

        let mut handles = vec![];
        for i in 0..10 {
            let state = app_state.clone();
            handles.push(async move {
                sse_stream(
                    web::Query(SseQuery {
                        device: format!("plc-{}", i),
                        groups: format!("group{}", i % 3),
                    }),
                    web::Data::new(state),
                )
                .await
            });
        }

        let results = join_all(handles).await;

        for result in results {
            assert!(
                matches!(result, Either::Right(_)),
                "All SSE connections should be accepted"
            );
        }

        assert_eq!(app_state.sse_registry.count(), initial_count + 10);
    }

    #[actix_rt::test]
    async fn test_sse_partial_group_matching() {
        use crate::SseConnection;
        use tokio::sync::mpsc::channel;

        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        let (tx, mut rx) = channel(100);
        let conn = SseConnection::new(
            "plc-1".to_string(),
            vec!["temperature".to_string(), "pressure".to_string()],
            1234567890,
            tx,
        );
        registry.register(conn).unwrap();

        let sent1 = registry.send_to_subscribers(
            "plc-1",
            "temperature",
            SseEventData::JsonData(json!({"temp": 25.5})),
        );
        assert_eq!(sent1, 1);
        assert!(rx.try_recv().is_ok());

        let sent2 = registry.send_to_subscribers(
            "plc-1",
            "pressure",
            SseEventData::JsonData(json!({"pressure": 101.3})),
        );
        assert_eq!(sent2, 1);
        assert!(rx.try_recv().is_ok());

        let sent3 = registry.send_to_subscribers(
            "plc-1",
            "humidity",
            SseEventData::JsonData(json!({"humidity": 60})),
        );
        assert_eq!(sent3, 0);
    }

    #[actix_rt::test]
    async fn test_sse_registry_clear() {
        let app_state = make_app_state_with_sse();
        let registry = &app_state.sse_registry;

        for i in 0..5 {
            let (tx, _rx) = tokio::sync::mpsc::channel(100);
            let conn = crate::SseConnection::new(
                format!("plc-{}", i),
                vec!["temperature".to_string()],
                1234567890 + i as u64,
                tx,
            );
            registry.register(conn).unwrap();
        }
        assert_eq!(registry.count(), 5);

        registry.clear();
        assert_eq!(registry.count(), 0);
    }

    #[actix_rt::test]
    async fn test_sse_integration_with_cache() {
        let app_state = make_app_state_with_sse();

        let cache_key = "plc-1_temperature";
        let entry = DataCacheEntry::new(
            json!({"temp": 25.5, "humidity": 60}),
            1712345678901,
            150,
            false,
        );
        app_state.data_cache.set(cache_key, entry);

        let cached = app_state.data_cache.get(cache_key);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().values["temp"], 25.5);

        let result = sse_stream(
            web::Query(SseQuery {
                device: "plc-1".to_string(),
                groups: "temperature".to_string(),
            }),
            web::Data::new(app_state.clone()),
        )
        .await;
        assert!(matches!(result, Either::Right(_)));
    }
}
