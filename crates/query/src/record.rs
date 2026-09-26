//! Execution record + stages (005).

use crate::options::{Concurrency, IsolationLevel};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

pub type PlanId = Uuid;
pub type TaskId = Uuid;
pub type ExecId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStage {
    Received,
    Bound,
    Planned,
    Scheduled,
    Running { task: TaskId },
    Finalizing,
    Done,
    Failed,
    Cancelled,
}

impl ExecutionStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Received => "Received",
            Self::Bound => "Bound",
            Self::Planned => "Planned",
            Self::Scheduled => "Scheduled",
            Self::Running { .. } => "Running",
            Self::Finalizing => "Finalizing",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn transition_to(&self, next: &Self) -> bool {
        use ExecutionStage::*;
        matches!(
            (self, next),
            (Received, Bound)
                | (Received, Planned)
                | (Bound, Planned)
                | (Planned, Scheduled)
                | (Planned, Done) // EXPLAIN
                | (Scheduled, Running { .. })
                | (Running { .. }, Running { .. })
                | (Running { .. }, Finalizing)
                | (Finalizing, Done)
                | (_, Failed)
                | (_, Cancelled)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub id: ExecId,
    pub stage: ExecutionStage,
    pub plan_id: Option<PlanId>,
    pub isolation_applied: Option<IsolationLevel>,
    pub concurrency_applied: Concurrency,
    pub coordinator: String,
    pub replicas_contacted: Vec<String>,
    pub acks_durable: u16,
    pub acks_memory: u16,
    pub elapsed: Duration,
    pub spill_bytes: u64,
    /// Always `"planner"` on the production path (never a protocol name).
    pub engine: String,
    pub partial: bool,
    pub namespace: String,
    pub error: Option<String>,
}

impl ExecutionRecord {
    pub fn new(namespace: impl Into<String>, coordinator: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            stage: ExecutionStage::Received,
            plan_id: None,
            isolation_applied: None,
            concurrency_applied: Concurrency::Sequential,
            coordinator: coordinator.into(),
            replicas_contacted: Vec::new(),
            acks_durable: 0,
            acks_memory: 0,
            elapsed: Duration::ZERO,
            spill_bytes: 0,
            engine: "planner".into(),
            partial: false,
            namespace: namespace.into(),
            error: None,
        }
    }

    pub fn set_stage(&mut self, next: ExecutionStage) {
        if !self.stage.transition_to(&next) {
            #[cfg(debug_assertions)]
            panic!(
                "invalid stage transition {} → {}",
                self.stage.as_str(),
                next.as_str()
            );
            #[cfg(not(debug_assertions))]
            {
                self.stage = ExecutionStage::Failed;
                self.error = Some(format!(
                    "invalid_transition:{}->{}",
                    self.stage.as_str(),
                    next.as_str()
                ));
                return;
            }
        }
        self.stage = next;
    }
}
