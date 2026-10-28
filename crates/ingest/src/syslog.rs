//! Syslog handler: UDP+TCP plaintext; TLS=TCP only; 1:1 target container.

use crate::buffer_gate::{LocalBufferGate, SharedBufferGate};
use crate::declaration::SyslogIngestBind;
use crate::parse_rfc3164;
use crate::parse_rfc5424;
use crate::record::LogRecord;
use crate::IngestRuntime;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Result of attempting to ingest one syslog line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    Ok,
    /// Recv buffer reject — TCP should close; UDP drop.
    BufferFull,
    ParseError,
    Draining,
}

/// Recv buffer with reject policy (never wait on worker).
pub struct RecvBuffer {
    gate: SharedBufferGate,
}

impl RecvBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            gate: Arc::new(LocalBufferGate::new(capacity as u64)),
        }
    }

    pub fn from_gate(gate: SharedBufferGate) -> Self {
        Self { gate }
    }

    pub fn try_enqueue(&self, bytes: usize) -> bool {
        self.gate.try_reserve(bytes as u64)
    }

    pub fn release(&self, bytes: usize) {
        self.gate.release(bytes as u64);
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

    pub fn with_buffer_gate(mut self, gate: SharedBufferGate) -> Self {
        self.buffer = RecvBuffer::from_gate(gate);
        self
    }

    pub fn begin_drain(&self) {
        self.draining.store(1, Ordering::SeqCst);
    }

    pub fn accepting(&self) -> bool {
        self.draining.load(Ordering::SeqCst) == 0
    }

    /// Prefer 5424, fallback 3164; malformed → metric, listener not stalled.
    pub fn ingest_line(&self, line: &str) -> IngestOutcome {
        if !self.accepting() {
            return IngestOutcome::Draining;
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
            return IngestOutcome::BufferFull;
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
                IngestOutcome::Ok
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
                IngestOutcome::ParseError
            }
        }
    }
}
