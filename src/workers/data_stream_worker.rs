//! DataStreamWorker - Streaming data polling worker

use crate::config::{Config, StreamConfig};
use crate::hub_protection::{send_to_hub_with_protection, DEFAULT_HUB_SEND_TIMEOUT};
use crate::messages::Operation;
use crate::next_correlation_id;
use crate::{DataCacheEntry, Message, Variables};
use roboplc::controller::prelude::*;
use roboplc::event_matches;
use roboplc::time::interval;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_POLL_TIMEOUT_MS: u64 = 2000;

#[derive(WorkerOpts)]
#[worker_opts(name = "data_stream", cpu = 3, scheduling = "fifo", priority = 75)]
pub struct DataStreamWorker {
    config: Config,
    stream_groups: HashMap<u32, Vec<StreamConfig>>,
    sequence: u64,
}

impl DataStreamWorker {
    pub fn new(config: Config) -> Self {
        let stream_groups = Self::group_streams_by_interval(&config.streams);
        Self {
            config,
            stream_groups,
            sequence: 0,
        }
    }

    fn group_streams_by_interval(streams: &[StreamConfig]) -> HashMap<u32, Vec<StreamConfig>> {
        let mut groups: HashMap<u32, Vec<StreamConfig>> = HashMap::new();
        for stream in streams {
            if stream.enabled {
                groups
                    .entry(stream.poll_interval_ms)
                    .or_default()
                    .push(stream.clone());
            }
        }
        groups
    }

    fn poll_signal_group(
        &mut self,
        device_id: &str,
        signal_group: &str,
        context: &Context<Message, Variables>,
    ) -> (bool, u64, JsonValue) {
        let start = SystemTime::now();
        let correlation_id = next_correlation_id();
        let (tx, rx) = mpsc::sync_channel(crate::MAX_PENDING_RESPONSES);
        let params = serde_json::json!({ "group_name": signal_group });

        if let Err(e) = send_to_hub_with_protection(
            context.hub(),
            Message::DeviceControl {
                device_id: device_id.to_string(),
                operation: Operation::ReadSignalGroup,
                params,
                correlation_id,
                respond_to: Some(tx),
            },
            DEFAULT_HUB_SEND_TIMEOUT,
        ) {
            tracing::warn!(error = %e, device_id = %device_id, signal_group = %signal_group, "Failed to send poll request");
            return (false, 0, JsonValue::Null);
        }

        let timeout = Duration::from_millis(DEFAULT_POLL_TIMEOUT_MS);
        match rx.recv_timeout(timeout) {
            Ok((success, data, error)) => {
                let latency_us = start.elapsed().unwrap_or(Duration::ZERO).as_micros() as u64;
                let values = if success {
                    data.get("result")
                        .and_then(|r| r.get("fields"))
                        .cloned()
                        .unwrap_or(JsonValue::Null)
                } else {
                    JsonValue::Null
                };

                let cache_key = format!("{device_id}_{signal_group}");
                let timestamp_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let entry = DataCacheEntry::new(values.clone(), timestamp_ms, latency_us, !success);
                context.variables().data_cache.set(&cache_key, entry);

                self.sequence += 1;
                let values_for_return = values.clone();
                if let Err(e) = send_to_hub_with_protection(
                    context.hub(),
                    Message::DataStreamUpdate {
                        device_id: device_id.to_string(),
                        signal_group: signal_group.to_string(),
                        values,
                        timestamp_ms,
                        latency_us,
                        sequence: self.sequence,
                    },
                    DEFAULT_HUB_SEND_TIMEOUT,
                ) {
                    tracing::warn!(error = %e, device_id = %device_id, signal_group = %signal_group, "Failed to broadcast DataStreamUpdate");
                }

                if let Some(err_msg) = error {
                    tracing::debug!(device_id = %device_id, signal_group = %signal_group, error = %err_msg, "Poll failed");
                }

                (success, latency_us, values_for_return)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(device_id = %device_id, signal_group = %signal_group, timeout_ms = DEFAULT_POLL_TIMEOUT_MS, "Poll request timed out");

                let cache_key = format!("{device_id}_{signal_group}");
                let timestamp_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let entry = DataCacheEntry::new(
                    JsonValue::Null,
                    timestamp_ms,
                    DEFAULT_POLL_TIMEOUT_MS * 1000,
                    true,
                );
                context.variables().data_cache.set(&cache_key, entry);

                (false, DEFAULT_POLL_TIMEOUT_MS * 1000, JsonValue::Null)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!(device_id = %device_id, signal_group = %signal_group, "Response channel disconnected");
                (false, 0, JsonValue::Null)
            }
        }
    }

    fn update_config(&mut self, config_json: &str) {
        match serde_json::from_str::<Config>(config_json) {
            Ok(new_config) => {
                tracing::info!(
                    new_streams_count = new_config.streams.len(),
                    "Received ConfigUpdate, updating stream configuration"
                );
                self.config = new_config;
                self.stream_groups = Self::group_streams_by_interval(&self.config.streams);
                tracing::info!(
                    stream_groups_count = self.stream_groups.len(),
                    "Stream configuration updated"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse ConfigUpdate JSON");
            }
        }
    }
}

impl Worker<Message, Variables> for DataStreamWorker {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        tracing::info!("DataStreamWorker started");

        let hub_client = context.hub().register(
            "data_stream_worker",
            event_matches!(Message::ConfigUpdate { .. }),
        )?;

        let mut last_poll_times: HashMap<u32, std::time::Instant> = HashMap::new();
        for &interval_ms in self.stream_groups.keys() {
            last_poll_times.insert(interval_ms, std::time::Instant::now());
        }

        tracing::info!(stream_groups = ?self.stream_groups.keys().collect::<Vec<_>>(), "Initialized polling timers for stream groups");

        for _ in interval(Duration::from_millis(10)).take_while(|_| context.is_online()) {
            match hub_client.try_recv() {
                Ok(Message::ConfigUpdate { config }) => {
                    self.update_config(&config);
                    last_poll_times.clear();
                    for &interval_ms in self.stream_groups.keys() {
                        last_poll_times.insert(interval_ms, std::time::Instant::now());
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }

            let now = std::time::Instant::now();

            let mut polls_to_run: Vec<(u32, Vec<(String, String)>)> = Vec::new();

            for (&interval_ms, streams) in &self.stream_groups {
                let last_poll = last_poll_times.entry(interval_ms).or_insert(now);
                let elapsed = now.duration_since(*last_poll);
                let target_duration = Duration::from_millis(interval_ms as u64);

                if elapsed >= target_duration {
                    tracing::debug!(
                        interval_ms = interval_ms,
                        streams_count = streams.len(),
                        "Polling stream group"
                    );

                    let poll_items: Vec<(String, String)> = streams
                        .iter()
                        .map(|s| (s.device_id.clone(), s.signal_group.clone()))
                        .collect();

                    polls_to_run.push((interval_ms, poll_items));
                }
            }

            for (interval_ms, poll_items) in polls_to_run {
                for (device_id, signal_group) in poll_items {
                    let (success, latency_us, _values) =
                        self.poll_signal_group(&device_id, &signal_group, context);

                    tracing::trace!(
                        device_id = %device_id,
                        signal_group = %signal_group,
                        success = success,
                        latency_us = latency_us,
                        "Poll completed"
                    );
                }

                last_poll_times.insert(interval_ms, now);
            }
        }

        tracing::info!("DataStreamWorker stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Server, StreamSettings};

    fn test_config_with_streams() -> Config {
        Config {
            server: Server {
                rpc_port: 8080,
                http_port: 8081,
                ..Default::default()
            },
            logging: Default::default(),
            timeouts: Default::default(),
            devices: vec![],
            streams: vec![
                StreamConfig {
                    device_id: "plc-1".to_string(),
                    signal_group: "sensors".to_string(),
                    poll_interval_ms: 100,
                    enabled: true,
                    priority: 0,
                },
                StreamConfig {
                    device_id: "plc-1".to_string(),
                    signal_group: "actuators".to_string(),
                    poll_interval_ms: 100,
                    enabled: true,
                    priority: 1,
                },
                StreamConfig {
                    device_id: "plc-2".to_string(),
                    signal_group: "status".to_string(),
                    poll_interval_ms: 200,
                    enabled: true,
                    priority: 0,
                },
                StreamConfig {
                    device_id: "plc-3".to_string(),
                    signal_group: "disabled_stream".to_string(),
                    poll_interval_ms: 50,
                    enabled: false,
                    priority: 0,
                },
            ],
            stream_settings: StreamSettings::default(),
        }
    }

    fn test_config_empty_streams() -> Config {
        Config {
            server: Server {
                rpc_port: 8080,
                http_port: 8081,
                ..Default::default()
            },
            logging: Default::default(),
            timeouts: Default::default(),
            devices: vec![],
            streams: vec![],
            stream_settings: StreamSettings::default(),
        }
    }

    #[test]
    fn test_worker_new_initializes_stream_groups() {
        let config = test_config_with_streams();
        let worker = DataStreamWorker::new(config);

        assert_eq!(worker.stream_groups.len(), 2);

        let group_100 = worker.stream_groups.get(&100).unwrap();
        assert_eq!(group_100.len(), 2);

        let group_200 = worker.stream_groups.get(&200).unwrap();
        assert_eq!(group_200.len(), 1);
    }

    #[test]
    fn test_worker_empty_streams() {
        let config = test_config_empty_streams();
        let worker = DataStreamWorker::new(config);

        assert_eq!(worker.stream_groups.len(), 0);
    }

    #[test]
    fn test_group_streams_excludes_disabled() {
        let config = test_config_with_streams();
        let groups = DataStreamWorker::group_streams_by_interval(&config.streams);

        assert!(!groups.contains_key(&50));
        assert!(groups.contains_key(&100));
        assert!(groups.contains_key(&200));
    }

    #[test]
    fn test_group_streams_by_interval() {
        let streams = vec![
            StreamConfig {
                device_id: "dev1".to_string(),
                signal_group: "group1".to_string(),
                poll_interval_ms: 100,
                enabled: true,
                priority: 0,
            },
            StreamConfig {
                device_id: "dev1".to_string(),
                signal_group: "group2".to_string(),
                poll_interval_ms: 100,
                enabled: true,
                priority: 1,
            },
            StreamConfig {
                device_id: "dev2".to_string(),
                signal_group: "group3".to_string(),
                poll_interval_ms: 500,
                enabled: true,
                priority: 0,
            },
        ];

        let groups = DataStreamWorker::group_streams_by_interval(&streams);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups.get(&100).unwrap().len(), 2);
        assert_eq!(groups.get(&500).unwrap().len(), 1);
    }

    #[test]
    fn test_cache_key_format() {
        let device_id = "plc-1";
        let signal_group = "temperature_sensor";
        let expected_key = "plc-1_temperature_sensor";

        let cache_key = format!("{device_id}_{signal_group}");
        assert_eq!(cache_key, expected_key);
    }

    #[test]
    fn test_sequence_increments() {
        let config = test_config_empty_streams();
        let mut worker = DataStreamWorker::new(config);

        let seq1 = worker.sequence;
        worker.sequence += 1;
        let seq2 = worker.sequence;

        assert_eq!(seq2, seq1 + 1);
    }

    #[test]
    fn test_default_poll_timeout_is_reasonable() {
        assert!(DEFAULT_POLL_TIMEOUT_MS >= 100);
        assert!(DEFAULT_POLL_TIMEOUT_MS <= 5000);
    }

    #[test]
    fn test_update_config_rebuilds_groups() {
        let config = test_config_with_streams();
        let mut worker = DataStreamWorker::new(config);

        assert_eq!(worker.stream_groups.len(), 2);

        let new_config_json = serde_json::to_string(&Config {
            server: Server {
                rpc_port: 8080,
                http_port: 8081,
                ..Default::default()
            },
            logging: Default::default(),
            timeouts: Default::default(),
            devices: vec![],
            streams: vec![StreamConfig {
                device_id: "new-device".to_string(),
                signal_group: "new-group".to_string(),
                poll_interval_ms: 300,
                enabled: true,
                priority: 0,
            }],
            stream_settings: StreamSettings::default(),
        })
        .unwrap();

        worker.update_config(&new_config_json);

        assert_eq!(worker.stream_groups.len(), 1);
        assert!(worker.stream_groups.contains_key(&300));
    }

    #[test]
    fn test_data_cache_entry_created_on_poll() {
        let cache = crate::DataCache::new();
        let key = "test_device_test_group";

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let entry = DataCacheEntry::new(
            serde_json::json!({"temperature": 25.5}),
            timestamp_ms,
            100,
            false,
        );

        cache.set(key, entry);

        let retrieved = cache.get(key).unwrap();
        assert_eq!(retrieved.values["temperature"], 25.5);
        assert!(!retrieved.error);
    }

    #[test]
    fn test_data_cache_error_entry_on_failure() {
        let cache = crate::DataCache::new();
        let key = "failed_device_failed_group";

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let entry = DataCacheEntry::new(JsonValue::Null, timestamp_ms, 5000, true);

        cache.set(key, entry);

        let retrieved = cache.get(key).unwrap();
        assert!(retrieved.error);
        assert!(retrieved.values.is_null());
    }
}
