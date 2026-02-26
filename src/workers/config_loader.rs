use crate::{config::Config, Message, Variables};
use notify::{RecursiveMode, Watcher};
use roboplc::controller::prelude::*;
use roboplc::prelude::*;
use std::path::Path;

#[derive(WorkerOpts)]
#[worker_opts(name = "config_loader", blocking = true)]
pub struct ConfigLoader {
    config_path: String,
    config: Config,
}

impl ConfigLoader {
    pub fn new(config_path: String, config: Config) -> Self {
        Self {
            config_path,
            config,
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
                    context.hub().send(Message::ConfigUpdate {
                        config: self.config_path.clone(),
                    });
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        Ok(())
    }
}
