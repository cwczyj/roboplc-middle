use crate::{config::Config, Message, Variables};
use notify::{RecursiveMode, Watcher};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(WorkerOpts)]
#[worker_opts(name = "config_loader", blocking = true)]
pub struct ConfigLoader {
    config_path: String,
    current_config: Config,
}

impl ConfigLoader {
    pub fn new(config_path: String, config: Config) -> Self {
        Self {
            config_path,
            current_config: config,
        }
    }

    fn reload_config(
        &mut self,
    ) -> Result<Option<(String, Vec<String>)>, Box<dyn std::error::Error>> {
        let new_config = Config::from_file(&self.config_path)?;

        let old_value = serde_json::to_value(&self.current_config)?;
        let new_value = serde_json::to_value(&new_config)?;
        let mut changed_paths = Vec::new();
        Self::collect_diff_paths("", &old_value, &new_value, &mut changed_paths);

        if changed_paths.is_empty() {
            return Ok(None);
        }

        let config_json = serde_json::to_string(&new_config)?;
        self.current_config = new_config;

        Ok(Some((config_json, changed_paths)))
    }

    fn collect_diff_paths(prefix: &str, old: &JsonValue, new: &JsonValue, out: &mut Vec<String>) {
        match (old, new) {
            (JsonValue::Object(old_map), JsonValue::Object(new_map)) => {
                let keys: BTreeSet<&str> = old_map
                    .keys()
                    .map(String::as_str)
                    .chain(new_map.keys().map(String::as_str))
                    .collect();

                for key in keys {
                    let key_path = if prefix.is_empty() {
                        key.to_string()
                    } else {
                        format!("{prefix}.{key}")
                    };

                    match (old_map.get(key), new_map.get(key)) {
                        (Some(old_val), Some(new_val)) => {
                            Self::collect_diff_paths(&key_path, old_val, new_val, out);
                        }
                        _ => out.push(key_path),
                    }
                }
            }
            (JsonValue::Array(old_arr), JsonValue::Array(new_arr)) => {
                if old_arr != new_arr {
                    out.push(prefix.to_string());
                }
            }
            _ => {
                if old != new {
                    out.push(prefix.to_string());
                }
            }
        }
    }
}

impl Worker<Message, Variables> for ConfigLoader {
    fn run(&mut self, context: &Context<Message, Variables>) -> WResult {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(Path::new(&self.config_path), RecursiveMode::NonRecursive)?;

        while context.is_online() {
            match rx.try_recv() {
                Ok(_event) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    match self.reload_config() {
                        Ok(Some((config_json, changed_paths))) => {
                            context.hub().send(Message::ConfigUpdate {
                                config: config_json,
                            });
                            tracing::info!(
                                config_path = %self.config_path,
                                changed_fields = ?changed_paths,
                                "Config reloaded"
                            );
                        }
                        Ok(None) => {
                            tracing::debug!(
                                config_path = %self.config_path,
                                "Config file event received but no content change detected"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                config_path = %self.config_path,
                                error = %e,
                                "Failed to reload config"
                            );
                        }
                    }
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn write_config(path: &Path, rpc_port: u16, http_port: u16, level: &str) {
        let content = format!(
            r#"[server]
rpc_port = {rpc_port}
http_port = {http_port}

[logging]
level = "{level}"
file = "/tmp/roboplc.log"
daily_rotation = true
"#
        );
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn reload_config_detects_diff_and_updates_current_config() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        write_config(path, 8080, 8081, "info");
        let config = Config::from_file(path).unwrap();
        let mut loader = ConfigLoader::new(path.display().to_string(), config);

        write_config(path, 9090, 8081, "debug");

        let diff = loader.reload_config().unwrap();
        assert!(diff.is_some());
        let (_config_json, changed_paths) = diff.unwrap();
        assert!(changed_paths.iter().any(|path| path == "server.rpc_port"));
        assert!(changed_paths.iter().any(|path| path == "logging.level"));
        assert_eq!(loader.current_config.server.rpc_port, 9090);
        assert_eq!(loader.current_config.logging.level, "debug");
    }
}
