//! Session transactions + 2PC consume (005 US3).

use crate::error::ExecError;
use crate::options::IsolationLevel;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub type TxnId = Uuid;
pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxnState {
    Open,
    Preparing,
    Committed,
    Aborted,
    InDoubt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: TxnId,
    pub session: SessionId,
    pub protocol: String,
    pub isolation: IsolationLevel,
    pub participants: Vec<String>,
    pub distributed: bool,
    pub state: TxnState,
    /// Buffered mutate SQL applied on commit (single-node path).
    pub buffered: Vec<BufferedWrite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedWrite {
    pub namespace: String,
    pub sql: String,
}

impl Transaction {
    pub fn begin(session: SessionId, protocol: impl Into<String>, isolation: IsolationLevel) -> Self {
        Self {
            id: Uuid::now_v7(),
            session,
            protocol: protocol.into(),
            isolation,
            participants: Vec::new(),
            distributed: false,
            state: TxnState::Open,
            buffered: Vec::new(),
        }
    }

    pub fn rollback(&mut self) -> Result<(), ExecError> {
        match self.state {
            TxnState::Open | TxnState::Preparing => {
                self.state = TxnState::Aborted;
                self.buffered.clear();
                Ok(())
            }
            TxnState::Committed | TxnState::Aborted | TxnState::InDoubt => Err(ExecError::Msg(
                format!("txn_invalid_state:{:?}", self.state),
            )),
        }
    }

    pub fn prepare(&mut self) -> Result<(), ExecError> {
        if self.state != TxnState::Open {
            return Err(ExecError::Msg(format!(
                "txn_invalid_state:{:?}",
                self.state
            )));
        }
        self.state = TxnState::Preparing;
        Ok(())
    }

    pub fn commit_local(&mut self) -> Result<(), ExecError> {
        match self.state {
            TxnState::Open => {
                self.state = TxnState::Preparing;
                self.state = TxnState::Committed;
                Ok(())
            }
            TxnState::Preparing => {
                self.state = TxnState::Committed;
                Ok(())
            }
            other => Err(ExecError::Msg(format!("txn_invalid_state:{other:?}"))),
        }
    }

    pub fn mark_in_doubt(&mut self) {
        self.state = TxnState::InDoubt;
    }

    pub fn abort_conflict(&mut self, reason: &str) -> ExecError {
        self.state = TxnState::Aborted;
        self.buffered.clear();
        ExecError::RetryableTxn(reason.into())
    }
}

#[derive(Debug, Default)]
pub struct TxnRegistry {
    by_session: Mutex<HashMap<SessionId, Transaction>>,
}

impl TxnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(
        &self,
        session: SessionId,
        protocol: &str,
        isolation: IsolationLevel,
    ) -> Result<TxnId, ExecError> {
        let mut map = self.by_session.lock();
        if map.contains_key(&session) {
            return Err(ExecError::Msg("txn_already_open".into()));
        }
        let txn = Transaction::begin(session.clone(), protocol, isolation);
        let id = txn.id;
        map.insert(session, txn);
        Ok(id)
    }

    pub fn with_mut<R>(
        &self,
        session: &str,
        f: impl FnOnce(&mut Transaction) -> Result<R, ExecError>,
    ) -> Result<R, ExecError> {
        let mut map = self.by_session.lock();
        let txn = map
            .get_mut(session)
            .ok_or_else(|| ExecError::Msg("no_active_txn".into()))?;
        f(txn)
    }

    pub fn take(&self, session: &str) -> Option<Transaction> {
        self.by_session.lock().remove(session)
    }

    pub fn get(&self, session: &str) -> Option<Transaction> {
        self.by_session.lock().get(session).cloned()
    }
}

/// Invoke placement-style 2PC for multi-participant commits.
/// Single-node / no remote participants → local commit outcome.
pub fn commit_distributed(
    txn: &mut Transaction,
    participant_alive: impl Fn(&str) -> bool,
) -> Result<(), ExecError> {
    txn.prepare()?;
    if !txn.distributed || txn.participants.is_empty() {
        return txn.commit_local();
    }
    for p in &txn.participants {
        if !participant_alive(p) {
            txn.mark_in_doubt();
            return Err(ExecError::Msg("txn_in_doubt".into()));
        }
    }
    txn.commit_local()
}

pub type SharedTxnRegistry = Arc<TxnRegistry>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_commit_rollback() {
        let mut t = Transaction::begin("s1".into(), "postgresql", IsolationLevel::ReadCommitted);
        assert_eq!(t.state, TxnState::Open);
        t.commit_local().unwrap();
        assert_eq!(t.state, TxnState::Committed);

        let mut t2 = Transaction::begin("s2".into(), "postgresql", IsolationLevel::ReadCommitted);
        t2.rollback().unwrap();
        assert_eq!(t2.state, TxnState::Aborted);
    }

    #[test]
    fn distributed_in_doubt() {
        let mut t = Transaction::begin("s".into(), "postgresql", IsolationLevel::ReadCommitted);
        t.distributed = true;
        t.participants = vec!["a".into(), "b".into()];
        let err = commit_distributed(&mut t, |p| p != "b").unwrap_err();
        assert_eq!(t.state, TxnState::InDoubt);
        assert!(err.to_string().contains("in_doubt"));
    }

    #[test]
    fn retryable_on_conflict() {
        let mut t = Transaction::begin("s".into(), "postgresql", IsolationLevel::Snapshot);
        let e = t.abort_conflict("deadlock");
        assert!(matches!(e, ExecError::RetryableTxn(_)));
        assert_eq!(t.state, TxnState::Aborted);
    }
}
