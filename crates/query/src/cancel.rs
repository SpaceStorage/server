//! Cancel + timeout token plumbing (005).

use crate::error::ExecError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> Result<(), ExecError> {
        if self.is_cancelled() {
            Err(ExecError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Run `fut` until deadline (+1s SLA grace for emit) or cancel.
pub async fn with_timeout_cancel<F, T>(
    deadline: Duration,
    token: &CancelToken,
    fut: F,
) -> Result<T, ExecError>
where
    F: std::future::Future<Output = Result<T, ExecError>>,
{
    let start = Instant::now();
    let sla = deadline.saturating_add(Duration::from_secs(1));
    tokio::select! {
        biased;
        _ = token.notify.notified(), if !token.is_cancelled() => {
            Err(ExecError::Cancelled)
        }
        r = timeout(sla, fut) => {
            match r {
                Ok(inner) => inner,
                Err(_) => {
                    // Timed out — treat as Timeout if past deadline
                    if start.elapsed() >= deadline {
                        Err(ExecError::Timeout)
                    } else {
                        Err(ExecError::Timeout)
                    }
                }
            }
        }
    }
}

/// Soft check used by execute loops (deadline already elapsed → Timeout).
pub fn check_deadline(start: Instant, deadline: Duration, token: &CancelToken) -> Result<(), ExecError> {
    token.check()?;
    if start.elapsed() >= deadline {
        Err(ExecError::Timeout)
    } else {
        Ok(())
    }
}
