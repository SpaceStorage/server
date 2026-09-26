//! Subscription / async job types (always compiled; engines behind feature).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub type ExecId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub exec_id: ExecId,
    pub principal: String,
    pub namespace: String,
    pub state: JobState,
    pub created_unix_ms: u64,
    pub error: Option<String>,
    pub result_tag: Option<String>,
}

impl Subscription {
    pub fn accepted(namespace: impl Into<String>, principal: impl Into<String>) -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            exec_id: Uuid::now_v7(),
            principal: principal.into(),
            namespace: namespace.into(),
            state: JobState::Accepted,
            created_unix_ms: ms,
            error: None,
            result_tag: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct JobRegistry {
    jobs: Mutex<HashMap<ExecId, Subscription>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&self, mut sub: Subscription) -> ExecId {
        let id = sub.exec_id;
        sub.state = JobState::Accepted;
        self.jobs.lock().insert(id, sub);
        id
    }

    pub fn set_state(&self, id: ExecId, state: JobState) {
        if let Some(j) = self.jobs.lock().get_mut(&id) {
            j.state = state;
        }
    }

    pub fn fail(&self, id: ExecId, err: impl Into<String>) {
        if let Some(j) = self.jobs.lock().get_mut(&id) {
            j.state = JobState::Failed;
            j.error = Some(err.into());
        }
    }

    pub fn succeed(&self, id: ExecId, tag: impl Into<String>) {
        if let Some(j) = self.jobs.lock().get_mut(&id) {
            j.state = JobState::Succeeded;
            j.result_tag = Some(tag.into());
        }
    }

    pub fn cancel(&self, id: ExecId) -> bool {
        let mut map = self.jobs.lock();
        if let Some(j) = map.get_mut(&id) {
            if matches!(j.state, JobState::Accepted | JobState::Running) {
                j.state = JobState::Cancelled;
                return true;
            }
        }
        false
    }

    pub fn get(&self, id: ExecId) -> Option<Subscription> {
        self.jobs.lock().get(&id).cloned()
    }

    pub fn list(&self) -> Vec<Subscription> {
        self.jobs.lock().values().cloned().collect()
    }
}

pub type SharedJobRegistry = Arc<JobRegistry>;
