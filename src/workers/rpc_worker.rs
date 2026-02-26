use roboplc::controller::prelude::*;
use roboplc_rpc::{dataformat::Json, server::RpcServer, server::RpcServerHandler, RpcResult};
use serde::{Deserialize, Serialize};
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

struct RpcHandler {
    rpc_port: u16,
}

impl RpcHandler {
    pub fn new(rpc_port: u16) -> Self {
        Self { rpc_port }
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
        tracing::info!(
            "RPC Server Worker started on port {}",
            self.config.server.rpc_port
        );
        while context.is_online() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        tracing::info!("RPC Server Worker stopped");
        Ok(())
    }
}
