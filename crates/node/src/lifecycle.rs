use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeState {
    Starting = 0,
    Ready = 1,
    Draining = 2,
    Failed = 3,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Failed => "failed",
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Ready,
            2 => Self::Draining,
            3 => Self::Failed,
            _ => Self::Starting,
        }
    }
}

pub struct NodeStateMachine {
    state: AtomicU8,
}

impl NodeStateMachine {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(NodeState::Starting as u8),
        }
    }

    pub fn get(&self) -> NodeState {
        NodeState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn set(&self, s: NodeState) {
        self.state.store(s as u8, Ordering::SeqCst);
    }

    pub fn try_ready(&self) -> bool {
        self.state
            .compare_exchange(
                NodeState::Starting as u8,
                NodeState::Ready as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    pub fn begin_drain(&self) -> bool {
        let cur = self.get();
        if matches!(cur, NodeState::Ready | NodeState::Starting) {
            self.set(NodeState::Draining);
            true
        } else {
            false
        }
    }
}

impl Default for NodeStateMachine {
    fn default() -> Self {
        Self::new()
    }
}
