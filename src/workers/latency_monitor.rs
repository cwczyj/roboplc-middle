use crate::{DeviceEvent, DeviceEventType, LatencySample, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::collections::{HashMap, VecDeque};

const LATENCY_WINDOW: usize = 100;
const SIGMA_THRESHOLD: f64 = 3.0;
const MIN_ANOMALY_SAMPLES: usize = 10;

#[derive(Debug)]
struct LatencyStats {
    samples: VecDeque<u64>,
    mean: f64,
    variance: f64,
}

impl LatencyStats {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(LATENCY_WINDOW),
            mean: 0.0,
            variance: 0.0,
        }
    }

    fn add_sample(&mut self, latency_us: u64) {
        if self.samples.len() >= LATENCY_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(latency_us);
        self.recalculate();
    }

    fn recalculate(&mut self) {
        if self.samples.is_empty() {
            self.mean = 0.0;
            self.variance = 0.0;
            return;
        }

        let n = self.samples.len() as f64;
        self.mean = self.samples.iter().sum::<u64>() as f64 / n;

        let sum_sq: f64 = self
            .samples
            .iter()
            .map(|&x| {
                let diff = x as f64 - self.mean;
                diff * diff
            })
            .sum();
        self.variance = sum_sq / n;
    }

    fn std_dev(&self) -> f64 {
        self.variance.sqrt()
    }

    fn anomaly_threshold(&self) -> Option<f64> {
        if self.samples.len() < MIN_ANOMALY_SAMPLES {
            return None;
        }
        Some(self.mean + SIGMA_THRESHOLD * self.std_dev())
    }

    fn is_anomaly(&self, latency_us: u64) -> bool {
        match self.anomaly_threshold() {
            Some(threshold) => latency_us as f64 > threshold,
            None => false,
        }
    }
}

#[derive(WorkerOpts)]
#[worker_opts(name = "latency_monitor")]
pub struct LatencyMonitor {
    latency_stats: HashMap<String, LatencyStats>,
}

impl LatencyMonitor {
    pub fn new() -> Self {
        Self {
            latency_stats: HashMap::new(),
        }
    }

    fn process_latency_sample(
        &mut self,
        device_id: &str,
        latency_us: u64,
        timestamp_ms: u64,
    ) -> Option<DeviceEvent> {
        let stats = self
            .latency_stats
            .entry(device_id.to_string())
            .or_insert_with(LatencyStats::new);

        let anomaly_threshold = stats.anomaly_threshold();
        let is_anomaly = stats.is_anomaly(latency_us);
        stats.add_sample(latency_us);

        if !is_anomaly {
            return None;
        }

        Some(DeviceEvent {
            device_id: device_id.to_string(),
            event_type: DeviceEventType::Error,
            timestamp_ms,
            details: format!(
                "Latency anomaly: {}us exceeds {:.2}us (mean {:.2}us, σ {:.2}us)",
                latency_us,
                anomaly_threshold.unwrap_or(0.0),
                stats.mean,
                stats.std_dev()
            ),
        })
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

                if let Some(event) =
                    self.process_latency_sample(&device_id, latency_us, timestamp_ms)
                {
                    context.variables().device_events.force_push(event);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_stats_maintains_rolling_window() {
        let mut stats = LatencyStats::new();

        for sample in 0..(LATENCY_WINDOW as u64 + 10) {
            stats.add_sample(sample);
        }

        assert_eq!(stats.samples.len(), LATENCY_WINDOW);
        assert_eq!(stats.samples.front().copied(), Some(10));
    }

    #[test]
    fn latency_stats_requires_minimum_samples_for_anomaly_detection() {
        let mut stats = LatencyStats::new();
        for _ in 0..9 {
            stats.add_sample(1000);
        }

        assert!(!stats.is_anomaly(10_000));
    }

    #[test]
    fn latency_stats_detects_three_sigma_outlier() {
        let mut stats = LatencyStats::new();
        for _ in 0..50 {
            stats.add_sample(1000);
        }

        assert!(stats.is_anomaly(2000));
    }

    #[test]
    fn monitor_emits_event_on_detected_anomaly() {
        let mut monitor = LatencyMonitor::new();
        let device_id = "42";

        for _ in 0..20 {
            let event = monitor.process_latency_sample(device_id, 1000, 1);
            assert!(event.is_none());
        }

        let event = monitor.process_latency_sample(device_id, 5000, 2);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.device_id, device_id);
        assert!(matches!(event.event_type, DeviceEventType::Error));
        assert!(event.details.contains("Latency anomaly"));
    }
}
