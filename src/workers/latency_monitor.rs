use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;

#[derive(WorkerOpts)]
#[worker_opts(name = "latency_monitor")]
pub struct LatencyMonitor;

impl LatencyMonitor {
    pub fn new() -> Self {
        Self
    }
}

impl Worker<Message, Variables> for LatencyMonitor {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let client = context.hub().register(
            "latency_monitor",
            event_matches!(Message::DeviceHeartbeat { .. }),
        )?;

        for msg in client {
            if let Message::DeviceHeartbeat {
                device_id,
                timestamp_ms,
                latency_us,
            } = msg
            {
                let device_id_num = device_id.parse::<u32>().unwrap_or(0);
                let sample = LatencySample {
                    device_id: device_id_num,
                    latency_us,
                    timestamp_ms,
                };
                context.variables().latency_samples.force_push(sample);

                if latency_us > 10_000 {
                    let event = DeviceEvent {
                        device_id: device_id.clone(),
                        event_type: DeviceEventType::Error,
                        timestamp_ms,
                        details: format!("High latency: {}us", latency_us),
                    };
                    context.variables().device_events.force_push(event);
                }
            }
        }
        Ok(())
    }
}
