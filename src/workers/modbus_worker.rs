use crate::config::Device;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::io::modbus::prelude::*;
use roboplc::prelude::*;
use roboplc::{comm::tcp, time::interval};
use std::time::{SystemTime, UNIX_EPOCH};

const MODBUS_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(WorkerOpts)]
#[worker_opts(name = "modbus_worker", cpu = 1, scheduling = "fifo", priority = 80)]
pub struct ModbusWorker {
    device: Device,
}

impl ModbusWorker {
    pub fn new(device: Device) -> Self {
        Self { device }
    }
}

impl Worker<Message, Variables> for ModbusWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let endpoint = format!("{}:{}", self.device.address, self.device.port);
        let _modbus_tcp_client = tcp::connect(&endpoint, MODBUS_TIMEOUT)?;

        let _mapping_placeholder: Option<ModbusMapping> = None;

        for _ in interval(Duration::from_millis(100)).take_while(|_| context.is_online()) {
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            context.hub().send(Message::DeviceHeartbeat {
                device_id: self.device.id.clone(),
                timestamp_ms,
                latency_us: 0,
            });
        }
        Ok(())
    }
}
