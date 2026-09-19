use spacestorage_admin_proto::{BufferRange, BufferReport};
use spacestorage_config::{buffer_spec, NodeConfig, BUILTIN_BUFFERS};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    Reject,
    Wait,
}

pub struct Buffer {
    pub name: String,
    pub capacity: AtomicU64,
    pub used: AtomicU64,
    pub limit_hits: AtomicU64,
    pub policy: OverflowPolicy,
    pub min: u64,
    pub max: u64,
    pub default: u64,
}

impl Buffer {
    pub fn try_reserve(&self, n: u64) -> bool {
        loop {
            let used = self.used.load(Ordering::SeqCst);
            let cap = self.capacity.load(Ordering::SeqCst);
            if used + n > cap {
                self.limit_hits.fetch_add(1, Ordering::SeqCst);
                return false;
            }
            if self
                .used
                .compare_exchange(used, used + n, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn release(&self, n: u64) {
        self.used.fetch_sub(n.min(self.used.load(Ordering::SeqCst)), Ordering::SeqCst);
    }

    pub fn set_capacity(&self, cap: u64) {
        self.capacity.store(cap, Ordering::SeqCst);
    }

    pub fn report(&self) -> BufferReport {
        let capacity = self.capacity.load(Ordering::SeqCst);
        let used = self.used.load(Ordering::SeqCst);
        BufferReport {
            name: self.name.clone(),
            capacity_bytes: capacity,
            used_bytes: used,
            usage_ratio: if capacity == 0 {
                0.0
            } else {
                used as f64 / capacity as f64
            },
            limit_hits_total: self.limit_hits.load(Ordering::SeqCst),
            policy: match self.policy {
                OverflowPolicy::Reject => "reject",
                OverflowPolicy::Wait => "wait",
            }
            .into(),
            range: BufferRange {
                min: self.min,
                max: self.max,
            },
            default_bytes: self.default,
            owner: "01-runtime-cli-api".into(),
        }
    }
}

pub struct BufferRegistry {
    inner: Mutex<HashMap<String, Buffer>>,
}

impl BufferRegistry {
    pub fn from_config(cfg: &NodeConfig) -> Self {
        let mut map = HashMap::new();
        for (name, default, min, max) in BUILTIN_BUFFERS {
            let policy = if *name == "request.queue" {
                OverflowPolicy::Reject
            } else {
                OverflowPolicy::Wait
            };
            let cap = cfg.buffers.get(*name).copied().unwrap_or(*default);
            map.insert(
                (*name).to_string(),
                Buffer {
                    name: (*name).to_string(),
                    capacity: AtomicU64::new(cap),
                    used: AtomicU64::new(0),
                    limit_hits: AtomicU64::new(0),
                    policy,
                    min: *min,
                    max: *max,
                    default: *default,
                },
            );
        }
        Self {
            inner: Mutex::new(map),
        }
    }

    pub fn reports(&self) -> Vec<BufferReport> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<_> = g.values().map(|b| b.report()).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn apply_capacities(&self, cfg: &NodeConfig) {
        let g = self.inner.lock().unwrap();
        for (name, cap) in &cfg.buffers {
            if let Some(b) = g.get(name) {
                b.set_capacity(*cap);
            }
        }
        for (name, default, ..) in BUILTIN_BUFFERS {
            if !cfg.buffers.contains_key(*name) {
                if let Some(b) = g.get(*name) {
                    b.set_capacity(*default);
                }
            }
        }
        let _ = buffer_spec; // keep import used for future
    }
}

pub mod builtin {
    // builtins registered via BufferRegistry::from_config
}
