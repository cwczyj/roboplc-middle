use crate::config::Config;
use crate::{DeviceStatus, Message, Variables};
use parking_lot_rt::RwLock;
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(WorkerOpts)]
#[worker_opts(name = "config_updater")]
pub struct ConfigUpdater {
    config: Config,
}

impl ConfigUpdater {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    fn apply_config_update(
        &mut self,
        config_json: &str,
        device_states: &Arc<RwLock<HashMap<String, DeviceStatus>>>,
    ) -> Result<ConfigUpdateSummary, Box<dyn std::error::Error>> {
        let new_config: Config = serde_json::from_str(config_json)?;

        let old_device_ids: std::collections::HashSet<_> =
            self.config.devices.iter().map(|d| d.id.as_str()).collect();
        let new_device_ids: std::collections::HashSet<_> =
            new_config.devices.iter().map(|d| d.id.as_str()).collect();

        let added: Vec<_> = new_device_ids
            .difference(&old_device_ids)
            .map(|s| s.to_string())
            .collect();
        let removed: Vec<_> = old_device_ids
            .difference(&new_device_ids)
            .map(|s| s.to_string())
            .collect();
        let unchanged: Vec<_> = old_device_ids
            .intersection(&new_device_ids)
            .map(|s| s.to_string())
            .collect();

        let mut states = device_states.write();

        for device_id in &removed {
            states.remove(device_id);
        }

        for device_id in &added {
            states.insert(
                device_id.clone(),
                DeviceStatus {
                    connected: false,
                    last_communication: Instant::now(),
                    error_count: 0,
                    reconnect_count: 0,
                },
            );
        }

        self.config = new_config;

        Ok(ConfigUpdateSummary {
            added,
            removed,
            unchanged,
        })
    }
}

struct ConfigUpdateSummary {
    added: Vec<String>,
    removed: Vec<String>,
    unchanged: Vec<String>,
}

impl Worker<Message, Variables> for ConfigUpdater {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let client = context.hub().register(
            "config_updater",
            event_matches!(Message::ConfigUpdate { .. }),
        )?;

        tracing::info!("Config Updater started, waiting for config updates");

        for msg in client {
            match msg {
                Message::ConfigUpdate { config } => {
                    match self.apply_config_update(&config, &context.variables().device_states) {
                        Ok(summary) => {
                            tracing::info!(
                                added_devices = ?summary.added,
                                removed_devices = ?summary.removed,
                                unchanged_devices = ?summary.unchanged,
                                total_devices = self.config.devices.len(),
                                "Config applied successfully"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                "Failed to apply config update"
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        tracing::info!("Config Updater stopped");
        Ok(())
    }
}
