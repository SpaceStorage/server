use spacestorage_config::NodeConfig;
use std::sync::Mutex;

pub struct EffectiveStore {
    pending_restart: Mutex<Vec<String>>,
    #[allow(dead_code)]
    threads: u32,
    #[allow(dead_code)]
    threads_source: String,
    #[allow(dead_code)]
    handlers: Vec<String>,
}

impl EffectiveStore {
    pub fn new(
        _cfg: &NodeConfig,
        threads: u32,
        threads_source: &str,
        handlers: Vec<String>,
    ) -> Self {
        Self {
            pending_restart: Mutex::new(Vec::new()),
            threads,
            threads_source: threads_source.to_string(),
            handlers,
        }
    }

    pub fn pending_restart(&self) -> Vec<String> {
        self.pending_restart.lock().unwrap().clone()
    }

    pub fn set_pending_restart(&self, items: Vec<String>) {
        *self.pending_restart.lock().unwrap() = items;
    }
}
