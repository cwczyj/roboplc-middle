use actix_web::{web, App, HttpResponse, HttpServer, Result};
use roboplc::controller::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{config::Config, Message, Variables};

/// Application state shared across HTTP handlers
pub struct AppState {
    pub device_states: Arc<parking_lot_rt::RwLock<HashMap<String, crate::DeviceStatus>>>,
}

/// GET /api/devices - Returns list of all devices with status
async fn get_devices(data: web::Data<AppState>) -> Result<HttpResponse> {
    let states = data.device_states.read();
    let devices: Vec<serde_json::Value> = states
        .iter()
        .map(|(id, status)| {
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

/// GET /api/devices/{id} - Returns status of specific device
async fn get_device_by_id(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    let device_id = path.into_inner();
    let states = data.device_states.read();

    if let Some(status) = states.get(&device_id) {
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

/// GET /api/health - Returns health status
async fn get_health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"status": "healthy"})))
}

/// GET /api/config - Returns current configuration
async fn get_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"config": {}})))
}

/// POST /api/config/reload - Triggers configuration reload
async fn reload_config() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({"reload": "ok"})))
}

/// Configure routes for the HTTP API
fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .route("/devices", web::get().to(get_devices))
            .route("/devices/{id}", web::get().to(get_device_by_id))
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

        let app_state = web::Data::new(AppState { device_states });

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("HttpWorker: failed to create Tokio runtime");

            rt.block_on(async move {
                let server = HttpServer::new(move || {
                    App::new()
                        .app_data(app_state.clone())
                        .configure(configure_routes)
                });

                match server.bind(&addr) {
                    Ok(server) => {
                        println!("HttpWorker: listening on http://{}", addr);
                        server.run().await.expect("HttpWorker: failed to run server");
                    }
                    Err(e) => {
                        eprintln!("HttpWorker: failed to bind {}: {}", addr, e);
                    }
                }
            });
        });

        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceStatus;
    use std::time::Instant;

    fn make_app_state() -> AppState {
        AppState {
            device_states: Arc::new(parking_lot_rt::RwLock::new(HashMap::new())),
        }
    }

    fn make_app_state_with_device(id: &str, connected: bool) -> AppState {
        let mut states = HashMap::new();
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
            device_states: Arc::new(parking_lot_rt::RwLock::new(states)),
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
        let result = get_device_by_id(web::Path::from("device-1".to_string()), web::Data::new(app_state)).await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_device_by_id_not_found() {
        let app_state = make_app_state();
        let result = get_device_by_id(web::Path::from("nonexistent".to_string()), web::Data::new(app_state)).await;
        assert_eq!(result.unwrap().status(), actix_web::http::StatusCode::NOT_FOUND);
    }

    #[actix_rt::test]
    async fn test_get_health() {
        let result = get_health().await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_get_config() {
        let result = get_config().await;
        assert!(result.is_ok());
    }

    #[actix_rt::test]
    async fn test_reload_config() {
        let result = reload_config().await;
        assert!(result.is_ok());
    }
}