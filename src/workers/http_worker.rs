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

use actix_web::{web, App, HttpResponse, HttpServer, Result};
use dashmap::DashMap;
use roboplc::controller::prelude::*;
use serde_json::json;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::oneshot;

use crate::{config::Config, DeviceStatus, Message, Variables};

pub struct AppState {
    pub device_states: Arc<DashMap<String, DeviceStatus>>,
    pub config: Arc<Config>,
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

fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .route("/devices", web::get().to(get_devices))
            .route("/devices/{id}/status", web::get().to(get_device_by_id))
            .route("/health", web::get().to(get_health))
            .route("/config", web::get().to(get_config))
            .route("/config/reload", web::post().to(reload_config)),
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
    use crate::DeviceStatus;
    use std::time::Instant;

    fn make_app_state() -> AppState {
        AppState {
            device_states: Arc::new(DashMap::new()),
            config: Arc::new(Config {
                server: Server {
                    rpc_port: 8080,
                    http_port: 8081,
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
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
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
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
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
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
                },
                logging: Logging {
                    level: "info".to_string(),
                    file: String::new(),
                    daily_rotation: false,
                },
                devices: vec![],
            }),
        };
        let result = get_config(web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_reload_config() {
        let result = reload_config().await;
        assert!(result.is_ok());
    }
}
