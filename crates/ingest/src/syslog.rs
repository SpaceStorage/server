//! Syslog handler: UDP+TCP plaintext; TLS=TCP only; 1:1 target container.

use crate::declaration::SyslogIngestBind;
use crate::parse_rfc3164;
use crate::parse_rfc5424;
use crate::record::LogRecord;
use crate::IngestRuntime;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct RecvBuffer {
    capacity: usize,
    used: AtomicU64,
}

impl RecvBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: AtomicU64::new(0),
        }
    }

    pub fn try_enqueue(&self, bytes: usize) -> bool {
        let cur = self.used.load(Ordering::SeqCst) as usize;
        if cur + bytes > self.capacity {
            return false;
        }
        self.used.fetch_add(bytes as u64, Ordering::SeqCst);
        true
    }

    pub fn release(&self, bytes: usize) {
        let _ = self.used.fetch_sub(bytes as u64, Ordering::SeqCst);
    }
}

pub type StoreFn = Arc<dyn Fn(LogRecord) + Send + Sync>;

/// One dedicated listen entrypoint → one namespace/container (FR-008).
pub struct SyslogHandler {
    pub bind: SyslogIngestBind,
    pub buffer: RecvBuffer,
    pub store: StoreFn,
    pub runtime: Option<Arc<IngestRuntime>>,
    pub stored: Mutex<Vec<LogRecord>>,
    pub draining: AtomicU64, // 0=accepting, 1=drain
}

impl SyslogHandler {
    pub fn new(bind: SyslogIngestBind, store: StoreFn) -> Result<Self, crate::IngestError> {
        bind.validate()?;
        Ok(Self {
            bind,
            buffer: RecvBuffer::new(16 * 1024 * 1024),
            store,
            runtime: None,
            stored: Mutex::new(Vec::new()),
            draining: AtomicU64::new(0),
        })
    }

    pub fn with_runtime(mut self, rt: Arc<IngestRuntime>) -> Self {
        self.runtime = Some(rt);
        self
    }

    pub fn with_buffer_capacity(mut self, cap: usize) -> Self {
        self.buffer = RecvBuffer::new(cap);
        self
    }

    pub fn begin_drain(&self) {
        self.draining.store(1, Ordering::SeqCst);
    }

    pub fn accepting(&self) -> bool {
        self.draining.load(Ordering::SeqCst) == 0
    }

    /// Prefer 5424, fallback 3164; malformed → metric, listener not stalled.
    pub fn ingest_line(&self, line: &str) -> bool {
        if !self.accepting() {
            return false;
        }
        let bytes = line.len();
        if !self.buffer.try_enqueue(bytes) {
            if let Some(rt) = &self.runtime {
                rt.bump_dropped("syslog", "buffer_full");
                rt.bump_record(
                    "syslog",
                    &self.bind.namespace,
                    &self.bind.container,
                    "rfc5424",
                    "dropped",
                );
            }
            return false;
        }
        let parsed = parse_rfc5424::parse(line)
            .map(|r| ("rfc5424", r))
            .or_else(|_| parse_rfc3164::parse(line).map(|r| ("rfc3164", r)));
        self.buffer.release(bytes);
        match parsed {
            Ok((fmt, rec)) => {
                (self.store)(rec.clone());
                self.stored.lock().push(rec);
                if let Some(rt) = &self.runtime {
                    rt.bump_record(
                        "syslog",
                        &self.bind.namespace,
                        &self.bind.container,
                        fmt,
                        "ok",
                    );
                }
                true
            }
            Err(_) => {
                if let Some(rt) = &self.runtime {
                    rt.bump_parse_error(
                        "syslog",
                        &self.bind.namespace,
                        &self.bind.container,
                        "syslog_unparsed",
                    );
                    rt.bump_record(
                        "syslog",
                        &self.bind.namespace,
                        &self.bind.container,
                        "rfc5424",
                        "parse_error",
                    );
                }
                false
            }
        }
    }
}
