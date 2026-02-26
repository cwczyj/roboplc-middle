use roboplc::controller::prelude::*;
use roboplc_rpc::{dataformat::Json, server::RpcServer, server::RpcServerHandler, RpcResult};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::SocketAddr;

use crate::config::Config;
use crate::messages::Message;
use crate::Variables;

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "m",
    content = "p",
    rename_all = "lowercase",
    deny_unknown_fields
)]
enum RpcMethod<'a> {
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

struct RpcHandler;

impl RpcHandler {
    pub fn new(_rpc_port: u16) -> Self {
        Self
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
            RpcMethod::GetStatus { device_id: _ } => Ok(RpcResultType::Status {
                connected: false,
                last_communication_ms: 0,
                error_count: 0,
            }),
            RpcMethod::SetRegister {
                device_id: _,
                address: _,
                value: _,
            } => Ok(RpcResultType::Success { success: true }),
            RpcMethod::GetRegister {
                device_id: _,
                address: _,
            } => Ok(RpcResultType::Data {
                data: serde_json::json!({ "value": 0 }),
            }),
            RpcMethod::MoveTo {
                device_id: _,
                position: _,
            } => Ok(RpcResultType::Success { success: true }),
            RpcMethod::ReadBatch {
                device_id: _,
                addresses: _,
            } => Ok(RpcResultType::Data {
                data: serde_json::json!({}),
            }),
            RpcMethod::WriteBatch {
                device_id: _,
                values: _,
            } => Ok(RpcResultType::Success { success: true }),
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
        let handler = RpcHandler::new(port);
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
