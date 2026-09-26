//! Job identity, kind, status, progress (010 data-model §§1–4).

use crate::error::MigrateError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type JobId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    DataMigration,
    DataTransformation,
    DataBackup,
    DataRestore,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DataMigration => "data_migration",
            Self::DataTransformation => "data_transformation",
            Self::DataBackup => "data_backup",
            Self::DataRestore => "data_restore",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MigrateError> {
        match s {
            "data_migration" => Ok(Self::DataMigration),
            "data_transformation" => Ok(Self::DataTransformation),
            "data_backup" => Ok(Self::DataBackup),
            "data_restore" => Ok(Self::DataRestore),
            other => Err(MigrateError::UnknownKind(other.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    Live,
    Snapshot,
    Offline,
}

impl Strategy {
    pub fn parse(s: &str) -> Result<Self, MigrateError> {
        match s {
            "live" | "" => Ok(Self::Live),
            "snapshot" => Ok(Self::Snapshot),
            "offline" => Ok(Self::Offline),
            other => Err(MigrateError::NotSupported {
                what: format!("strategy {other}"),
            }),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Snapshot => "snapshot",
            Self::Offline => "offline",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProgress {
    pub bytes_copied: u64,
    pub objects_copied: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_remaining: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects_remaining: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_applied_source_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_source_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub kind: JobKind,
    pub status: JobStatus,
    /// Dual-write pause flag; `08` status stays `running`.
    pub paused: bool,
    pub principal_id: Uuid,
    pub strategy: Strategy,
    pub progress: JobProgress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Spec body (migration / transform / backup / restore JSON).
    pub body: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_internal_name: Option<String>,
}

impl JobRecord {
    pub fn new(kind: JobKind, principal_id: Uuid, strategy: Strategy, body: serde_json::Value) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind,
            status: JobStatus::Starting,
            paused: false,
            principal_id,
            strategy,
            progress: JobProgress::default(),
            error: None,
            error_code: None,
            body,
            snapshot_id: None,
            target_internal_name: None,
        }
    }

    pub fn mark_running(&mut self) {
        self.status = JobStatus::Running;
    }

    pub fn mark_completed(&mut self) {
        self.status = JobStatus::Completed;
        self.paused = false;
    }

    pub fn mark_failed(&mut self, err: &MigrateError) {
        self.status = JobStatus::Failed;
        self.paused = false;
        self.error_code = Some(err.code().into());
        self.error = Some(err.to_string());
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.status == JobStatus::Running {
            self.paused = paused;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_machine_and_paused_stays_running() {
        let mut j = JobRecord::new(
            JobKind::DataMigration,
            Uuid::nil(),
            Strategy::Live,
            serde_json::json!({}),
        );
        assert_eq!(j.status, JobStatus::Starting);
        j.mark_running();
        j.set_paused(true);
        assert!(j.paused);
        assert_eq!(j.status, JobStatus::Running);
        assert_eq!(j.status.as_str(), "running");
    }

    #[test]
    fn unknown_kind_refused() {
        assert!(matches!(
            JobKind::parse("snapshot"),
            Err(MigrateError::UnknownKind(_))
        ));
    }
}
