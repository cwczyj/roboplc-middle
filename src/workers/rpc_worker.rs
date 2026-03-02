use crate::config::Config;
use crate::messages::Message;
use crate::messages::Operation;
use crate::Variables;
use roboplc::controller::prelude::*;
use roboplc_rpc::{dataformat::Json, server::RpcServer, server::RpcServerHandler, RpcResult};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_correlation_id() -> u64 {
    CORRELATION_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "m",
    content = "p",
    rename_all = "lowercase",
    deny_unknown_fields
)]
enum RpcMethod<'a> {
    Ping {},
    GetVersion {},
    GetDeviceList {},
    GetStatus {
        device_id: &'a str,
    },
    SetRegister {
        device_id: &'a str,
        address: String,
        value: u16,
    },
    GetRegister {
        device_id: &'a str,
        address: String,
    },
    MoveTo {
        device_id: &'a str,
        position: String,
    },
    ReadBatch {
        device_id: &'a str,
        addresses: Vec<String>,
    },
    WriteBatch {
        device_id: &'a str,
        values: Vec<(String, u16)>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RpcResultType {
    Success {
        success: bool,
    },
    Version {
        version: String,
    },
    DeviceList {
        devices: Vec<String>,
    },
    Data {
        data: serde_json::Value,
    },
    Status {
        connected: bool,
        last_communication_ms: u64,
        error_count: u32,
    },
    Error {
        error: String,
    },
}

pub type ResponseSender = Sender<(bool, JsonValue, Option<String>)>;

#[derive(Clone)]
pub struct DeviceControlRequest {
    pub device_id: String,
    pub operation: Operation,
    pub params: JsonValue,
    pub correlation_id: u64,
    pub respond_to: ResponseSender,
}

struct RpcHandler {
    device_ids: Vec<String>,
    device_control_tx: Sender<DeviceControlRequest>,
}

impl RpcHandler {
    pub fn new(device_ids: Vec<String>, device_control_tx: Sender<DeviceControlRequest>) -> Self {
        Self {
            device_ids,
            device_control_tx,
        }
    }
}

impl<'a> RpcServerHandler<'a> for RpcHandler {
    type Method = RpcMethod<'a>;
    type Result = RpcResultType;
    type Source = SocketAddr;

    fn handle_call(
        &'a self,
        method: Self::Method,
        _source: Self::Source,
    ) -> RpcResult<Self::Result> {
        match method {
            RpcMethod::Ping {} => Ok(RpcResultType::Success { success: true }),
            RpcMethod::GetVersion {} => Ok(RpcResultType::Version {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            RpcMethod::GetDeviceList {} => Ok(RpcResultType::DeviceList {
                devices: self.device_ids.clone(),
            }),
            RpcMethod::GetStatus { device_id } => {
                self.send_device_control(device_id, Operation::GetStatus, serde_json::json!({}))
            }
            RpcMethod::SetRegister {
                device_id,
                address,
                value,
            } => {
                let params = serde_json::json!({ "address": address, "value": value });
                self.send_device_control(device_id, Operation::SetRegister, params)
            }
            RpcMethod::GetRegister { device_id, address } => {
                let params = serde_json::json!({ "address": address });
                self.send_device_control(device_id, Operation::GetRegister, params)
            }
            RpcMethod::MoveTo {
                device_id,
                position,
            } => {
                let params = serde_json::json!({ "position": position });
                self.send_device_control(device_id, Operation::MoveTo, params)
            }
            RpcMethod::ReadBatch {
                device_id,
                addresses,
            } => {
                let params = serde_json::json!({ "addresses": addresses });
                self.send_device_control(device_id, Operation::ReadBatch, params)
            }
            RpcMethod::WriteBatch { device_id, values } => {
                let params = serde_json::json!({ "values": values });
                self.send_device_control(device_id, Operation::WriteBatch, params)
            }
        }
    }
}

impl RpcHandler {
    fn send_device_control(
        &self,
        device_id: &str,
        operation: Operation,
        params: JsonValue,
    ) -> RpcResult<RpcResultType> {
        let correlation_id = next_correlation_id();
        let (response_tx, response_rx) = channel();

        let request = DeviceControlRequest {
            device_id: device_id.to_string(),
            operation,
            params,
            correlation_id,
            respond_to: response_tx,
        };

        if let Err(error) = self.device_control_tx.send(request) {
            tracing::error!(%error, "failed to send DeviceControl request");
            return Ok(RpcResultType::Error {
                error: format!("Internal error: {}", error),
            });
        }

        match response_rx.recv() {
            Ok((success, data, error)) => {
                if success {
                    Ok(RpcResultType::Data { data })
                } else {
                    Ok(RpcResultType::Error {
                        error: error.unwrap_or_else(|| "Unknown error".to_string()),
                    })
                }
            }
            Err(error) => {
                tracing::error!(%error, "failed to receive DeviceResponse");
                Ok(RpcResultType::Error {
                    error: format!("Response error: {}", error),
                })
            }
        }
    }
}

#[derive(WorkerOpts)]
#[worker_opts(name = "rpc_server", blocking = true)]
pub struct RpcWorker {
    config: Config,
}

impl RpcWorker {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Worker<Message, Variables> for RpcWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let port = self.config.server.rpc_port;
        let bind_addr = format!("0.0.0.0:{}", port);

        let device_ids: Vec<String> = self.config.devices.iter().map(|d| d.id.clone()).collect();

        let (device_control_tx, device_control_rx) = channel::<DeviceControlRequest>();

        let handler = RpcHandler::new(device_ids, device_control_tx);
        let server = RpcServer::new(handler);

        let listener = match std::net::TcpListener::bind(&bind_addr) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%error, "RPC Server Worker failed to bind {}", bind_addr);
                return Ok(());
            }
        };
        if let Err(error) = listener.set_nonblocking(true) {
            tracing::error!(
                %error,
                "RPC Server Worker failed to set non-blocking mode on {}",
                bind_addr
            );
            return Ok(());
        }

        tracing::info!("RPC Server Worker started on {}", bind_addr);

        let mut pending_requests: HashMap<u64, ResponseSender> = HashMap::new();

        while context.is_online() {
            match listener.accept() {
                Ok((mut stream, source)) => {
                    if let Err(error) =
                        stream.set_read_timeout(Some(std::time::Duration::from_millis(200)))
                    {
                        tracing::warn!(%source, %error, "failed to set read timeout");
                    }

                    let mut request_payload = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                request_payload.extend_from_slice(&buf[..n]);
                            }
                            Err(e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut =>
                            {
                                break;
                            }
                            Err(error) => {
                                tracing::warn!(%source, %error, "failed reading RPC request payload");
                                break;
                            }
                        }
                    }

                    if request_payload.is_empty() {
                        continue;
                    }

                    if let Some(response_payload) =
                        server.handle_request_payload::<Json>(&request_payload, source)
                    {
                        if let Err(error) = stream.write_all(&response_payload) {
                            tracing::warn!(%source, %error, "failed writing RPC response payload");
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    while let Ok(request) = device_control_rx.try_recv() {
                        pending_requests.insert(request.correlation_id, request.respond_to);

                        let message = Message::DeviceControl {
                            device_id: request.device_id,
                            operation: request.operation,
                            params: request.params,
                            correlation_id: request.correlation_id,
                        };
                        context.hub().send(message);
                    }

                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => {
                    tracing::warn!(%error, "RPC Server Worker accept error");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

        tracing::info!("RPC Server Worker stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_id_increments() {
        let id1 = next_correlation_id();
        let id2 = next_correlation_id();
        assert!(id2 > id1, "correlation IDs should increment");
    }

    #[test]
    fn device_control_request_can_be_sent() {
        let (tx, rx) = channel::<DeviceControlRequest>();
        let (response_tx, _response_rx) = channel();

        let request = DeviceControlRequest {
            device_id: "test-device".to_string(),
            operation: Operation::GetRegister,
            params: serde_json::json!({ "address": "h100" }),
            correlation_id: 1,
            respond_to: response_tx,
        };

        tx.send(request.clone()).unwrap();
        let received = rx.try_recv().unwrap();
        assert_eq!(received.device_id, "test-device");
        assert_eq!(received.correlation_id, 1);
    }
}
